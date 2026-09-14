//! Repo map: an on-demand symbol map of the workspace (spec §9).
//!
//! The map is the read-side use of tree-sitter (spec §9): parse every Rust file
//! under the session cwd with the official `tags.scm` and collect the
//! `(file, name, kind)` tuples it defines. It reaches the model as an ordinary
//! tool result from the built-in `repo_map(focus?)` tool — never as a pinned
//! injection, because a "refresh when dirty" injection would push everything
//! after it out of the cached prefix every time a file changes.
//!
//! Three deliberate narrowings keep v1 explainable:
//!
//! * **Extraction is names only.** `tags.scm` captures no signatures, and v1
//!   does not reconstruct them from byte ranges; the map lists names.
//! * **Ranking is a naive pure function**, not a whole-graph PageRank: session
//!   relevance first (a `focus` the model gave, paths this session read or
//!   wrote, identifiers the recent messages used), then a structural signal
//!   (how often a name is referenced and defined). [`rank`] is the replaceable
//!   seam a weighted PageRank would drop into later.
//! * **The budget is fixed and owned by configuration**, not by the model: a
//!   1k-token default, a 4k ceiling, and no `tokens` argument.
//!
//! [`RepoMap`] holds the compiled query and a per-file mtime cache so a repeat
//! call only re-parses what changed (aider keeps the same kind of cache in
//! SQLite; an in-memory map is enough for one session).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

use crate::config::{DEFAULT_REPO_MAP_TOKENS, MAX_REPO_MAP_TOKENS};
use crate::events::{Event, EventPayload};
use crate::tools::file::{EDIT_FILE, READ_FILE, WRITE_FILE};

use super::estimate_tokens;

/// The built-in tool that renders the map. Named once here, because the module
/// owns what "a repo map" means (mirroring `context::skills::SKILL_TOOL`).
pub const REPO_MAP_TOOL: &str = "repo_map";

/// Stop walking after this many Rust files, so a pathological tree cannot make
/// one call unbounded.
const MAX_FILES: usize = 2_000;

/// Skip one file larger than this; generated Rust is not worth parsing.
const MAX_FILE_BYTES: u64 = 1_000_000;

/// Directory names never walked when a hidden directory rule does not catch
/// them. Build output and vendored dependencies.
const SKIP_DIRS: [&str; 2] = ["target", "node_modules"];

/// How many recent messages contribute identifiers to the ranking.
const RECENT_MESSAGE_COUNT: usize = 6;

/// How many recently touched paths the ranking remembers.
const MAX_RECENT_PATHS: usize = 16;

/// How many distinct recent identifiers the ranking remembers.
const MAX_RECENT_IDENTIFIERS: usize = 128;

/// Shortest token that can count as an identifier.
const MIN_IDENTIFIER_LEN: usize = 3;

/// The kind `tags.scm` assigned to a definition.
///
/// The official Rust query collapses `struct` / `enum` / `union` / type alias
/// into `definition.class`, so `Class` means "a named type", not specifically a
/// struct. `Other` is the forward-compatible slot for a capture name a future
/// `tags.scm` adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    Module,
    Macro,
    Other,
}

impl SymbolKind {
    fn from_capture(kind: &str) -> Self {
        match kind {
            "function" => SymbolKind::Function,
            "method" => SymbolKind::Method,
            "class" => SymbolKind::Class,
            "interface" => SymbolKind::Interface,
            "module" => SymbolKind::Module,
            "macro" => SymbolKind::Macro,
            _ => SymbolKind::Other,
        }
    }
}

/// One symbol defined in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub file: PathBuf,
    pub name: String,
    pub kind: SymbolKind,
}

/// What one parsed file contributed: its definitions and every name it
/// referenced. References are what the structural signal is built from; they are
/// never rendered (the map lists definitions).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSymbols {
    pub definitions: Vec<(String, SymbolKind)>,
    pub references: Vec<String>,
}

impl FileSymbols {
    /// Drop a `Function` capture whose name is also captured as `Method`: the
    /// official query matches a function inside a `declaration_list` (`impl`,
    /// `trait`, or `mod`) twice — once as `@definition.method`, once as the
    /// generic `@definition.function`. Distinct same-named methods (two `impl`s
    /// each defining `new`, say) both stay, because they are both `Method`.
    fn collapse_method_shadows(&mut self) {
        let has_method: BTreeSet<String> = self
            .definitions
            .iter()
            .filter(|(_, kind)| *kind == SymbolKind::Method)
            .map(|(name, _)| name.clone())
            .collect();
        self.definitions.retain(|(name, kind)| {
            !(*kind == SymbolKind::Function && has_method.contains(name))
        });
    }
}

/// Why a symbol ranked where it did, kept inspectable rather than folded into an
/// opaque score.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Relevance {
    /// How many distinct `focus` tokens matched the symbol's name or file path.
    pub focus_matches: u32,
    /// The symbol's file is one this session recently read or wrote.
    pub recent_path: bool,
    /// The symbol's name appeared in the recent conversation.
    pub recent_identifier: bool,
}

impl Relevance {
    /// The relevance weight. Focus is the model's explicit request, a recently
    /// touched file is strong evidence of where the work is, and a name the
    /// conversation just used is weakest of the three.
    pub fn score(&self) -> u32 {
        self.focus_matches * 4 + u32::from(self.recent_path) * 2 + u32::from(self.recent_identifier)
    }
}

/// One ranked definition, with the signals that ranked it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scored {
    pub definition: Definition,
    pub relevance: Relevance,
    /// How many times this name is referenced across the walked files.
    pub references: u32,
    /// How many sites define this name (a trait method and its impls, say).
    pub definitions: u32,
}

/// The session-derived input to [`rank`]: everything the ranking may consult
/// besides the symbols themselves.
///
/// This is a value recomputed from the event stream, not new session state, so
/// "the messages are a function of the stream" still holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RankContext {
    /// Distinct lowercased tokens from the tool's `focus` argument.
    pub focus: Vec<String>,
    /// Absolute paths recently read or written in this session, oldest first.
    pub recent_paths: Vec<PathBuf>,
    /// Distinct lowercased identifiers from the most recent messages.
    pub recent_identifiers: Vec<String>,
}

impl RankContext {
    /// Rebuild the session half of the ranking from the stream: recently read or
    /// written files plus identifiers the recent messages used.
    ///
    /// A pure query over events, like [`crate::context::skills::loaded_skill_names`]:
    /// no counter is stored, so a future compaction can rebuild it at will.
    pub fn from_session(events: &[Event], cwd: &Path) -> Self {
        let mut recent_paths: Vec<PathBuf> = Vec::new();
        for event in events {
            let EventPayload::ToolCallStarted {
                tool_name, args, ..
            } = &event.payload
            else {
                continue;
            };
            if !matches!(tool_name.as_str(), READ_FILE | WRITE_FILE | EDIT_FILE) {
                continue;
            }
            let Some(raw) = args.get("file_path").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let path = absolute(cwd, raw);
            recent_paths.retain(|known| known != &path);
            recent_paths.push(path);
        }
        if recent_paths.len() > MAX_RECENT_PATHS {
            recent_paths.drain(..recent_paths.len() - MAX_RECENT_PATHS);
        }

        let mut recent_identifiers: Vec<String> = Vec::new();
        let mut messages = 0usize;
        for event in events.iter().rev() {
            let EventPayload::MessageCompleted { text, .. } = &event.payload else {
                continue;
            };
            messages += 1;
            for token in tokenize(text) {
                if !recent_identifiers.contains(&token) {
                    recent_identifiers.push(token);
                }
                if recent_identifiers.len() >= MAX_RECENT_IDENTIFIERS {
                    break;
                }
            }
            if messages >= RECENT_MESSAGE_COUNT
                || recent_identifiers.len() >= MAX_RECENT_IDENTIFIERS
            {
                break;
            }
        }

        Self {
            focus: Vec::new(),
            recent_paths,
            recent_identifiers,
        }
    }

    /// The same context with the tool's `focus` argument folded in.
    pub fn with_focus(mut self, focus: Option<&str>) -> Self {
        self.focus = focus.map(tokenize).unwrap_or_default();
        self
    }
}

/// The session-supplied inputs the `repo_map` tool consumes: the ranking context
/// and the configured budget. They travel together from the loop to the tool
/// through the dispatch context, mirroring how the skill library is a single
/// field on [`crate::tools::PendingCall`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoMapInput {
    pub context: RankContext,
    /// The configured budget, in estimated tokens. [`RepoMap::build`] clamps it
    /// to the documented ceiling.
    pub tokens: u64,
}

impl Default for RepoMapInput {
    fn default() -> Self {
        Self {
            context: RankContext::default(),
            tokens: DEFAULT_REPO_MAP_TOKENS,
        }
    }
}

/// Rank definitions for the map (spec §9). The replaceable pure-function seam.
///
/// Order, in plain words: what the session is working on first, then how
/// structurally central a symbol is, then a stable alphabetical tiebreak so the
/// map is byte-stable across calls:
///
/// 1. [`Relevance::score`] descending;
/// 2. reference count descending, then definition count descending;
/// 3. name, then file path, ascending.
pub fn rank(
    definitions: &[Definition],
    references: &BTreeMap<String, u32>,
    context: &RankContext,
) -> Vec<Scored> {
    let mut definition_counts: BTreeMap<&str, u32> = BTreeMap::new();
    for definition in definitions {
        *definition_counts.entry(definition.name.as_str()).or_default() += 1;
    }

    let mut scored: Vec<Scored> = definitions
        .iter()
        .map(|definition| Scored {
            definition: definition.clone(),
            relevance: relevance_of(definition, context),
            references: references.get(&definition.name).copied().unwrap_or(0),
            definitions: definition_counts
                .get(definition.name.as_str())
                .copied()
                .unwrap_or(0),
        })
        .collect();

    scored.sort_by(|a, b| {
        b.relevance
            .score()
            .cmp(&a.relevance.score())
            .then_with(|| b.references.cmp(&a.references))
            .then_with(|| b.definitions.cmp(&a.definitions))
            .then_with(|| a.definition.name.cmp(&b.definition.name))
            .then_with(|| a.definition.file.cmp(&b.definition.file))
    });
    scored
}

fn relevance_of(definition: &Definition, context: &RankContext) -> Relevance {
    let name = definition.name.to_ascii_lowercase();
    let file = definition.file.to_string_lossy().to_ascii_lowercase();
    let focus_matches = context
        .focus
        .iter()
        .filter(|token| name.contains(token.as_str()) || file.contains(token.as_str()))
        .count() as u32;

    Relevance {
        focus_matches,
        recent_path: context
            .recent_paths
            .iter()
            .any(|path| path == &definition.file),
        recent_identifier: context.recent_identifiers.iter().any(|token| token == &name),
    }
}

/// Render a rank-ordered list as `<relative path>: <name>, <name>` lines, whole
/// symbols only, never exceeding `budget` estimated tokens.
///
/// Files keep the order of their best-ranked symbol; names inside a file keep
/// rank order. A truncated map always ends with a one-line count of the omitted
/// symbols — the note is the model's only signal that the map is incomplete, so
/// each symbol is admitted only if the note for the remainder still fits after
/// it.
pub fn render(ranked: &[Scored], root: &Path, budget: u64) -> String {
    if ranked.is_empty() || budget == 0 {
        return String::new();
    }

    // Group by file while preserving the order in which each file first appears,
    // which is the order of its best-ranked symbol.
    let mut order: Vec<PathBuf> = Vec::new();
    let mut groups: BTreeMap<PathBuf, Vec<&Scored>> = BTreeMap::new();
    for scored in ranked {
        groups
            .entry(scored.definition.file.clone())
            .or_insert_with(|| {
                order.push(scored.definition.file.clone());
                Vec::new()
            })
            .push(scored);
    }

    let total = ranked.len();
    let mut text = String::new();
    let mut shown = 0usize;
    'outer: for file in &order {
        let display = display_path(file, root);
        let mut opened = false;
        for scored in groups.get(file).into_iter().flatten() {
            let candidate = if opened {
                format!("{text}, {}", scored.definition.name)
            } else {
                let separator = if text.is_empty() { "" } else { "\n" };
                format!("{text}{separator}{display}: {}", scored.definition.name)
            };
            // Reserve the note at its stop-point size (the largest it will be),
            // so a symbol is admitted only when the note for the remainder fits.
            let with_note = format!("{candidate}{}", note_for(total - shown, shown > 0));
            if estimate_tokens(&candidate) > budget || estimate_tokens(&with_note) > budget {
                break 'outer;
            }
            text = candidate;
            opened = true;
            shown += 1;
        }
    }

    if shown == total {
        return text;
    }
    // The note for this `shown` fit when the last symbol was admitted, so
    // appending it cannot exceed the budget.
    format!("{text}{}", note_for(total - shown, shown > 0))
}

/// The one-line note that a truncated map ends with.
fn note_for(omitted: usize, some_shown: bool) -> String {
    let more = if some_shown { " more" } else { "" };
    format!("\n[{omitted}{more} symbol(s) omitted to fit the repo map budget]")
}

/// A repo map source: the compiled official Rust query plus a per-file mtime
/// cache.
///
/// Construct once per session (the tool owns it) and call [`RepoMap::build`] as
/// often as the model asks: unchanged files are not re-parsed, and the parser is
/// the only per-call cost that remains.
pub struct RepoMap {
    language: Language,
    query: Query,
    cache: Mutex<BTreeMap<PathBuf, CachedFile>>,
    parses: AtomicU64,
}

struct CachedFile {
    modified: Option<SystemTime>,
    len: u64,
    symbols: FileSymbols,
}

impl RepoMap {
    /// Compile the official `tags.scm` query once.
    ///
    /// Panics only if tree-sitter rejects the query that ships inside the
    /// grammar crate: a static build invariant, not a runtime condition.
    pub fn new() -> Self {
        let language: Language = tree_sitter_rust::LANGUAGE.into();
        let query = Query::new(&language, tree_sitter_rust::TAGS_QUERY)
            .expect("the official Rust tags.scm query compiles");
        Self {
            language,
            query,
            cache: Mutex::new(BTreeMap::new()),
            parses: AtomicU64::new(0),
        }
    }

    /// How many files this map has parsed since it was created. Unchanged files
    /// served from the cache do not count; a test uses this to prove the cache is
    /// doing its job.
    pub fn parses(&self) -> u64 {
        self.parses.load(Ordering::Relaxed)
    }

    /// Build the rendered map for `root`, clamped to [`MAX_MAP_TOKENS`].
    ///
    /// A workspace with no Rust files, or a query the parser cannot compile,
    /// yields an empty string rather than an error: a map is a convenience, and
    /// its absence must not fail the turn.
    pub fn build(&self, root: &Path, context: &RankContext, budget: u64) -> String {
        let budget = budget.min(MAX_REPO_MAP_TOKENS);
        let files = rust_files(root);
        if files.is_empty() {
            return String::new();
        }

        let mut parser = Parser::new();
        if parser.set_language(&self.language).is_err() {
            return String::new();
        }

        let mut symbols: Vec<(PathBuf, FileSymbols)> = Vec::with_capacity(files.len());
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for file in &files {
            let fresh = cache.get(&file.path).filter(|cached| {
                cached.modified == file.modified && cached.len == file.len
            });
            let file_symbols = match fresh {
                Some(cached) => cached.symbols.clone(),
                None => {
                    let Ok(text) = std::fs::read_to_string(&file.path) else {
                        continue;
                    };
                    let extracted = extract_with(&text, &mut parser, &self.query);
                    self.parses.fetch_add(1, Ordering::Relaxed);
                    cache.insert(
                        file.path.clone(),
                        CachedFile {
                            modified: file.modified,
                            len: file.len,
                            symbols: extracted.clone(),
                        },
                    );
                    extracted
                }
            };
            symbols.push((file.path.clone(), file_symbols));
        }
        drop(cache);

        let (definitions, references) = index(&symbols);
        let ranked = rank(&definitions, &references, context);
        render(&ranked, root, budget)
    }
}

impl Default for RepoMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the official tags query over one Rust source text, compiling the query on
/// the spot.
///
/// [`RepoMap`] keeps a compiled query instead; this convenience exists for
/// callers (and tests) that have no map, and it keeps tree-sitter an
/// implementation detail of this module rather than a type in the public API.
pub fn extract(text: &str) -> FileSymbols {
    let language: Language = tree_sitter_rust::LANGUAGE.into();
    let Ok(query) = Query::new(&language, tree_sitter_rust::TAGS_QUERY) else {
        return FileSymbols::default();
    };
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return FileSymbols::default();
    }
    extract_with(text, &mut parser, &query)
}

fn extract_with(text: &str, parser: &mut Parser, query: &Query) -> FileSymbols {
    let mut symbols = FileSymbols::default();
    let Some(tree) = parser.parse(text, None) else {
        return symbols;
    };
    let source = text.as_bytes();
    let capture_names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source);
    while let Some(matched) = matches.next() {
        let mut role: Option<&str> = None;
        let mut name: Option<&str> = None;
        for capture in matched.captures() {
            let capture_name = capture_names[capture.index as usize];
            if capture_name == "name" {
                name = capture.node.utf8_text(source).ok();
            } else if capture_name.starts_with("definition.")
                || capture_name.starts_with("reference.")
            {
                role = Some(capture_name);
            }
        }
        let (Some(role), Some(name)) = (role, name) else {
            continue;
        };
        if let Some(kind) = role.strip_prefix("definition.") {
            symbols
                .definitions
                .push((name.to_owned(), SymbolKind::from_capture(kind)));
        } else {
            symbols.references.push(name.to_owned());
        }
    }
    symbols.collapse_method_shadows();
    symbols
}

/// Fold per-file symbols into every definition plus a name -> reference count
/// index.
fn index(
    files: &[(PathBuf, FileSymbols)],
) -> (Vec<Definition>, BTreeMap<String, u32>) {
    let mut definitions = Vec::new();
    let mut references: BTreeMap<String, u32> = BTreeMap::new();
    for (path, symbols) in files {
        for (name, kind) in &symbols.definitions {
            definitions.push(Definition {
                file: path.clone(),
                name: name.clone(),
                kind: *kind,
            });
        }
        for name in &symbols.references {
            *references.entry(name.clone()).or_default() += 1;
        }
    }
    (definitions, references)
}

/// One Rust file the walker found, with the metadata the cache keys on.
struct DiscoveredFile {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
}

/// Every Rust file under `root`, sorted, bounded by [`MAX_FILES`].
///
/// Hidden directories, `target/` and `node_modules/` are skipped, and symlinks
/// are not followed: a repo map of build output or a symlink cycle helps nobody.
fn rust_files(root: &Path) -> Vec<DiscoveredFile> {
    let mut found = Vec::new();
    walk(root, &mut found);
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

fn walk(directory: &Path, found: &mut Vec<DiscoveredFile>) {
    if found.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    children.sort();
    for path in children {
        if found.len() >= MAX_FILES {
            return;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if metadata.is_dir() {
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && metadata.len() <= MAX_FILE_BYTES
        {
            found.push(DiscoveredFile {
                path,
                modified: metadata.modified().ok(),
                len: metadata.len(),
            });
        }
    }
}

/// Split free text into distinct lowercased identifier-like tokens.
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for token in text.split(|character: char| !(character.is_alphanumeric() || character == '_')) {
        if token.len() < MIN_IDENTIFIER_LEN {
            continue;
        }
        let token = token.to_ascii_lowercase();
        if !tokens.contains(&token) {
            tokens.push(token);
        }
    }
    tokens
}

/// Resolve a model-supplied path against the cwd and collapse `.` / `..` so it
/// compares equal to the absolute path the walker produced.
fn absolute(cwd: &Path, raw: &str) -> PathBuf {
    let joined = cwd.join(raw);
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// A path as it should read in the map: relative to the workspace root.
fn display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}
