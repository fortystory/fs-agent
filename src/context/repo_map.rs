//! 仓库地图：按需给出的工作区符号地图（spec §9）。
//!
//! 地图是 tree-sitter 的读侧用法（spec §9）：用官方的 `tags.scm` 解析会话 cwd 下的每一个 Rust
//! 文件，收集它定义出来的 `(文件, 名字, 种类)` 三元组。它以一条普通的工具结果的形式、从内建的
//! `repo_map(focus?)` 工具到达模型 —— 绝不是作为钉住的注入，因为一个「脏了就刷新」的注入每次文件
//! 一变就会把它之后的一切挤出缓存前缀。
//!
//! 三处刻意的收窄让 v1 讲得清楚：
//!
//! * **抽取只要名字。** `tags.scm` 不捕获签名，而 v1 不从字节区间重建它们；地图列的是名字。
//! * **排序是一个朴素的纯函数**，不是整图 PageRank：先是会话相关度（模型给的 `focus`、本会话读过
//!   或写过的路径、近期消息用过的标识符），再是一个结构信号（一个名字被引用与被定义了多少次）。
//!   [`rank`] 就是将来加权 PageRank 会落进来的那条可替换接缝。
//! * **预算是固定且归配置所有的**，不归模型：默认 1k token、天花板 4k，而且没有 `tokens` 参数。
//!
//! [`RepoMap`] 持有那条编译好的查询与一份逐文件的 mtime 缓存，所以重复调用只会重解析变过的那些
//! （aider 在 SQLite 里维护同类的缓存；对一个会话来说，一张内存里的表就够了）。

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

/// 渲染地图的那个内建工具。只在这里命名一次，因为「一份仓库地图」意味着什么由这个模块拥有
/// （与 `context::skills::SKILL_TOOL` 对称）。
pub const REPO_MAP_TOOL: &str = "repo_map";

/// 走过这么多 Rust 文件之后就停，于是一棵病态的目录树不能让一次调用变得无边无际。
const MAX_FILES: usize = 2_000;

/// 跳过比这个还大的单个文件；生成出来的 Rust 不值得解析。
const MAX_FILE_BYTES: u64 = 1_000_000;

/// 隐藏目录那条规矩没拦住的那些目录名，永不走进去。构建产物与随源码带进来的依赖。
const SKIP_DIRS: [&str; 2] = ["target", "node_modules"];

/// 有多少条最近的发言会把标识符贡献给排序。
const RECENT_MESSAGE_COUNT: usize = 6;

/// 排序记得多少条最近碰过的路径。
const MAX_RECENT_PATHS: usize = 16;

/// 排序记得多少个不同的近期标识符。
const MAX_RECENT_IDENTIFIERS: usize = 128;

/// 能算作标识符的最短 token。
const MIN_IDENTIFIER_LEN: usize = 3;

/// `tags.scm` 给一个定义安上的种类。
///
/// 官方的 Rust 查询把 `struct` / `enum` / `union` / 类型别名一起塌成 `definition.class`，所以
/// `Class` 的意思是「一个具名类型」，并不专指 struct。`Other` 是给将来某个 `tags.scm` 新增的
/// 捕获名留的向前兼容槽位。
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

/// 一个文件里定义的一个符号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub file: PathBuf,
    pub name: String,
    pub kind: SymbolKind,
}

/// 一个已解析的文件贡献了什么：它的定义，以及它引用到的每一个名字。引用是结构信号据以搭起来的
/// 东西；它们从不被渲染出来（地图列的是定义）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSymbols {
    pub definitions: Vec<(String, SymbolKind)>,
    pub references: Vec<String>,
}

impl FileSymbols {
    /// 丢掉那种「名字也被捕获成 `Method`」的 `Function` 捕获：官方查询会把 `declaration_list`
    /// （`impl`、`trait` 或 `mod`）里的一个函数匹配两次 —— 一次作为 `@definition.method`，一次作为
    /// 泛化的 `@definition.function`。同名的不同方法（比如说两个 `impl` 各自定义一个 `new`）都留
    /// 下来，因为它们都是 `Method`。
    fn collapse_method_shadows(&mut self) {
        let has_method: BTreeSet<String> = self
            .definitions
            .iter()
            .filter(|(_, kind)| *kind == SymbolKind::Method)
            .map(|(name, _)| name.clone())
            .collect();
        self.definitions
            .retain(|(name, kind)| !(*kind == SymbolKind::Function && has_method.contains(name)));
    }
}

/// 一个符号为什么排在那个位置：留着可查，而不是折进一个不透明的分数里。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Relevance {
    /// 有多少个不同的 `focus` token 命中了这个符号的名字或文件路径。
    pub focus_matches: u32,
    /// 这个符号所在的文件是本会话最近读过或写过的。
    pub recent_path: bool,
    /// 这个符号的名字在近期对话里出现过。
    pub recent_identifier: bool,
}

impl Relevance {
    /// 相关度权重。focus 是模型明确的请求，最近碰过的文件是「活在哪」的强证据，而对话刚用过的名字
    /// 是三者里最弱的。
    pub fn score(&self) -> u32 {
        self.focus_matches * 4 + u32::from(self.recent_path) * 2 + u32::from(self.recent_identifier)
    }
}

/// 一个已排好序的定义，连同给它排序的那些信号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scored {
    pub definition: Definition,
    pub relevance: Relevance,
    /// 这个名字在走过的那些文件里被引用了多少次。
    pub references: u32,
    /// 有多少个地方定义了这个名字（比如说一个 trait 方法及其各 impl）。
    pub definitions: u32,
}

/// [`rank`] 的会话派生输入：排序除符号本身之外可以查的一切。
///
/// 这是一个从事件流重算出来的值，不是新的会话状态，所以「messages 是流的一个函数」依旧成立。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RankContext {
    /// 工具 `focus` 参数里那些去重、小写化之后的 token。
    pub focus: Vec<String>,
    /// 本会话最近读过或写过的绝对路径，最旧的在前。
    pub recent_paths: Vec<PathBuf>,
    /// 最近那些发言里去重、小写化之后的标识符。
    pub recent_identifiers: Vec<String>,
}

impl RankContext {
    /// 从流上重建排序的会话那一半：最近读过或写过的文件，加上近期发言用过的标识符。
    ///
    /// 与 [`crate::context::skills::loaded_skill_names`] 一样是对事件的一个纯查询：不存任何计数，
    /// 所以将来的一次压缩可以随意重建它。
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

    /// 同一个上下文，把工具给的 `focus` 参数折了进去。
    pub fn with_focus(mut self, focus: Option<&str>) -> Self {
        self.focus = focus.map(tokenize).unwrap_or_default();
        self
    }
}

/// `repo_map` 工具消费的那些会话提供的输入：排序上下文与配置的预算。它们一起从循环经派发上下文
/// 走到工具，与技能库在 [`crate::tools::PendingCall`] 上是单个字段的方式对称。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoMapInput {
    pub context: RankContext,
    /// 配置的预算，单位是估计 token。[`RepoMap::build`] 把它夹到文档写明的那条天花板。
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

/// 给地图里的定义排序（spec §9）。那条可替换的纯函数接缝。
///
/// 用大白话说顺序：先说会话正在做的是什么，再说一个符号在结构上有多居中，最后是一个稳定的字母序
/// 兜底，好让地图跨调用逐字节稳定：
///
/// 1. [`Relevance::score`] 降序；
/// 2. 引用次数降序，再是定义次数降序；
/// 3. 名字，再是文件路径，升序。
pub fn rank(
    definitions: &[Definition],
    references: &BTreeMap<String, u32>,
    context: &RankContext,
) -> Vec<Scored> {
    let mut definition_counts: BTreeMap<&str, u32> = BTreeMap::new();
    for definition in definitions {
        *definition_counts
            .entry(definition.name.as_str())
            .or_default() += 1;
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
        recent_identifier: context
            .recent_identifiers
            .iter()
            .any(|token| token == &name),
    }
}

/// 把一份按名次排好的列表渲染成 `<相对路径>: <名字>, <名字>` 那些行，只给整符号，永不超出
/// `budget` 个估计 token。
///
/// 文件保持它们最好的那个符号的名次；文件内的名字保持名次顺序。一份被截断的地图永远以一行「省掉了
/// 多少符号」的计数收尾 —— 那条说明是模型判断地图不完整的唯一信号，所以每个符号只在「为剩余部分
/// 写的那条说明在它之后仍然装得下」时才被收进来。
pub fn render(ranked: &[Scored], root: &Path, budget: u64) -> String {
    if ranked.is_empty() || budget == 0 {
        return String::new();
    }

    // 按文件分组，同时保留每个文件首次出现的顺序 —— 也就是它最好的那个符号的名次。
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
            // 按那条说明的停留点大小（它会长到的最大）预留，于是一个符号只在「为剩余部分写的那条
            // 说明装得下」时才被收进来。
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
    // 这条针对当前 `shown` 的说明在上一个符号被收进来时是装得下的，所以把它追加上去不会超预算。
    format!("{text}{}", note_for(total - shown, shown > 0))
}

/// 一份被截断的地图用来收尾的那一行说明。
fn note_for(omitted: usize, some_shown: bool) -> String {
    let more = if some_shown { "另有 " } else { "" };
    format!("\n[为装进仓库地图的预算，{more}{omitted} 个符号被省略]")
}

/// 一份仓库地图的来源：那条编译好的官方 Rust 查询，加上一份逐文件的 mtime 缓存。
///
/// 每个会话构造一次（工具持有它），模型问几次就调几次 [`RepoMap::build`]：没变的文件不重解析，
/// 剩下唯一一次调用的代价就是那个解析器。
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
    /// 把那条官方 `tags.scm` 查询编译一次。
    ///
    /// 只在 tree-sitter 拒掉那条随语法 crate 一起发出来的查询时 panic：那是一条静态的构建不变量，
    /// 不是一个运行时状况。
    pub fn new() -> Self {
        let language: Language = tree_sitter_rust::LANGUAGE.into();
        let query = Query::new(&language, tree_sitter_rust::TAGS_QUERY)
            .expect("官方 Rust 的 tags.scm 查询编译得过");
        Self {
            language,
            query,
            cache: Mutex::new(BTreeMap::new()),
            parses: AtomicU64::new(0),
        }
    }

    /// 这张地图自建出来以后解析过多少个文件。从缓存里拿没变的文件不计入；一个测试用这个数来证明
    /// 缓存在干活。
    pub fn parses(&self) -> u64 {
        self.parses.load(Ordering::Relaxed)
    }

    /// 为 `root` 构建渲染好的地图，夹到 [`MAX_MAP_TOKENS`]。
    ///
    /// 一个没有 Rust 文件的工作区，或者一条解析器编译不过的查询，得到的是空字符串而不是错误：地图是
    /// 一个便利品，它的缺席绝不能把这一回合弄失败。
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
            let fresh = cache
                .get(&file.path)
                .filter(|cached| cached.modified == file.modified && cached.len == file.len);
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

/// 把那条官方 tags 查询跑在一段 Rust 源码上，就地编译那条查询。
///
/// [`RepoMap`] 保存的是编译好的查询；这个便利函数是给那些没有地图的调用方（以及测试）准备的，它让
/// tree-sitter 留在这个模块的实现细节里，而不是变成公开 API 里的一个类型。
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

/// 把逐文件的符号折成每一条定义，外加一张「名字 -> 引用次数」的表。
fn index(files: &[(PathBuf, FileSymbols)]) -> (Vec<Definition>, BTreeMap<String, u32>) {
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

/// 遍历器找到的一个 Rust 文件，带有缓存据以判定的那些元数据。
struct DiscoveredFile {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
}

/// `root` 下的每一个 Rust 文件，排好序，以 [`MAX_FILES`] 为界。
///
/// 隐藏目录、`target/` 与 `node_modules/` 都跳过，也不跟随符号链接：一份构建产物或一个符号链接环的
/// 仓库地图对谁都没用。
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

/// 把自由文本切成去重、小写化之后的、像标识符的 token。
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

/// 把模型给的路径解析到 cwd 上，并折叠 `.` / `..`，好让它与遍历器产出的绝对路径比得相等。
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

/// 一条路径在地图里该有的读法：相对工作区根。
fn display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}
