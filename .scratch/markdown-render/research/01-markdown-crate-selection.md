# Markdown renderers for ratatui — primary-source notes

Facts about three Rust crates that convert Markdown into ratatui text, gathered from
primary sources only: the crates.io API, the published `.crate` archives (their normalized
`Cargo.toml`, bundled `Cargo.lock`, and shipped sources), docs.rs source pages, and the
GitHub sources at the exact release tags. No blog posts, no secondary reviews, no
LLM-generated summaries.

This file is a **feature-level research note** and therefore lives in English per the
`docs/research/` exception in `AGENTS.md`. No conclusions about which crate to use are drawn
here; the last section records the repo's existing constraints so the facts above can be
read against them.

## Provenance

| Crate | Repo | Version examined | Commit (tag) | Date examined |
|---|---|---|---|---|
| `tui-markdown` | https://github.com/joshka/tui-markdown | 0.3.10 (2026-09-25) | `e6e04858f54306820cdd913714293615230519d7` (tag `tui-markdown-v0.3.10`) | 2026-10-01 |
| `ratatui-markdown` | https://github.com/celestia-island/ratatui-markdown | 0.3.6 (2026-05-21) | `9f4a2c06927859247c1c69ec8cd428facd857e6d` (tag `v0.3.6`); the archive's `.cargo_vcs_info.json` records `2609d035c42bb0c7c9b8ebf816c6717d496fb97a` **dirty** | 2026-10-01 |
| `markdown-ratatui` | https://github.com/karanabe/mira | 0.1.0 (2026-09-12) | `8803d31379644b414e185dec385aeeae640c4860` (tag `v0.1.0`; same SHA in `.cargo_vcs_info.json`) | 2026-10-01 |

Source-code citations use `https://github.com/<owner>/<repo>/blob/<tag>/<path>` so each claim
pins to the released version, not to a moving `main`/`dev` tip. Where the main-branch state
differs from the released state, that is called out explicitly.

Repo HEADs at the time of writing (for context, not for citation):

- `joshka/tui-markdown` — `9143707da309d4924938334697b8fad84d332dfe` (2026-09-28)
- `celestia-island/ratatui-markdown` — `000a97b1752841aa711203f082de4b75c5e1fd4b` (2026-09-26)
- `karanabe/mira` — `58377f20268d6ecfdb4fe10aa428a801040d79ef` (2026-09-12)

One methodological caveat used throughout: a published crate's registry-side
`.cargo_vcs_info.json` and crates.io's dependency endpoint reflect **what was packaged**, and
the tarball's `Cargo.lock` includes dev-dependencies for all feature combinations (see the
transitive-dependency caveat in §1.1). Where exact runtime-only closure counts matter they are
marked **unverified**.

**Not verified in this session:** GitHub repository star counts and contributor counts. The
GitHub REST API returned `API rate limit exceeded` for every unauthenticated request from this
environment. Where a maturity claim depends on those numbers it is marked **unverified**;
crates.io download counts and release timestamps were available and are used instead.

---

# 1. `tui-markdown` (joshka/tui-markdown) 0.3.10

## 1.1 Dependencies and version compatibility

- **The runtime dependency is `ratatui-core ^0.1`, not `ratatui`.** The published manifest
  lists `[dependencies.ratatui-core] version = "0.1", default-features = false`; `ratatui 0.30`
  appears only as a `[dev-dependencies]` entry (`default-features = false`). The crate returns
  `ratatui_core::text::Text`, which the caller renders through ratatui 0.30 (its example uses
  `frame.render_widget(text, frame.area())`).
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/Cargo.toml ·
  https://crates.io/api/v1/crates/tui-markdown/0.3.10/dependencies ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/lib.rs
- **Normal (runtime) dependencies are four unconditional crates plus two optional ones behind
  the default feature:** `itertools ^0.15`, `pulldown-cmark ^0.13`, `ratatui-core ^0.1`,
  `tracing ^0.1.37`, plus `syntect ^5` and `ansi-to-tui ^8` under the default `highlight-code`
  feature. An optional `document-features ^0.2.11` is only used for rustdoc feature tables.
  https://crates.io/api/v1/crates/tui-markdown/0.3.10/dependencies
- **A published `Cargo.lock` ships inside the crate and contains 149 packages**, including
  `syntect 5.3.0`, `onig 6.5.1`, `onig_sys 69.9.1`, `ratatui 0.30.2`, `ratatui-core 0.1.2`,
  and dev-dependency machinery (`insta`, `rstest`, `pretty_assertions`, `tracing-subscriber`).
  Because that lock covers dev-dependencies and all features, it is an upper bound rather than
  the runtime closure; **the exact runtime-only transitive count is unverified** (it would
  require `cargo tree` with this repo's feature selection).
  https://docs.rs/crate/tui-markdown/0.3.10/source/Cargo.lock
- **No `tree-sitter` or `tree-sitter-highlight` dependency anywhere in the manifest.** Its
  syntax highlighting is entirely syntect-based, so the tree-sitter version question does not
  arise for this crate.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/Cargo.toml
- **MSRV: `rust-version = "1.88.0"`**, declared in the workspace manifest and inherited by the
  crate. Edition 2021.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/Cargo.toml ·
  https://crates.io/api/v1/crates/tui-markdown/0.3.10

## 1.2 Tables

- **Tables are rendered, and the header row is buffered before layout.** The module doc says
  it outright: "A table must be buffered before rendering because every cell can increase its
  column's terminal display width. `TableBuilder` collects the header and body rows, then
  renders their content, alignment, padding, and Unicode box-drawing borders once
  pulldown-cmark closes the table."
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L1-L8
- **The header is visually distinguished twice over:** header cells get the
  `StyleSheet::table_header()` style (default `Style::new().bold().cyan()`), and a dedicated
  `├──┼──┤` separator line is emitted between header and body. Borders use
  `StyleSheet::table_border()` (default dark gray); body cells use `table_cell()` (default
  the surrounding style). Padding is included in the header/cell style, borders are not.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L163-L190 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/style_sheet.rs#L176-L197
- **Column widths auto-fit to content, with an optional width budget.** `column_widths()` takes
  the per-column maximum over header and body cells (floor 1). If `Options::table_width` is
  set, `fit_columns()` spends the budget on the narrowest unfinished column one cell at a time,
  never below each column's widest indivisible grapheme; without a width the table uses its
  natural content width. Markdown alignment (`:--`, `--:`, `:-:`) is honored via
  `padding()`. Content is never truncated, even at width zero.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L192-L245 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L462-L482 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/options.rs
- **Whole-table context is required: yes.** Rendering happens in `TableBuilder::render`, called
  from `end_table` after the whole table has closed; it builds `TOP_BORDER`, the header,
  `HEADER_SEPARATOR`, the body rows, and `BOTTOM_BORDER` from the computed `column_widths`.
  Rows taller than one line are padded out to the tallest cell in the row.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L60-L77 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L425-L460
- **Inline Markdown inside cells is rendered.** `TextWriter::push_span` routes spans to the
  active table cell, and the module doc states the cells parse inline content and that
  pulldown-cmark emits only inline events inside `TableCell`; inline handlers (code, bold,
  italic, links, images) write through the same sink. Snapshot tests assert link destinations
  and styled content survive inside cells, and that wrapping preserves inline styles.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/mod.rs#L414-L434 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/snapshots/tui_markdown__renderer__table__tests__table_keeps_inline_features_in_cell.snap ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/snapshots/tui_markdown__renderer__table__tests__wrapping_preserves_inline_styles.snap
- **The alignment row is consumed, not printed** (it drives `padding()` instead), and the
  rendered alignment matches the Markdown delimiter row, per the crate docs.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/lib.rs#L10-L16

## 1.3 Code highlighting

- **On by default; the feature is `highlight-code`, and it is the only default feature:**
  `default = ["highlight-code"]`, `highlight-code = ["dep:syntect", "dep:ansi-to-tui"]`.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/Cargo.toml#L15-L19
- **It can be switched off** with `default-features = false`. With the feature off, fenced
  code is still colored as a block, just not syntax-highlighted: the code style is pushed onto
  the line-style stack and applied to every code line, and the fence lines (``` ` ``` + the
  info string) are still emitted. The default code style is `Style::new().white().on_black()`,
  configurable through `StyleSheet::code`. A test snapshot pinning this behavior is compiled
  only with the feature disabled.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L48-L84 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L215-L244 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/style_sheet.rs#L60-L62
- **Engine: syntect, and the shipped build uses Oniguruma.** The highlighter is
  `syntect::easy::HighlightLines` over a `LazyLock<SyntaxSet>` initialized with
  `SyntaxSet::load_defaults_newlines()`. `syntect` is declared as `version = "5"` with default
  features, and syntect's default feature set is `default-onig = [..., "regex-onig"]`; the
  crate's own published `Cargo.lock` resolves `onig 6.5.1` + `onig_sys 69.9.1`. Rendering
  converts syntect's 24-bit ANSI output back into ratatui spans via `ansi-to-tui`.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L16-L30 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L92-L108 ·
  https://github.com/trishume/syntect/blob/master/Cargo.toml (version 5.3.0, `default = ["default-onig"]`, `default-fancy`) ·
  https://docs.rs/crate/tui-markdown/0.3.10/source/Cargo.lock
- **Can a consumer configure it to avoid Oniguruma? Not by declaring features.** Cargo
  features are additive: `tui-markdown` itself requests syntect with default features, so
  adding `syntect = { version = "5", default-features = false, features = ["default-fancy"] }`
  as a direct dependency would unify to *onig + fancy*, and syntect's `regex_impl` module is
  selected by `#[cfg(feature = "regex-onig")]` (the fancy backend is only compiled under
  `not(regex-onig)`). syntect's own Makefile confirms the intended way to switch engines is
  `cargo run --features default-fancy --no-default-features` — i.e. the *crate that depends on
  syntect* must drop its default features. `tui-markdown`'s manifest gives a downstream user no
  way to do that. Workarounds not verified here: a `[patch]`/vendored fork of the manifest, or
  an upstream change. Also note `[dependencies.syntect] version = "5"` carries no
  `default-features = false`, so even a hypothetical `syntect` feature flag on `tui-markdown`
  would need the manifest to change.
  https://github.com/trishume/syntect/blob/master/src/parsing/regex.rs#L160-L170 ·
  https://github.com/trishume/syntect/blob/master/Cargo.toml (features block) ·
  https://github.com/trishume/syntect/blob/master/Makefile (`syntest-fancy`, `update-known-failures-fancy`)
- **Language coverage and loading: syntect's bundled default syntax set, compiled in.** The
  grammars are the ones inside syntect's prebuilt `default_newlines.packdump`, loaded lazily at
  first use — no runtime file loading and no per-language Cargo features. syntect's Makefile
  generates that dump from a `testdata/DefaultPackage` checkout at release time.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L29-L30 ·
  https://github.com/trishume/syntect/blob/master/Makefile (`packs` target) ·
  https://github.com/trishume/syntect/blob/master/src/dumps.rs#L200-L219
- **The fence language tag is used as a syntect token.** `start_codeblock` takes the
  `CodeBlockKind::Fenced` info string and calls `SYNTAX_SET.find_syntax_by_token(lang)`; a hit
  starts `HighlightLines`, a miss logs `Could not find syntax for code block` and the block
  falls back to `StyleSheet::code`. The same string is printed on the opening fence line
  (`format!("{fence}{lang}")`). Indented code blocks pass `lang = ""`.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L48-L69 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L115-L130

## 1.4 Interface shape

- **Public API is two free functions.** `pub fn from_str(input: &str) -> Text<'_>` and
  `pub fn from_str_with_options<'a, S: StyleSheet>(input: &'a str, options: &Options<S>) -> Text<'a>`.
  Also public: `Options`, `ImageFallback`, `StyleSheet`, `DefaultStyleSheet`, `AlertKind`, and
  (feature-gated) `CodeTheme`, `BuiltinCodeTheme`, `CodeThemeLoadError`. `ratatui_core::text::Text`
  is the return type — i.e. the caller gets ratatui lines/spans, not a widget.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/mod.rs#L53-L95 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/lib.rs#L52-L63
- **Width-aware wrapping is the caller's job, with one exception.** The returned text may
  borrow from the input; only tables accept a width (`Options::table_width`, and re-render is
  required when the pane resizes). The docs say other Markdown blocks "are unaffected and can
  be wrapped by the consuming widget". There is no `Options` field for content width.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/lib.rs#L10-L16 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/options.rs (doc on `table_width`)
- **Streaming: one-shot parse of a complete `&str`; output borrows the input.** `from_str`
  feeds the whole string to `Parser::new_ext` once and runs the event loop to completion; the
  returned `Text<'a>` can borrow from the input, so it is not `'static` unless the input is
  owned. Re-rendering growing text means re-parsing the whole buffer on every call. The
  buffered table path additionally means a table cannot be emitted until its closing event
  arrives. There is no incremental/streaming entry point in the public API.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/mod.rs#L72-L95 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L60-L77

## 1.5 Maturity and risk

- **Release history: 26 versions, 2024-02-27 → 2026-09-25.** Cadence is irregular: four
  releases in the first four days (0.1.0 → 0.2.1), then roughly monthly maintenance through
  2024, sparse through 2025, then 0.3.8 → 0.3.9 → 0.3.10 in the last three months of the
  window. 0.3.10 itself has only 913 downloads so far.
  https://crates.io/api/v1/crates/tui-markdown/versions · https://crates.io/api/v1/crates/tui-markdown
- **There have been breaking 0.x jumps.** `0.2.0` (2024-02-27), `0.3.0` (2024-11-20,
  "Update compatibility to Ratatui 0.29") and `0.3.7` (2025-12-27, "Preserve heading metadata
  and update compatibility to Ratatui 0.30") each changed the compatibility surface. The
  changelog does not label any of them as breaking, but 0.3.0 and 0.3.7 are ratatui-major
  transitions.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/CHANGELOG.md ·
  https://crates.io/api/v1/crates/tui-markdown/versions
- **Downloads and reach: 498,387 lifetime, 149,790 in the recent window** (`recent_downloads`,
  the crates.io 90-day figure).
  https://crates.io/api/v1/crates/tui-markdown
- **Maintainership: effectively a solo project.** The workspace authors field is
  `authors = ["Joshka"]`, the repository owner is `joshka`, and the crate was created
  2024-02-27. **Contributor count is unverified** (GitHub API rate-limited in this
  environment); the 0.3.8 → 0.3.10 changelog entries are almost entirely maintenance,
  dependency bumps, and refactors.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/Cargo.toml ·
  https://crates.io/api/v1/crates/tui-markdown ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/CHANGELOG.md
- **Packed crate size: 52,936 bytes (0.05 MB).** The tarball is ~392 KB unpacked and contains
  only `src/`, `Cargo.toml(.orig)`, `Cargo.lock`, `README.md`, `CHANGELOG.md`.
  https://crates.io/api/v1/crates/tui-markdown/0.3.10 · `tui-markdown-0.3.10.crate` from
  https://crates.io/api/v1/crates/tui-markdown/0.3.10/download
- **License: `MIT OR Apache-2.0`** in both the published manifest and the repo workspace; the
  repo carries `LICENSE-MIT` and `LICENSE-APACHE`.
  https://crates.io/api/v1/crates/tui-markdown/0.3.10 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/Cargo.toml

---

# 2. `ratatui-markdown` (celestia-island/ratatui-markdown) 0.3.6

## 2.1 Dependencies and version compatibility

- **`ratatui ^0.29`, not 0.30.** The published 0.3.6 manifest declares `ratatui = "^0.29"`;
  the crate's own README states "ratatui 0.29" as a prerequisite, and its bundled
  `Cargo.lock` resolves `ratatui 0.29.0`. The repository's current main branch has since moved
  to `ratatui = "^0.30"` — that change is **after** 0.3.6 and does not apply to the released
  artifact.
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/dependencies ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml ·
  https://docs.rs/crate/ratatui-markdown/0.3.6/source/Cargo.lock ·
  https://github.com/celestia-island/ratatui-markdown/blob/main/Cargo.toml
- **Runtime dependencies: two, plus 47 optional ones.** Always-on: `ratatui ^0.29` and
  `unicode-width ^0.2`. Optional, grouped behind features: `image 0.25` (`image`), `pest` +
  `pest_derive` (`mermaid`, `highlight-pest`), `serde_json` + `toml 0.8` (`tree`), and the
  `tree-sitter` stack (one `tree-sitter` runtime plus 39 grammar crates).
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/dependencies
- **The published `Cargo.lock` contains 264 packages**, including the whole optional
  tree-sitter stack and dev-only `resvg`/`usvg`/`tiny-skia`/`font-kit`/`fontdb`/`ratatui-image`
  machinery. This is an upper bound, not the runtime closure; **the exact runtime-only
  transitive count is unverified.**
  https://docs.rs/crate/ratatui-markdown/0.3.6/source/Cargo.lock
- **Yes, it depends on `tree-sitter` and `tree-sitter-highlight`, at `0.26`:**
  `tree-sitter = { version = "0.26", optional = true }` and
  `tree-sitter-highlight = { version = "0.26", optional = true }`, both enabled together by
  the `highlight` feature. The bundled lock resolves **`tree-sitter 0.26.9`** and
  **`tree-sitter-highlight 0.26.9`**, alongside 39 grammar crates (`tree-sitter-rust 0.24.2`,
  `tree-sitter-python 0.25.0`, … , plus the shared `tree-sitter-language 0.1.7`).
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L126-L127 ·
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/dependencies ·
  https://docs.rs/crate/ratatui-markdown/0.3.6/source/Cargo.lock
- **Coexistence with `tree-sitter 0.27`:** `^0.26` and `^0.27` are *different* semver
  requirements — for `0.x` crates Cargo treats the minor as the compatibility boundary — so
  Cargo would build **two copies** of `tree-sitter` (`0.26.x` and `0.27.x`) rather than
  unifying them. `tree-sitter-highlight 0.26` likewise cannot unify with `0.27`. The current
  main branch has already bumped to `tree-sitter = "^0.27"` / `tree-sitter-highlight = "^0.27"`,
  which would unify with this repo's `0.27` — but that is unreleased.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L126-L127 ·
  https://github.com/celestia-island/ratatui-markdown/blob/main/Cargo.toml
- **MSRV: `rust-version = "1.74"`.** Edition 2021. This is the lowest MSRV of the three
  candidates.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L1-L8 ·
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6

## 2.2 Tables

- **Tables are rendered when the `markdown` feature is on (it is in the default set).**
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L17-L34
- **Parsing buffers the whole table before the block exists.** `is_table_line` accumulates
  consecutive pipe lines into `table_buffer`; `flush_table` requires a separator row, then
  splits the row *before* the separator into `headers` and everything after into `rows`, and
  pushes one `MarkdownBlock::Table { headers, rows }`. So the parser does hold whole-table
  context, and it stores cells as **plain `String`s**.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/parser.rs#L485-L578 ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/types.rs#L31-L44
- **Rendering also needs whole-table context.** `render_table(headers, rows, theme)` computes
  `header_widths`, per-column `min_widths` (longest single token), and
  `natural_widths` (longest full cell) across *all* rows, then allocates widths against
  `self.max_width`, before emitting anything.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L659-L760
- **Header distinction: a bold header row plus separator lines.** The first row is rendered
  with `base_style = Style::default().fg(theme.get_text_color()).add_modifier(BOLD)`; an
  `├──┼──┤` line follows the header and a `├──┼──┤` line follows every body row, with the
  final one rewritten to `└──┴──┘`.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L793-L860
- **Column widths: auto-fit to content, then compressed to the pane.** Natural width per
  column; if the sum exceeds the available budget it is compressed proportionally down to
  `min_widths`; if there is surplus, the surplus is distributed by natural width. `available`
  falls back to an 80-column budget when `max_width` is too small for borders and padding.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L659-L760
- **Inline Markdown in cells is rendered.** Header and body cells both go through
  `parse_inline_formatting`, and the resulting spans are patched over the row's base style and
  then wrapped with `wrap_styled_spans_to_width`. `parse_inline_formatting` is public.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L793-L860 ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/mod.rs#L16-L20

## 2.3 Code highlighting

- **Off by default.** The default feature set is `["markdown", "scroll", "tree", "preview",
  "mermaid", "image", "viewer"]` — `highlight` and every `highlight-lang-*` feature are
  opt-in.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L17-L34
- **Engine: `tree-sitter` via `tree-sitter-highlight`.** `TreeSitterHighlighter` wraps a
  `Mutex<Highlighter>` and `CodeColors`; the `CodeHighlighter` trait returns
  `Vec<StyleSegment>`; `HighlightHooks` installs it as a `RenderHooks::render_code_block`
  override and draws its own `╭─ lang` / `╰─` frame.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/highlight/treesitter.rs ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/highlight/hooks.rs
- **Languages are feature-selected and statically linked.** Each grammar is an optional crate
  behind `highlight-lang-<name>` (37 language features plus `highlight-lang-all`); `get_lang`
  matches the fence tag (including aliases such as `py`, `js`, `ts`, `c++`, `sh`) and reads
  `X::LANGUAGE` / `X::HIGHLIGHTS_QUERY` at compile time. There is also a `highlight-pest`
  escape hatch for a custom pest-based highlighter.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L35-L75 ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/highlight/treesitter.rs#L1-L120
- **Highlight class mapping:** `HIGHLIGHT_NAMES` is a fixed 36-entry list in the same order
  `tree-sitter-highlight` emits class indices, and `highlight_to_style` maps each to a
  `CodeColors` slot.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/highlight/config.rs
- **With `highlight` off, fenced code is plain text plus a frame.** A `CodeBlock` renders a
  `╭─ <lang>` header line, the code lines wrapped to `max_width`, and a `╰─` footer, all in
  `RichTextTheme` colors — no syntax coloring. There is no `Options`-level toggle beyond the
  Cargo feature.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L309-L360 ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L530-L560

## 2.4 Interface shape

- **The renderer is a byte-level, max-width-aware one-shot:** `MarkdownRenderer::new(max_width)`,
  then `.parse(&str) -> Vec<MarkdownBlock>` and `.render(&[MarkdownBlock], &theme) -> Vec<Line<'static>>`.
  Also public: the `markdown::RenderHooks` trait (per-block overrides), `MarkdownBlock`,
  `RichTextTheme`, and the `highlight` module's `CodeHighlighter`/`TreeSitterHighlighter`/`HighlightHooks`.
  The crate additionally exports unrelated widgets (`scroll`, `tree`, `preview`, `viewer`,
  `mermaid`, `text_input`).
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/lib.rs ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/parser.rs#L57-L62
- **It wraps for you, to its stored `max_width`.** Paragraphs/headings go through
  `wrap_text_with_inline_formatting`, code blocks through `wrap_styled_spans_to_width`, and
  tables get their own budget from `max_width`; tests assert "no line exceeds max width".
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/text.rs ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/tests.rs
- **Streaming: one-shot full-text parse, but the output is owned (`Vec<Line<'static>>`).**
  There is no incremental parser and no partial-document API; every call reparses the whole
  string. Because the rendered lines are owned, storing them alongside a growing model buffer
  is possible in a way `tui-markdown`'s borrowed `Text<'_>` is not. `max_width` is fixed at
  construction, so a resize requires a new `MarkdownRenderer`.
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/mod.rs ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L78-L92

## 2.5 Maturity and risk

- **Release history: 12 versions, all published within eight days (2026-05-14 → 2026-05-21).**
  0.1.0, 0.1.1, 0.2.0, 0.2.1, 0.2.2, then 0.3.0 … 0.3.6 on 2026-05-20/21 — several releases per
  day. Nothing has been published in the ~4.5 months since.
  https://crates.io/api/v1/crates/ratatui-markdown/versions
- **Downloads: 6,002 lifetime, 4,794 recent; 5,615 of those are for 0.3.6 itself** (i.e. the
  recent window is almost entirely the latest patch, consistent with CI/mirror traffic rather
  than long-tail use).
  https://crates.io/api/v1/crates/ratatui-markdown
- **Maintainership: single author.** `authors = ["langyo <langyo.china@gmail.com>"]`; the
  repository lives under the `celestia-island` org. **Contributor counts are unverified**
  (GitHub API rate-limited).
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L1-L8
- **Packed crate size: 8,369,402 bytes (≈8.0 MiB)** — and the contents of the published
  tarball show what dominates it. Unpacked it is 9.3 MB, of which 7.9 MB is `examples/`:
  `examples/screenshots/mermaid-image.gif` alone is **7,271,378 bytes**, plus
  `examples/demo.webp` (160,490), `examples/screenshots/*.webp` (≈30–135 KB each) and
  `examples/logo.webp` (98,074). `src/` is 716 KB and `docs/` is 552 KB. So the size is
  bundled screenshots/GIFs, not code.
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6 ·
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/download (`ratatui-markdown-0.3.6.crate`)
- **The workspace license and the published license disagree at the current tips, but not for
  0.3.6.** The `v0.3.6` tag declares `license = "MIT OR Apache-2.0"` and crates.io reports the
  same for 0.3.6; the repository's current `main` Cargo.toml has since switched to
  `license = "SySL-1.0"`. Anyone pinning 0.3.6 gets MIT OR Apache-2.0 (the shipped tarball's
  `LICENSE` is the Apache-2.0 text).
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml ·
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6 ·
  https://github.com/celestia-island/ratatui-markdown/blob/main/Cargo.toml
- **Packaging caveat: the 0.3.6 archive was built from a dirty tree.**
  `.cargo_vcs_info.json` records `"sha1": "2609d035c42bb0c7c9b8ebf816c6717d496fb97a", "dirty": true`,
  which is *not* the `v0.3.6` tag commit (`9f4a2c06…`). Diffing the published sources against
  the tag shows the files relevant here (`src/lib.rs`, `src/markdown/{render,parser,inline}.rs`)
  are byte-identical, but that should not be assumed for every file.
  https://docs.rs/crate/ratatui-markdown/0.3.6/source/.cargo_vcs_info.json

---

# 3. `markdown-ratatui` (karanabe/mira) 0.1.0

## 3.1 Dependencies and version compatibility

- **`ratatui-core ^0.1.2`, never the full `ratatui` at runtime.** The published manifest
  declares `ratatui-core = { version = "0.1.2", default-features = false }`; `ratatui 0.30`
  (with `crossterm`, default-features off) is only a dev-dependency for examples/tests. The
  README states this as a design point ("Its normal dependency graph uses `ratatui-core`, not a
  terminal backend or the complete `ratatui` application crate").
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0/dependencies ·
  https://docs.rs/crate/markdown-ratatui/0.1.0/source/Cargo.toml ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/README.md#L28-L36
- **Runtime dependencies are four:** `markdown-model ^0.1.0`, `ratatui-core ^0.1.2`,
  `unicode-segmentation ^1.12`, `unicode-width ^0.2`. `markdown-model 0.1.0` in turn depends
  only on `pulldown-cmark ^0.13.4` (with default features off). There are **no Cargo features**
  on `markdown-ratatui`.
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0/dependencies ·
  https://crates.io/api/v1/crates/markdown-model/0.1.0/dependencies ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-model/Cargo.toml
- **The published `Cargo.lock` contains 95 packages**, and that figure covers dev-dependencies
  too (crossterm, signal-hook, palette, parking_lot, etc.). The runtime closure is small —
  `markdown-ratatui` + `markdown-model` + `pulldown-cmark` (+ `pulldown-cmark-escape`) +
  `ratatui-core`/`ratatui-widgets` + three unicode crates.
  https://docs.rs/crate/markdown-ratatui/0.1.0/source/Cargo.lock
- **No `tree-sitter` and no `tree-sitter-highlight` dependency at all.** The crate has no
  syntax-highlighting engine, so the coexistence question does not arise.
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0/dependencies
- **MSRV: `rust-version = "1.88"` for the crate**, while the surrounding Mira workspace
  declares `rust-version = "1.98"` and the app README says the full application needs 1.98.
  Edition **2024**.
  https://docs.rs/crate/markdown-ratatui/0.1.0/source/Cargo.toml ·
  https://github.com/karanabe/mira/blob/v0.1.0/Cargo.toml ·
  https://github.com/karanabe/mira/blob/v0.1.0/CONTRIBUTING.md

## 3.2 Tables

- **Tables are parsed into a structured model that holds the whole table.** `markdown-model`
  builds `BlockKind::Table(Table { alignments, header, rows })` while consuming pulldown-cmark
  events; `TableHead` is recognized separately, and cells are stored as **inline content**, not
  flat strings (`TableCell(Vec<Inline>)`).
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-model/src/parse.rs#L256-L278 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-model/src/model.rs#L352-L400
- **Layout also requires the whole table up front.** `Layout::table` iterates the header and
  *every* row to compute per-column widths before emitting any line, then decides between two
  layouts.
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L555-L600
- **Header distinction: its own theme style plus a separator row.**
  `Theme::heading` (default cyan + bold) is used for the header row and `Theme::text` for body
  rows; after the header, a `─┼─`-joined rule is emitted as its own line.
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L605-L646
- **Column widths: auto-fit to plain-text content, then a stacked fallback when it does not
  fit.** Each column starts at the display width of the sanitized plain text of the header and
  each cell (floor 1); if
  `sum(widths) + 3·(columns−1) + prefix.width() > layout.width`, or if
  `TablePolicy::Stacked` is set, the table is *not* drawn as a grid — the header is joined with
  `" / "` and each body cell is emitted as a `"<header>: <cell>"` labelled line. The default is
  `TablePolicy::Auto`; the other variant is `Stacked`. Alignment (`Alignment::Left/Right/Center`)
  drives padding in the grid branch.
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L55-L65 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L555-L600
- **Inline Markdown inside cells is rendered** through `inlines()`: `InlineKind::Code` gets
  `theme.code`, `Emphasis` adds ITALIC, `Strong` adds BOLD, and links/selection are handled by
  the same run builder. A test asserts alignment plus the stacked fallback keep content
  (`wide.contains("L    │ R")`, `stacked.contains("L: xxxx")`).
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L661-L700 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/tests/render.rs#L101-L121

## 3.3 Code highlighting

- **There is none, and there is no feature for it.** The manifest declares no Cargo features
  and no highlighting dependency; the README's policy list covers `Theme`, `CodePolicy`,
  `TablePolicy`, `LineLimit` and never mentions syntax highlighting.
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/README.md#L173-L178
- **The closest knob is `CodePolicy`:** `Wrap` (default, wrap long code lines at cell
  boundaries) or `Clip` (clip to the available width). Code lines otherwise use
  `Theme::code` (default yellow) as a single flat style. Language tags on fences are not used
  for anything — there is no language lookup table in the crate.
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L40-L52 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L16-L33

## 3.4 Interface shape

- **API: a view type that prepares a fixed-width layout, plus a stateful widget and its state.**
  `MarkdownView::new(&str) -> Result<Self, ParseError>` parses once;
  `MarkdownView::prepare(width: u16) -> Result<&Layout, LayoutError>` retains a single cached
  layout; `Layout::widget() -> MarkdownWidget`, which implements
  `StatefulWidget<State = ViewState>`; `Layout` exposes `headings()`, `links()`, `width()`,
  `line_count()`, `plain_lines()`. `LayoutOptions { theme, code, tables, line_limit }` and
  `LineLimit::new(n)` (max 200_000, default 100_000) are public, as are `Document`,
  `HeadingId`, `LinkId`, `ParseError`, `LinkPosition`, `CellRange`, `DocumentRow`,
  `sanitize_terminal_text`.
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/lib.rs ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/widget.rs ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L100-L150
- **Wrapping is the crate's job, but only for the width you handed it:** `prepare(width)` lays
  out for exactly that width; `MarkdownWidget::render` clips into a narrower area and does not
  rewrap. "Call `MarkdownView::prepare` with the width of the exact area passed to the widget."
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/README.md#L162-L165 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/widget.rs#L80-L100
- **Streaming: explicitly layout-once-per-document/width.** `prepare` caches one layout and
  `set_document` unconditionally discards it; `ViewState` scrolling/selection "never parse or
  lay out content". There is no incremental parse and no width-change-free API — but there is a
  clean "replace the document and re-layout" path, and the parsed `Document` can be shared via
  `Arc` across views.
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/lib.rs#L30-L90
- **Deliberate non-goals:** the README states the crate enables no raw mode/alt screen, reads
  no input, picks no key bindings, opens no links, touches no files/network, starts no async
  runtime, and never terminates a process.
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/README.md#L8-L10

## 3.5 Maturity and risk

- **Release history: exactly one version, 0.1.0, published 2026-09-12** (the same day the tag
  was cut). `markdown-model 0.1.0` was published 21 seconds earlier, in the same batch.
  https://crates.io/api/v1/crates/markdown-ratatui/versions ·
  https://crates.io/api/v1/crates/markdown-model/versions
- **Downloads: 84 lifetime (84 recent)** for `markdown-ratatui`; `markdown-model` likewise has
  89 lifetime. This is a brand-new, essentially unused crate at the time of writing.
  https://crates.io/api/v1/crates/markdown-ratatui ·
  https://crates.io/api/v1/crates/markdown-model
- **Maintainership: single author, `karanabe`**, working in the `mira` repository (the crate is
  one of three workspace members; the repository's default member is the `mira-viewer`
  application). **Contributor counts are unverified** (GitHub API rate-limited).
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/Cargo.toml ·
  https://github.com/karanabe/mira/blob/v0.1.0/Cargo.toml
- **Packed crate size: 26,462 bytes (0.025 MB).** The tarball contains `src/` (4 files, ~1.2k
  lines), `tests/render.rs`, two examples, both license files, and a `Cargo.lock`.
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0 ·
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0/download
- **License: `MIT OR Apache-2.0`**, with `LICENSE-MIT` and `LICENSE-APACHE` shipped in the
  tarball; the repository commits license texts to the published crate (HEAD commit message:
  "mira-viewer: add license texts to published crate").
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/Cargo.toml

---

# 4. This repo's existing constraints (for comparison only)

Read from the working tree at `/home/forty/code/fortystory/fs-agent` on 2026-10-01.

| Constraint | Source |
|---|---|
| `ratatui = "0.30"`, `crossterm = "0.29"` (with `event-stream`) | `Cargo.toml` |
| `tree-sitter = "0.27"`, `tree-sitter-highlight = "0.27"`, `tree-sitter-rust = "0.24"` | `Cargo.toml` |
| Deliberate decision: **do not take syntect's Oniguruma path** (no C build dependency); the tree-sitter Rust grammar also compiles a C parser, which is accepted | `Cargo.toml` comments; `docs/render.md` (Highlighting section); `docs/highlight.md` |
| Existing hand-written renderer: `pub fn to_lines(text: &str) -> Vec<Line<'static>>`, a single forward pass over `text.split('\n')` with a fence state machine | `src/render/markdown.rs` |
| That scanner renders tables row-by-row: `table_row()` splits a pipe row into cells and `table_line()` joins them with `" │ "`; `is_table_separator()` drops the delimiter row; **no column-width computation and no whole-table buffering** | `src/render/markdown.rs` |
| Existing highlight module has **no production consumer** today; it uses `tree-sitter-highlight` and currently carries only the Rust grammar | `docs/highlight.md`; `src/render/highlight.rs` |
| Render boundary is `src/render/` with three implementations (headless / plain / TUI); TUI is ratatui on crossterm in the alt screen | `docs/render.md` |

Facts above that bear directly on these constraints, restated without recommendation:

- Only `tui-markdown` uses syntect; its shipped feature set pulls the Oniguruma backend
  (`onig`/`onig_sys` in its own `Cargo.lock`), and a downstream consumer cannot remove it by
  feature-declaration because Cargo features are additive and the manifest hard-enables
  syntect's defaults (§1.3).
- Only `ratatui-markdown` uses `tree-sitter-highlight`, at `0.26` in the released 0.3.6 —
  a different semver requirement from this repo's `0.27`, so both versions would be built
  (§2.1). Its main branch has already moved to `0.27`.
- `markdown-ratatui` has no highlighting engine and the smallest manifest of the three; it
  requires `edition = "2024"` and `rust-version = "1.88"` (§3.1).
- All three compute table column widths only after the whole table is known; none of them can
  produce correctly aligned columns from a pure per-row scan (§1.2, §2.2, §3.2).
