# Parser interfaces and grammar coverage for the Markdown renderer — primary-source notes

Facts about (A) what `tui-markdown` 0.3.10 still does once its highlighter is switched off,
(B) `pulldown-cmark` 0.13's dependency tree and event model, and (C) the ten tree-sitter
grammar crates that would be needed to extend the repo's existing highlighter, gathered from
primary sources only: the crates.io sparse index and JSON API, the published `.crate` archives
(their normalized `Cargo.toml`, `bindings/rust/lib.rs`, and shipped `.scm` queries), docs.rs
API pages, and the repositories at the exact release tags. No blog posts, no secondary reviews.

This file is a **feature-level research note** and therefore lives in English per the
`docs/research/` exception in `AGENTS.md`. It records **facts, not a decision**; the middle
sections deliberately avoid recommending a route.

Where a claim is about runtime behaviour rather than a file's text, it is marked as
**measured**: the two probe programs described in `## Provenance` were compiled in this
workspace against the exact versions named, and their output is quoted. Probes are evidence of
behaviour, not of intent; where a measurement and a doc comment disagree, both are quoted.

## Provenance

| Subject | Source | Version | Identifier | Date examined |
|---|---|---|---|---|
| `tui-markdown` (source) | https://github.com/joshka/tui-markdown | 0.3.10 (2026-09-25) | tag `tui-markdown-v0.3.10` | 2026-10-01 |
| `tui-markdown` (behaviour) | probe `tuiprobe` — `tui-markdown = { version = "0.3.10", default-features = false }` | resolved 0.3.10 | `cargo run` output quoted below | 2026-10-01 |
| `tui-markdown` (deps) | probe `tuideps` — `cargo tree --edges normal` | resolved 0.3.10; `pulldown-cmark` 0.13.4, `ratatui-core` 0.1.2 | `cargo tree` output quoted below | 2026-10-01 |
| `pulldown-cmark` | https://github.com/raphlinus/pulldown-cmark and https://crates.io/crates/pulldown-cmark | 0.13.4 (2026-05-20) | `pulldown-cmark-0.13.4.crate` from static.crates.io | 2026-10-01 |
| `pulldown-cmark` (behaviour) | probe `pdcprobe` — `pulldown-cmark = { version = "0.13", default-features = false }` | resolved 0.13.4 | `cargo run` output quoted below | 2026-10-01 |
| ten grammar crates | crates.io index + API, `.crate` archives, `bindings/rust/lib.rs` | versions in §3.1 | see the table in §3.1 | 2026-10-01 |
| ten grammars (behaviour) | probe `tsprobe` — `tree-sitter 0.27` + `tree-sitter-highlight 0.27` + all ten grammars | resolved versions in §3.2 | `cargo run` output quoted below | 2026-10-01 |
| `tree-sitter-highlight` internals | docs.rs source page and the extracted local registry copy | 0.27.0 | `src/highlight.rs` | 2026-10-01 |
| `tree-sitter-language` internals | published `.crate` archives | 0.1.1, 0.1.5, 0.1.8 | `src/language.rs` / `language.rs` | 2026-10-01 |
| this repo | working tree at `/home/forty/code/fortystory/fs-agent` | — | — | 2026-10-01 |

Method notes:

- **The crates.io JSON API is reachable from this environment** (unlike the GitHub REST API,
  which returned `API rate limit exceeded` for every unauthenticated request; see the same
  caveat in note 01). Sparse-index rows (`https://index.crates.io/<path>`) carry dependencies
  and features but **no timestamps and no license**; timestamps/license/repository/owners come
  from `https://crates.io/api/v1/crates/<name>/<version>`.
- **Every behaviour claim marked "measured" comes from executing code**, because in this
  session the published docs and the docs.rs rendering of doc comments disagreed with each
  other in at least one place that matters (§1.3, §1.2). Where that happened, the measurement
  and both doc wordings are quoted side by side.
- The probes ran with a workspace-local `CARGO_HOME` because the ambient
  `~/.cargo/registry` is read-only in this sandbox; that changes nothing about resolution
  except that the version of `cc` used to build the grammar parsers came from the fresh
  index (`cc 1.2.67`) rather than the ambient cache.
- Exact symbol names in §3.1 were extracted from each crate's shipped `bindings/rust/lib.rs`,
  not from documentation.

---

# 1. `tui-markdown` 0.3.10 with the highlighter switched off

## 1.1 Dependencies with `default-features = false`

- **The crate has exactly one default feature, `highlight-code`, and the manifest is short.**
  Runtime dependencies are `itertools = "0.15"`, `pulldown-cmark = "0.13"`,
  `ratatui-core.workspace = true` (resolved 0.1.2), `tracing = "0.1.37"`, plus the three
  optional ones (`syntect = "5"`, `ansi-to-tui = "8"`, `document-features = "0.2.11"`) which
  the default feature and rustdoc respectively switch on.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/Cargo.toml#L15-L34
- **Measured: with `default-features = false`, `cargo tree --edges normal` lists 44 lines =
  the local probe crate + 43 registry crates.** The complete first-order tree, verbatim:
  `itertools 0.15.0` → `either 1.18.0`; `pulldown-cmark 0.13.4` → `bitflags 2.13.2`,
  `getopts 0.2.24` → `unicode-width 0.2.2`, `memchr 2.8.3`, `pulldown-cmark-escape 0.11.0`,
  `unicase 2.9.0`; `ratatui-core 0.1.2` → `bitflags`, `compact_str 0.9.1` → `castaway 0.2.4`
  → `rustversion 1.0.23`, `cfg-if 1.0.5`, `itoa 1.0.18`, `rustversion`, `ryu 1.0.23`,
  `static_assertions 1.1.0`, `hashbrown 0.17.1` → `allocator-api2 0.2.21`,
  `equivalent 1.0.2`, `foldhash 0.2.0`, `itertools 0.14.0`, `kasuari 0.4.12` →
  `hashbrown 0.16.1`, `thiserror 2.0.21` → `thiserror-impl` → `proc-macro2 1.0.107` →
  `unicode-ident 1.0.26`, `quote 1.0.47`, `syn 3.0.6`/`syn 2.0.119`, `lru 0.18.5`,
  `strum 0.28.0` → `strum_macros 0.28.0` → `heck 0.5.0`, `thiserror 2.0.21`,
  `unicode-segmentation 1.13.3`, `unicode-truncate 2.0.1`, `unicode-width 0.2.2`;
  `tracing 0.1.44` → `pin-project-lite 0.2.17`, `tracing-attributes 0.1.31` → same proc-macro
  crates, `tracing-core 0.1.36` → `once_cell 1.21.4`. Note that `pulldown-cmark`'s
  `getopts` and `pulldown-cmark-escape` are in the closure because `tui-markdown` takes
  `pulldown-cmark` **with default features**.
  Measured 2026-10-01; manifest at
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/Cargo.toml#L27-L34
  and published dependency list at https://crates.io/api/v1/crates/tui-markdown/0.3.10/dependencies
- **Measured: `--features highlight-code` grows the same tree to 77 lines (76 registry
  crates)**, by adding `syntect 5.3.0` (`bincode 1.3.3` → `serde 1.0.229` → `serde_core`,
  `flate2 1.1.10` → `crc32fast 1.5.2`, `miniz_oxide 0.9.1` → `adler2 2.0.1`,
  `simd-adler32 0.3.10`, `fnv 1.0.7`, `once_cell`, `onig 6.5.3` → `bitflags`, `once_cell`,
  `onig_sys 69.9.3`, `plist 1.10.1` → `base64 0.23.1`, `indexmap 2.14.2`, `quick-xml 0.42.0`,
  `serde`, `time 0.3.55` → `deranged 0.5.8`, `num-conv 0.2.2`, `powerfmt 0.2.0`,
  `time-core 0.1.9`, `regex-syntax 0.8.11`, `serde_derive`, `serde_json 1.0.151` → `itoa`,
  `memchr`, `serde_core`, `zmij 1.0.23`, `thiserror`, `walkdir 2.5.0` → `same-file 1.0.6`,
  `yaml-rust 0.4.5` → `linked-hash-map 0.5.6`) and `ansi-to-tui 8.0.1` (`nom 8.0.0` →
  `memchr`, `ratatui-core`, `simdutf8 0.1.5`, `smallvec 1.16.2`, `thiserror`).
  This is the Oniguruma path the repo has decided against, and §1.3 of note 01 already
  records that additive Cargo features give a downstream consumer no way to remove it.
  Measured 2026-10-01.
- **So the price of the feature, measured, is 33 extra crates** (76 − 43), of which
  `onig_sys` is the one that builds C (Oniguruma). No other crate in either closure is a
  C-building or build-script-heavy one, except `pulldown-cmark`'s `build.rs`, which compiles
  to an empty `main` unless the `gen-tests` feature is on (§2.1).
  Measured 2026-10-01.
- **The current version of `pulldown-cmark` that `tui-markdown` ships with is 0.13.4**, and
  the requirement is `^0.13`.
  https://crates.io/api/v1/crates/tui-markdown/0.3.10/dependencies
- **MSRV stays `1.88.0`** (workspace `rust-version`, inherited); the feature does not change
  it. https://crates.io/api/v1/crates/tui-markdown/0.3.10

## 1.2 What fenced code renders as, and whether the language tag survives

- **Measured: a fence becomes three plain lines, and the info string is glued to the opening
  fence with no space.** Input `"before\n\n```rust\nfn main() { let x = 1; }\n```\n\nafter\n"`
  with `default-features = false` produces, with display width in brackets:

  ```
  [  6] "before"
  [  0] ""
  [  7] "```rust"
  [ 24] "fn main() { let x = 1; }"
  [  3] "```"
  [  0] ""
  [  5] "after"
  ```

  The same for ```` ```json ```` yields `"```json"` / `"{\"a\": 1}"` / `"```"`, and for an
  unknown tag ```` ```zzz-unknown-lang ```` yields `"```zzz-unknown-lang"` / `"body"` /
  `"```"`. The closing fence is always `"```"` regardless of the opening fence character or
  length. Source: `start_codeblock` takes `CodeBlockKind::Fenced(lang)`, uses `lang` for the
  highlighter lookup, and then emits `format!("{fence}{lang}")` where `fence` is
  `StyleSheet::code_block_fence()`, which defaults to `"```"` — hence `"```rust"`, not
  `"``` rust"`.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L48-L69 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/style_sheet.rs#L103-L105
- **The language label is only obtainable by string-scraping that line; there is no public
  structured API that says "this is a code block, language X".** The public surface is
  `pub use crate::renderer::{from_str, from_str_with_options}` plus `Options`,
  `ImageFallback`, `StyleSheet`, `DefaultStyleSheet`, `AlertKind` and (feature-gated)
  `CodeTheme`, `BuiltinCodeTheme`, `CodeThemeLoadError`; the return type is
  `ratatui_core::text::Text<'a>`, a plain list of `Line`s.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/lib.rs#L52-L63 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/mod.rs#L53-L95
  Internally the fact is destroyed at the boundary: `start_codeblock` matches
  `CodeBlockKind::Fenced(lang)` to a `&str` and immediately formats it into the fence line, so
  no field anywhere retains it. §15 of this note's A-group question resolves as: **the info
  string is still reachable as text; the block structure is not reachable at all.**
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L52-L68
- **Indented code blocks are turned into fences too, with an empty language, and their lines
  are concatenated without newlines.** Measured: input `"text\n\n    indented code\n    more\n"`
  produces `[4] "text"`, `[0] ""`, `[3] "```"`, `[17] "indented codemore"`, `[3] "```"` —
  i.e. `"indented code" + "more"` on one line. That is the `CodeBlockKind::Indented => ""`
  path in `start_codeblock`, and the join is consistent with the text events arriving one per
  line while the no-feature path defers to `line_styles` rather than pushing lines itself.
  The same input is fine with the feature on, because `push_highlighted_text` splits on
  `LinesWithEndings`. `src/renderer/code.rs` is quoted for the match arm and the style-stack
  push; the concatenation itself was measured here, and no crate doc mentions it.
  Measured 2026-10-01;
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L52-L58
- **The no-feature path does not lose the code style:** `self.line_styles.push(self.styles.code())`
  happens only under `#[cfg(not(feature = "highlight-code"))]`, and the default code style is
  `Style::new().white().on_black()`. So "highlighting off" means flat white-on-black code, not
  unstyled text.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L57-L58 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/style_sheet.rs#L60-L62

## 1.3 `Options::table_width`: exact semantics

- **The setting's own doc comment says it wraps: "Wraps table cells to fit within `width`
  terminal columns."** The field is `pub(crate) table_width: Option<u16>` on
  `Options<S>`, set through `Options::table_width(width)`; "the budget includes borders, cell
  padding, and enclosing list or blockquote prefixes"; render again on resize.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/options.rs#L103-L124
- **Measured: it is a column-width budget, and the implementation spends it in whole
  grapheme columns; it is not truncation.** `end_table` computes
  `width = table_width − (prefix_width + indent)` and hands it to `TableBuilder::render`;
  `fit_columns` computes `budget = width.saturating_sub(3 * columns + 1)`, returns early if
  the natural widths already fit, otherwise sets every column to its `minimum_width()`
  (widest indivisible grapheme, floor 1) and then grows the narrowest unfinished column one
  column at a time while `remaining = budget.saturating_sub(sum) > 0`; the `for _ in
  0..remaining` loop simply does nothing when `budget < sum`, so the column minimums stand
  even when they exceed the pane.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L60-L77 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L192-L219
- **Measured, on the 2-column, 129-column-wide table from the crate's own test:** requesting
  `table_width(40)` yields every line exactly 40 columns wide, `table_width(19)` (the 3-column
  alignment test) yields every line exactly 19, `table_width(6)` yields **9**, and
  `table_width(1)` and `table_width(0)` also yield **9**. So "content is never truncated"
  holds, but **the output can exceed the requested budget**, and the floor is a function of
  the column count, not a constant: with `columns = n` the narrowest possible table is
  `Σ minimum_width + 3n + 1` columns, i.e. `4n + 1` when every cell's widest grapheme is 1
  column. The doc comment's phrasing for exactly this case is "If the budget cannot fit the
  borders, padding, and one grapheme per column, the table uses that minimum width instead.
  Content is never truncated, including at width zero."
  Measured 2026-10-01;
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/options.rs#L108-L111
- **Measured: `table_width` is the only width-aware knob, and everything else comes back
  unwrapped.** A 200-character paragraph returns as **one** `Line` of width 200, and a
  59-character code line inside a fence returns as one line of width 59 (see §1.2). This
  matches both the options doc ("Other Markdown blocks are unaffected and can be wrapped by
  the consuming widget") and note 01 §1.4.
  Measured 2026-10-01;
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/options.rs#L103-L108
- **Therefore for this repo's requirement "any renderer we take in must hand unwrapped lines
  back": that holds for paragraphs, headings, lists, quotes and code, and does *not* hold for
  tables, which are wrapped in-cell to the width you pass (or left at natural width if you
  pass nothing).**

## 1.4 In-cell wrapping support: yes, and its exact algorithm

- **Yes.** `TableCell::wrap(width)` returns one cloned `TableCell` when the cell already fits,
  otherwise splits the cell's spans into `StyledGrapheme`s and walks them with
  `cell_line_end`, which prefers the last whitespace boundary that still fits and otherwise
  splits at a grapheme boundary; `from_graphemes` re-merges equal-styled neighbours so a
  wrapped cell does not become one span per character; whitespace at a wrap boundary is
  dropped rather than carried to the next line. Rows are then grown to the tallest cell in
  the row (`height = wrapped.iter().map(Vec::len).max()`), and shorter cells render styled
  blank padding.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L306-L355 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L376-L391 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L425-L452
- **Measured cells do wrap with alignment preserved**; `table_width(19)` on
  `| L | R | C |` / `| :-- | --: | :-: |` / `| a bb ccc | a bb ccc | a bb ccc |` gives
  `│ L   │   R │  C  │`, `│ a   │   a │  a  │`, `│ bb  │  bb │ bb  │`, `│ ccc │ ccc │ ccc │`,
  and the crate's own test pins exactly that.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L515-L538
- **The wrapping is by grapheme with `unicode-segmentation`/`unicode-width` through
  `ratatui-core`'s `Span::styled_graphemes`/`Span::width`**, so CJK width is handled by
  ratatui rather than by an ad-hoc count; the crate's tests include dedicated CJK and emoji
  cases.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L815-L830 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L888-L910

## 1.5 Does the table lose `:---:` / `---:` alignment?

- **No — alignment is carried through from `pulldown-cmark` and applied as padding.** The
  table starts from `Vec<Alignment>` (`start_table(alignments)`), each cell is rendered by
  `render_spans(width, alignment, style)`, and `padding()` maps `Left | None → (0, rest)`,
  `Right → (rest, 0)`, `Center → (left, rest−left)`. The alignment row itself is not printed,
  because `pulldown-cmark` never emits it as text.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L28-L34 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L454-L470
- **Measured: the rendered padding matches the delimiter row** (the `:--`/`--:`/`:-:` run
  above is left/right/center respectively), and the crate's test
  `wrapped_rows_keep_alignment_and_padding` asserts it.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L515-L538
- **Header vs body distinction, for completeness:** header cells take
  `StyleSheet::table_header()` (default bold cyan) and body cells `table_cell()`; a
  `├──┼──┤` line is emitted between header and body via the `HEADER_SEPARATOR` glyph set, and
  the table is framed by `┌─┬─┐`/`└─┴─┘`.
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L17-L21 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/style_sheet.rs#L176-L197

---

# 2. `pulldown-cmark` 0.13

## 2.1 Dependency tree, purity, MSRV, license, versions

- **Latest release is 0.13.4, published 2026-05-20**; 0.13.0 was published 2025-02-12.
  `max_version = max_stable_version = 0.13.4`. The crate has 55 published versions in the
  sparse index (50 not yanked), the first of which dates to 2015 on crates.io.
  https://crates.io/api/v1/crates/pulldown-cmark ·
  https://crates.io/api/v1/crates/pulldown-cmark/0.13.4
- **It is pure Rust with no C build step.** The manifest declares
  `build = "build.rs"`, but that script's `main` calls `generate_tests_from_spec()`, which is
  an empty function unless `gen-tests` is on ("If the `gen-tests` feature is absent, this
  function will be compiled down to nothing"). `src/lib.rs` carries
  `#![cfg_attr(not(feature = "simd"), forbid(unsafe_code))]`, and the only `unsafe` in the
  crate is the opt-in SIMD block in `src/firstpass.rs` plus the `main.rs` binary, which is
  itself `#![forbid(unsafe_code)]`.
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/Cargo.toml ·
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/build.rs ·
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/src/lib.rs ·
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/src/firstpass.rs ·
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/src/main.rs
- **Measured: the whole closure with `default-features = false` is 5 packages** — `pdc-probe`,
  `pulldown-cmark 0.13.4`, `bitflags 2.13.2`, `memchr 2.8.3`, `unicase 2.9.0`. No `cc`, no
  C compiler invocation. With the default features `getopts` + `html` it grows by
  `getopts 0.2.24` (→ `unicode-width 0.2.2`) and `pulldown-cmark-escape 0.11.0`, i.e. 8
  registry crates total.
  Measured 2026-10-01; declared dependencies at
  https://crates.io/api/v1/crates/pulldown-cmark/0.13.4/dependencies
- **Normal dependencies are exactly** `bitflags ^2`, `memchr ^2.5`, `unicase ^2.6`,
  optional `getopts ^0.2`, optional `pulldown-cmark-escape ^0.11`, optional `serde ^1.0`
  (with `derive`); `default = ["getopts", "html"]` and `html = ["pulldown-cmark-escape"]`.
  `simd = ["pulldown-cmark-escape?/simd"]` is the only other feature.
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/Cargo.toml
- **MSRV: `rust-version = "1.71.1"`, edition 2021**, both in the published manifest and in the
  README ("Rustc 1.71.1 or newer is required to build the crate"). This is far below the
  repo's toolchain (rustc 1.94.0 in this environment) and below `tree-sitter 0.27`'s own 1.90.
  https://crates.io/api/v1/crates/pulldown-cmark/0.13.4 ·
  https://github.com/pulldown-cmark/pulldown-cmark/blob/v0.13.4/README.md
- **License: `MIT`** (no `OR Apache-2.0`), in both the registry metadata and the manifest.
  https://crates.io/api/v1/crates/pulldown-cmark/0.13.4
- **Owners / maintenance:** crates.io owners are `raphlinus`, `marcusklaas`, `Martin1887`;
  the repository is still `raphlinus/pulldown-cmark`; README lists the workspace members
  `bench`, `dos-fuzzer`, `fuzz`, `pulldown-cmark`, `pulldown-cmark-escape`.
  https://crates.io/api/v1/crates/pulldown-cmark/owners ·
  https://github.com/pulldown-cmark/pulldown-cmark/blob/v0.13.4/Cargo.toml
- **Downloads: 163,652,346 lifetime / 49,747,849 in the crates.io recent window**; 0.13.4 alone
  has 25,708,877. https://crates.io/api/v1/crates/pulldown-cmark

## 2.2 The table event model

- **`Tag::Table(Vec<Alignment>)` carries one alignment per column**, and the header/body
  nesting is `Table → TableHead → TableRow → TableCell` with no `TableBody` tag ("the table
  body starts immediately after the closure of the `TableHead` tag"); `TableCell`s "contain
  inline tags". `Alignment` is `None | Left | Center | Right`. All four table tags are gated
  on `Options::ENABLE_TABLES` in their own docs.
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/enum.Tag.html#variant.Table ·
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/enum.Tag.html#variant.TableHead
- **Measured event stream (all four tags enabled), for
  `| **bold** | \`code\` and [link](https://x.test/) |` / `| :-- | --: |` /
  `| ~~del~~ | ![alt](i.png) |`:**

  ```
    0 Start(Table([Left, Right]))
    1 Start(TableHead)
    2 Start(TableCell)
    3 Start(Strong)
    4 Text(Borrowed("bold"))
    5 End(Strong)
    6 End(TableCell)
    7 Start(TableCell)
    8 Code(Borrowed("code"))
    9 Text(Borrowed(" and "))
   10 Start(Link { link_type: Inline, dest_url: Borrowed("https://x.test/"), title: Borrowed(""), id: Borrowed("") })
   11 Text(Borrowed("link"))
   12 End(Link)
   13 End(TableCell)
   14 End(TableHead)
   15 Start(TableRow)
   16 Start(TableCell)
   17 Text(Borrowed("~~del~~"))
   18 End(TableCell)
   19 Start(TableCell)
   20 Start(Image { link_type: Inline, dest_url: Borrowed("i.png"), title: Borrowed(""), id: Borrowed("") })
   21 Text(Borrowed("alt"))
   22 End(Image)
   23 End(TableCell)
   24 End(TableRow)
   25 End(Table)
  ```

  Two facts fall out of it: inline events inside cells really are ordinary inline events, and
  the text `~~del~~` arrived as a plain `Text` even though `ENABLE_STRIKETHROUGH` was **not**
  set for this run — see the next bullet for that flag in isolation.
  Measured 2026-10-01; event types at
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/enum.Event.html
- **The official example for walking events is `examples/events.rs`**, shipped inside the
  published crate and reprinted here verbatim (it prints every event, indented by nesting
  depth):

  ```rust
  use std::io::Read;

  use pulldown_cmark::{Event, Parser};

  /// Show all events from the text on stdin.
  fn main() {
      let mut text = String::new();
      std::io::stdin().read_to_string(&mut text).unwrap();

      eprintln!("{text:?} -> [");
      let mut width = 0;
      for event in Parser::new(&text) {
          if let Event::End(_) = event {
              width -= 2;
          }
          eprintln!("  {:width$}{event:?}", "");
          if let Event::Start(_) = event {
              width += 2;
          }
      }
      eprintln!("]");
  }
  ```

  The published crate contains eight examples: `broken-link-callbacks.rs`, `event-filter.rs`,
  `events.rs`, `footnote-rewrite.rs`, `normalize-wikilink.rs`, `parser-map-event-print.rs`,
  `parser-map-tag-print.rs`, `string-to-string.rs`. The `match` example that names every table
  arm is rustdoc's scraped copy of the tag-printing example, visible in the `Options` docs;
  docs.rs links it as `examples/parser-map-tag-print.rs`.
  `pulldown-cmark-0.13.4.crate` (static.crates.io) ·
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/struct.Options.html

## 2.3 The fenced language tag

- **It is `Tag::CodeBlock(CodeBlockKind::Fenced(lang))`, and `lang` is a `CowStr<'a>`**, i.e.
  borrowed from the input when the info string appears verbatim and owned when it must be
  built. `CodeBlockKind` is `Indented | Fenced(CowStr<'a>)`, with helpers
  `is_indented()`, `is_fenced()`, `into_static()`. The variant doc is explicit about the
  payload: "The value contained in the tag describes the language of the code, which may be
  empty."
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/enum.CodeBlockKind.html ·
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/enum.Tag.html#variant.CodeBlock
- **Measured: the tag is the whole info string, verbatim, with no alias handling and no
  normalisation.** ```` ```rust ignore ```` yields `Start(CodeBlock(Fenced(Borrowed("rust ignore"))))`;
  ```` ``` ```` yields `Fenced(Borrowed(""))`; an indented block yields `CodeBlock(Indented)`;
  `~~~python title="x.py"` yields `Fenced(Borrowed("python title=\"x.py\""))`; and case is
  preserved (````JS` → `Some("JS")`). Alias resolution (`rs` → rust, `sh` → bash, `js` →
  javascript) is therefore **entirely the consumer's responsibility**; the crate ships no
  language table of any kind.
  Measured 2026-10-01.
- **Measured: the code text arrives as one `Text` event whose payload includes a trailing
  newline** (`Text(Borrowed("fn main() {}\n"))`), so per-line work is a matter of splitting
  that payload.

## 2.4 Which flags gate which GFM construct

| Construct | Flag | Evidence |
|---|---|---|
| Tables | `Options::ENABLE_TABLES` | measured: `Options::empty()` + `ENABLE_GFM` both leave `| A | B |` as a paragraph; `ENABLE_TABLES` produces `Start(Table([None, None]))` |
| Task lists | `Options::ENABLE_TASKLISTS` | measured: without it `- [x] done` arrives as three separate `Text` events `"["`, `"x"`, `"]"` …; with it, `Event::TaskListMarker(true)` / `(false)` |
| Strikethrough | `Options::ENABLE_STRIKETHROUGH` | measured: without it `a ~~b~~ c` is one `Text`; with it, `Text("a ")`, `Start(Strikethrough)`, `Text("b")`, `End(Strikethrough)`, `Text(" c")` |
| Footnotes | `Options::ENABLE_FOOTNOTES` (or `ENABLE_OLD_FOOTNOTES`, which implies it) | measured: without it `ref[^1]` is parsed as a `Link { link_type: Shortcut, dest_url: "note", id: "^1" }`; with it, `Event::FootnoteReference("1")` and `Tag::FootnoteDefinition("1")` |
| GFM blockquote alerts (`> [!NOTE]`) | `Options::ENABLE_GFM` | measured: `ENABLE_GFM` yields `Start(BlockQuote(Some(Note)))`; the `Tag::BlockQuote` doc says the kind is `None` without it |

- **`ENABLE_GFM` is *not* a bundle flag for the four extensions.** Measured by bit test:
  `Options::ENABLE_GFM.contains(Options::ENABLE_TABLES) == false`, and a table with only
  `ENABLE_GFM` set stayed a paragraph (quoted above). Its own doc describes it as "Misc GitHub
  Flavored Markdown features not supported in CommonMark".
  Measured 2026-10-01; https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/struct.Options.html
- **`Options::all()` in 0.13.4 is the 15 flags**
  `ENABLE_TABLES | ENABLE_FOOTNOTES | ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS |
  ENABLE_SMART_PUNCTUATION | ENABLE_HEADING_ATTRIBUTES | ENABLE_YAML_STYLE_METADATA_BLOCKS |
  ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS | ENABLE_OLD_FOOTNOTES | ENABLE_MATH | ENABLE_GFM |
  ENABLE_DEFINITION_LIST | ENABLE_SUPERSCRIPT | ENABLE_SUBSCRIPT | ENABLE_WIKILINKS`, printed
  by `Debug` as their names (they are a `bitflags` 2 value over `u32`).
  Measured 2026-10-01; https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/struct.Options.html

## 2.5 Which pulldown-cmark version each candidate uses

- **`tui-markdown` 0.3.10 requires `pulldown-cmark ^0.13` with default features on**, which is
  where its `getopts` + `pulldown-cmark-escape` come from.
  https://crates.io/api/v1/crates/tui-markdown/0.3.10/dependencies
- **`markdown-ratatui` 0.1.0 → `markdown-model 0.1.0` → `pulldown-cmark ^0.13.4` with
  `default-features = false`**, declared in the model crate's own manifest
  (`pulldown-cmark = { version = "0.13.4", default-features = false }`).
  https://crates.io/api/v1/crates/markdown-model/0.1.0/dependencies ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-model/Cargo.toml
- **`ratatui-markdown` 0.3.6 does not use `pulldown-cmark` at all.** Its complete normal
  dependency list is `ratatui ^0.29`, `unicode-width ^0.2`, and the optional groups
  `image`/`pest`/`pest_derive`/`serde_json`/`toml`/`tree-sitter`+39 grammars — no
  `pulldown-cmark` entry. It ships its own line scanner (`src/markdown/parser.rs`), which
  note 01 §2.2 already describes.
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/dependencies ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml
- **So the two crates in this comparison that use `pulldown-cmark` both use a `0.13`
  requirement; the one that hand-rolls its parser is `ratatui-markdown`.** This repo currently
  has no `pulldown-cmark` dependency at all (`Cargo.toml` lists `toml = "0.8"`, not
  `pulldown-cmark`).

---

# 3. Extending `src/render/highlight.rs` to ten languages

## 3.1 The ten grammar crates, one language at a time

All ten were resolved and compiled together against `tree-sitter 0.27.0` and
`tree-sitter-highlight 0.27.0` in probe `tsprobe` (see §3.2 for the run output). Versions,
licenses, owners, release dates and repositories below come from the crates.io API; symbol
names come from each crate's shipped `bindings/rust/lib.rs`; the dependency requirement on
`tree-sitter-language` comes from the sparse-index rows
(`https://index.crates.io/tr/ee/<name>`).

| Language | Crate | Version (published) | Rust symbols (exact) | Declared `tree-sitter-language` req | C parser? | Repo / owner |
|---|---|---|---|---|---|---|
| rust | `tree-sitter-rust` | 0.24.2 (2026-03-27) | `LANGUAGE`, `HIGHLIGHTS_QUERY`, `INJECTIONS_QUERY`, `TAGS_QUERY`, `NODE_TYPES` | `^0.1` | yes, `src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter/tree-sitter-rust · `dcreager`, `maxbrunsfeld` |
| bash | `tree-sitter-bash` | 0.25.1 (2025-12-02) | `LANGUAGE`, **`HIGHLIGHT_QUERY`** (singular), `NODE_TYPES` | `^0.1` | yes, `src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter/tree-sitter-bash · `dcreager`, `maxbrunsfeld` |
| json | `tree-sitter-json` | 0.24.8 (2024-11-11) | `LANGUAGE`, `HIGHLIGHTS_QUERY`, `NODE_TYPES` | `^0.1` | yes, `src/parser.c` | https://github.com/tree-sitter/tree-sitter-json · `maxbrunsfeld`, `sergey-sign` |
| toml | `tree-sitter-toml-ng` | 0.7.0 (2024-12-03) | `LANGUAGE`, `HIGHLIGHTS_QUERY`, `NODE_TYPES` | `^0.1` | yes, `src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter-grammars/tree-sitter-toml · `ObserverOfTime`, `github:tree-sitter-grammars:crates` |
| html | `tree-sitter-html` | 0.23.2 (2024-11-11) | `LANGUAGE`, `HIGHLIGHTS_QUERY`, `INJECTIONS_QUERY`, `NODE_TYPES` | `^0.1` | yes, `src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter/tree-sitter-html · `maxbrunsfeld`, `amaanq` |
| javascript | `tree-sitter-javascript` | 0.25.0 (2025-09-01) | `LANGUAGE`, **`HIGHLIGHT_QUERY`** (singular), `INJECTIONS_QUERY`, `JSX_HIGHLIGHT_QUERY`, `LOCALS_QUERY`, `TAGS_QUERY`, `NODE_TYPES` | `^0.1` | yes, `src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter/tree-sitter-javascript · `dcreager`, `maxbrunsfeld` |
| typescript | `tree-sitter-typescript` | 0.23.2 (2024-11-11) | **`LANGUAGE_TYPESCRIPT`**, **`LANGUAGE_TSX`**, `HIGHLIGHTS_QUERY`, `LOCALS_QUERY`, `TAGS_QUERY`, `TYPESCRIPT_NODE_TYPES`, `TSX_NODE_TYPES` | `^0.1` | yes, both grammars: `typescript/src/parser.c`+`scanner.c`, `tsx/src/parser.c`+`scanner.c` | https://github.com/tree-sitter/tree-sitter-typescript · `dcreager`, `maxbrunsfeld`, `patrickt` |
| php | `tree-sitter-php` | 0.24.2 (2025-08-18) | **`LANGUAGE_PHP`**, **`LANGUAGE_PHP_ONLY`**, `HIGHLIGHTS_QUERY`, `INJECTIONS_QUERY`, `TAGS_QUERY`, `PHP_NODE_TYPES`, `PHP_ONLY_NODE_TYPES` | `^0.1` | yes, both grammars: `php/src/parser.c`+`scanner.c`, `php_only/src/parser.c`+`scanner.c` | https://github.com/tree-sitter/tree-sitter-php · `maxbrunsfeld` |
| sql | `tree-sitter-sequel` | 0.3.11 (2025-10-01) | `LANGUAGE` (backed by C symbol `tree_sitter_sql`), `HIGHLIGHTS_QUERY`, `NODE_TYPES` | `^0.1` | yes, `src/parser.c` + `src/scanner.c` | https://github.com/derekstride/tree-sitter-sql · `DerekStride` |
| python | `tree-sitter-python` | 0.25.0 (2025-09-11) | `LANGUAGE`, `HIGHLIGHTS_QUERY`, `TAGS_QUERY`, `NODE_TYPES` | `^0.1` | yes, `src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter/tree-sitter-python · `dcreager`, `maxbrunsfeld` |

Per-language notes that do not fit the table:

- **`HIGHLIGHT_QUERY` vs `HIGHLIGHTS_QUERY` is the single sharpest API trap in this list.**
  `tree-sitter-bash` and `tree-sitter-javascript` export the **singular** name; the other eight
  export the plural. Both crates ship `queries/highlights.scm` — only the Rust constant name
  differs. Verified against the `bindings/rust/lib.rs` inside each published tarball, not
  against docs.
  `tree-sitter-bash-0.25.1.crate` · `tree-sitter-javascript-0.25.0.crate` (static.crates.io)
- **`toml`: the crate for a *maintained* TOML grammar is `tree-sitter-toml-ng`, not
  `tree-sitter-toml`.** The name `tree-sitter-toml` has exactly one version, 0.20.0 published
  **2022-01-05**, and it depends directly on `tree-sitter ^0.20`, which cannot unify with
  0.27; `tree-sitter-toml-ng` 0.7.0 depends on `tree-sitter-language ^0.1`.
  https://index.crates.io/tr/ee/tree-sitter-toml ·
  https://crates.io/api/v1/crates/tree-sitter-toml ·
  https://index.crates.io/tr/ee/tree-sitter-toml-ng
- **`sql`: the crate whose name matches the language is dead; the live one is named
  `tree-sitter-sequel`.** `tree-sitter-sql` has 2 published versions, the latest 0.0.2 from
  **2021-06-05**, and it depends directly on `tree-sitter ^0.19.3`; it is not yanked but has
  not been published in five years. Three other SQL-ish crates were checked and rejected as
  candidates for a general SQL grammar: `tree-sitter-sql-bigquery` 0.8.0 (BigQuery dialect,
  `tree-sitter >=0.19, <0.23`), `tree-sitter-sqlite3` 0.1.0 (SQLite dialect, published
  2026-05-03, 92 lifetime downloads, license `CC0-1.0`), and `sqlparser` 0.63.0 (a pure-Rust
  SQL parser, not a tree-sitter grammar at all).
  https://index.crates.io/tr/ee/tree-sitter-sql ·
  https://crates.io/api/v1/crates/tree-sitter-sql ·
  https://crates.io/api/v1/crates/tree-sitter-sql-bigquery ·
  https://crates.io/api/v1/crates/tree-sitter-sqlite3 ·
  https://crates.io/api/v1/crates/sqlparser
- **`tree-sitter-sequel` 0.3.11 was published 2025-10-01, which is the day this note was
  written**; its previous release was 0.3.10. It is the only grammar in the list with a single
  maintainer (`DerekStride`) outside the `tree-sitter` org. Its 887,739-byte tarball is the
  second-largest of the ten.
  https://crates.io/api/v1/crates/tree-sitter-sequel
- **Seven of the ten are governed under the `tree-sitter` GitHub org** (`rust`, `bash`, `json`,
  `html`, `javascript`, `typescript`, `php`, `python` — eight, in fact), with `dcreager` /
  `maxbrunsfeld` as the recurring crates.io owners. `toml-ng` lives under
  `tree-sitter-grammars` with a different owner (`ObserverOfTime`), and `sequel` is a personal
  project. So "one organisation, uniform cadence" is *mostly* true but not true for the two
  languages the ecosystem traditionally struggles with.
  https://crates.io/api/v1/crates/tree-sitter-rust/owners ·
  https://crates.io/api/v1/crates/tree-sitter-toml-ng/owners ·
  https://crates.io/api/v1/crates/tree-sitter-sequel/owners
- **No grammar in this list declares an MSRV** (`rust_version` is `null` for all ten); all ten
  are `edition = "2021"` and `license = "MIT"` except `tree-sitter-sqlite3` (`CC0-1.0`, not
  chosen). The `tree-sitter-language` requirement of `^0.1` is what binds them to the
  toolchain, and `tree-sitter-language 0.1.8` itself sits inside `tree-sitter 0.27.0`'s own
  `^0.1.8` requirement.
  https://index.crates.io/tr/ee/tree-sitter-rust · https://crates.io/api/v1/crates/tree-sitter/0.27.0/dependencies
- **Every one of the ten builds a C parser via a `cc` build script** (the ten list
  `cc ^1.1`/`^1.2` as a build dependency; `ratatui-markdown`'s existence proof of the same
  approach is note 01 §2.3). `gcc 16.2.1` is what compiled them here. So "the closure has no C
  build step" is *not* true of any of the ten — this is the same trade-off the repo already
  made explicitly for `tree-sitter-rust` in `Cargo.toml`.

## 3.2 Compatibility with `tree-sitter 0.27` — verified by compiling, not by reading

- **Measured: all ten grammars compile, link, and produce a working
  `HighlightConfiguration` against `tree-sitter 0.27.0` / `tree-sitter-highlight 0.27.0`, using
  the exact call shape the repo's `rust_config()` uses** (`HighlightConfiguration::new(lang,
  name, highlights, injections, locals)` with `LANGUAGE.into()` for the `LanguageFn`). Probe
  `tsprobe` ran twelve configurations — the ten languages plus `tsx` and `php_only` — and every
  one returned `ok`:

  ```
  rust       ok           first-config-ms=55.23
  bash       ok           first-config-ms=13.27
  json       ok           first-config-ms=0.05
  toml       ok           first-config-ms=0.27
  html       ok           first-config-ms=0.14
  javascript ok           first-config-ms=38.37
  typescript ok           first-config-ms=12.60
  tsx        ok           first-config-ms=11.82
  php        ok           first-config-ms=70.22
  php_only   ok           first-config-ms=64.12
  sql        ok           first-config-ms=38.61
  python     ok           first-config-ms=13.81
  ```

  The probe is a debug build; the numbers are therefore an upper bound on a release build,
  but the *relative* ordering and the fact that every query parses are the points.
  Measured 2026-10-01.
- **Measured: the resolved lockfile contains exactly one `tree-sitter`, one
  `tree-sitter-highlight` and one `tree-sitter-language`** for the whole ten-grammar set:
  `tree-sitter 0.27.0`, `tree-sitter-highlight 0.27.0`, `tree-sitter-language 0.1.8`,
  `tree-sitter-rust 0.24.2`, `tree-sitter-bash 0.25.1`, `tree-sitter-json 0.24.8`,
  `tree-sitter-toml-ng 0.7.0`, `tree-sitter-html 0.23.2`, `tree-sitter-javascript 0.25.0`,
  `tree-sitter-typescript 0.23.2`, `tree-sitter-php 0.24.2`, `tree-sitter-sequel 0.3.11`,
  `tree-sitter-python 0.25.0`, plus `cc 1.2.67`. 38 packages in total. No duplicate grammar
  runtime, no version skew.
  Measured 2026-10-01.
- **Why it unifies: `LanguageFn` has been ABI- and API-identical since 0.1.1.** Its definition
  is `#[repr(transparent)] pub struct LanguageFn(unsafe extern "C" fn() -> *const ())` with
  `pub const unsafe fn from_raw` and `pub const fn into_raw`; the diff from 0.1.1 to 0.1.8 is
  doc-comment punctuation, one added `#[must_use]`, and a `build.rs`. `tree-sitter 0.27.0`
  itself requires `tree-sitter-language ^0.1.8`, and `tree-sitter-language 0.1.8` is `edition
  2024` with `rust-version = "1.90"` (so 0.27's closure raises the toolchain floor to 1.90,
  which this repo's rustc 1.94.0 already satisfies).
  `tree-sitter-language-0.1.1.crate` and `tree-sitter-language-0.1.8.crate` (static.crates.io) ·
  https://crates.io/api/v1/crates/tree-sitter/0.27.0/dependencies ·
  https://crates.io/api/v1/crates/tree-sitter-language/0.1.8
- **The conversion is not in `tree-sitter-language`; it is `impl From<LanguageFn> for Language`
  in the `tree-sitter` crate**, which is why `*.LANGUAGE.into()` at the call site is what
  decouples the grammar's `^0.1` requirement from the runtime's minor version.
  https://docs.rs/tree-sitter/0.27.0/tree_sitter/struct.Language.html
- **`HighlightConfiguration::new` accepts a `Language` (by value) and has this exact
  signature:** `pub fn new(language: Language, name: impl Into<String>, highlights_query: &str,
  injection_query: &str, locals_query: &str) -> Result<Self, QueryError>`. Internally it
  concatenates the three query strings, calls `Query::new` (the compile step) for the combined
  query and again for the injections query, then scans pattern property settings for
  `injection.combined` and `local`. `configure(recognized_names)` then builds a
  `Vec<Option<Highlight>>` by walking the query's capture names and picking the longest
  recognized match on dot-separated components — which is exactly the "longest prefix wins"
  behaviour `Class::of` assumes.
  https://docs.rs/tree-sitter-highlight/0.27.0/src/tree_sitter_highlight/highlight.rs.html ·
  (local copy of `tree-sitter-highlight-0.27.0/src/highlight.rs`)
- **Initialisation cost, measured, is per-language query compilation:** 0.05 ms (json) to
  70 ms (php), 55 ms (rust), in a debug build. `Highlighter::new()` is cheap
  (`Parser::new()` + empty `Vec`s) and its docs say to reuse one per thread; the repo's current
  `try_highlight` constructs a fresh `Highlighter` per call and relies on `OnceLock` only for
  the configuration. Nothing in the ten crates changes that shape.
  Measured 2026-10-01; local copy of `tree-sitter-highlight-0.27.0/src/highlight.rs`
  (`Highlighter::new`, and the doc comment "For the best performance `Highlighter` values
  should be reused between syntax highlighting calls. A separate highlighter is needed for each
  thread that is performing highlighting.")
- **Laziness is possible at the granularity the repo wants:** the existing pattern is one
  `OnceLock<Option<HighlightConfiguration>>` per language (`rust_config()`), and because
  `HighlightConfiguration` is `Send + Sync` ("This struct is immutable and can be shared
  between threads"), ten such `OnceLock`s give per-language lazy compilation with no locking
  beyond `OnceLock`. `HighlightConfiguration`'s fields are `language`, `language_name`,
  `query`, the optional combined-injections query, offsets, and the capture-index vector — the
  resident cost of a configured language is the parsed `Query` plus a few vectors.
  `src/render/highlight.rs#L190-L206` ·
  local copy of `tree-sitter-highlight-0.27.0/src/highlight.rs`
- **Injection is optional per language and empty is legal.** Rust, HTML, JavaScript and PHP
  ship `INJECTIONS_QUERY`; bash, json, toml-ng, typescript, sequel and python do not. The
  constructor's doc says the injections query "can be empty if no injections are desired", and
  the probe passed `""` for those six without error. Passing a non-empty injections query for
  e.g. HTML would additionally require the `highlight` callback's injection handling to be
  wired (`Highlighter::highlight` takes `injection_callback: impl FnMut(&str) ->
  Option<&'a HighlightConfiguration>`), which the repo's `try_highlight` currently satisfies
  with `|_| None`.
  `src/render/highlight.rs#L219-L224` ·
  https://docs.rs/tree-sitter-highlight/0.27.0/src/tree_sitter_highlight/highlight.rs.html

## 3.3 Capture-name coverage against the repo's `CAPTURES`

The repo's `CAPTURES` list (`src/render/highlight.rs#L24-L52`) is matched by
`tree-sitter-highlight`'s dot-component rule, so `punctuation.special` maps to
`punctuation` and `type.qualifier` maps to `type`, while a capture with no recognized
component maps to `Class::Plain` and is simply not coloured. Measured against every `@name`
appearing in each grammar's own `queries/highlights.scm`:

| Grammar | distinct captures | captures the repo's list cannot match |
|---|---|---|
| rust | 21 | 0 |
| bash | 9 | 0 |
| json | 6 | 0 |
| toml-ng | 10 | 0 |
| html | 7 | `tag`, `tag.error` |
| javascript | 19 | 0 |
| typescript | 5 | 0 |
| php | 19 | `module`, `module.builtin`, `tag` |
| sequel | 21 | `conditional`, `field`, `float`, `parameter`, `spell`, `storageclass` |
| python | 17 | 0 |

- Six of ten grammars have perfect coverage of the repo's vocabulary; the leaks are HTML tags
  (the most visible one for this product), PHP namespaces/`?>` markup, and SQL's
  `conditional`/`field`/`float`/`parameter`/`storageclass`/`spell`. Note that
  `tree-sitter-sequel`'s query uses `conditional` and `keyword.operator` where Rust's would
  use `keyword`/`operator`; the former lands in `Plain`.
  Measured 2026-10-01; queries from the published tarballs (`tree-sitter-html-0.23.2.crate`,
  `tree-sitter-php-0.24.2.crate`, `tree-sitter-sequel-0.3.11.crate`, static.crates.io)
- **`tree-sitter-typescript`'s five captures are only the TypeScript-specific additions**; the
  full highlighting for the language comes from the TypeScript query referencing JavaScript
  node names, and the `tree-sitter-javascript` grammar's own query/JSX query are separate
  constants. Any TypeScript configuration therefore has to decide whether to concatenate
  `JSX_HIGHLIGHT_QUERY`/`highlights-jsx.scm` (it is not part of `HIGHLIGHTS_QUERY`).
  `tree-sitter-typescript-0.23.2.crate` · `tree-sitter-javascript-0.25.0.crate`

## 3.4 The compatibility risk table

Rows are the ten target languages; "locked TB version" is the requirement the crate declares
on the runtime (from the sparse-index row of its latest version), "compatible with 0.27" is
the measured result of §3.2, and the last column is what to watch.

| Language | crate | version | locks (runtime) | with `tree-sitter 0.27` | `HIGHLIGHTS_QUERY`? | risk |
|---|---|---|---|---|---|---|
| rust | `tree-sitter-rust` | 0.24.2 | `tree-sitter-language ^0.1` | **compatible** | yes | already a repo dependency; lowest risk |
| bash | `tree-sitter-bash` | 0.25.1 | `tree-sitter-language ^0.1` | **compatible** | yes, **named `HIGHLIGHT_QUERY`** | constant name differs from the other nine |
| json | `tree-sitter-json` | 0.24.8 | `tree-sitter-language ^0.1` | **compatible** | yes | last release 2024-11; grammar is frozen but stable |
| toml | `tree-sitter-toml-ng` | 0.7.0 | `tree-sitter-language ^0.1` | **compatible** | yes | avoid the name `tree-sitter-toml` (0.20.0 → `tree-sitter ^0.20`, dead since 2021); toml-ng lives outside the tree-sitter org and has only 2 releases |
| html | `tree-sitter-html` | 0.23.2 | `tree-sitter-language ^0.1` | **compatible** | yes | 2 of its 7 captures (`tag`, `tag.error`) are invisible to the repo's `CAPTURES` |
| javascript | `tree-sitter-javascript` | 0.25.0 | `tree-sitter-language ^0.1` | **compatible** | yes, **named `HIGHLIGHT_QUERY`** | also ships `JSX_HIGHLIGHT_QUERY`, which is *not* in the main query; constant name differs |
| typescript | `tree-sitter-typescript` | 0.23.2 | `tree-sitter-language ^0.1` | **compatible** | yes | two `Language`s (`LANGUAGE_TYPESCRIPT`, `LANGUAGE_TSX`); the crate's own query has only 5 captures, so it leans on the JavaScript grammar's node names; last release 2024-11 |
| php | `tree-sitter-php` | 0.24.2 | `tree-sitter-language ^0.1` | **compatible** | yes | two `Language`s (`LANGUAGE_PHP`, `LANGUAGE_PHP_ONLY`); 3 captures unmapped; 70 ms is the most expensive first configuration measured |
| sql | `tree-sitter-sequel` | 0.3.11 | `tree-sitter-language ^0.1` | **compatible** | yes | crate name ≠ language name; single maintainer; 6 unmapped captures; the same-named `tree-sitter-sql` is unusable; the alternatives are dialect-specific or not tree-sitter at all |
| python | `tree-sitter-python` | 0.25.0 | `tree-sitter-language ^0.1` | **compatible** | yes | none observed |

- **The "grammar locks an old tree-sitter" risk that the task anticipated does not
  materialise for these ten crates, but it is real for the crate names one would reach for by
  instinct:** `tree-sitter-toml` (0.20.0, 2022-01-05 → `tree-sitter ^0.20`), `tree-sitter-sql`
  (0.0.2, 2021-06-05 → `tree-sitter ^0.19.3`), `tree-sitter-markdown` (0.7.1, 2021-04-18 →
  `tree-sitter ^0.19`), `tree-sitter-sql-bigquery` (→ `tree-sitter >=0.19, <0.23`). The
  `tree-sitter-language ^0.1` shim is what separates the live grammars from the dead ones.
  https://index.crates.io/tr/ee/tree-sitter-toml ·
  https://crates.io/api/v1/crates/tree-sitter-toml ·
  https://index.crates.io/tr/ee/tree-sitter-sql ·
  https://index.crates.io/tr/ee/tree-sitter-markdown ·
  https://crates.io/api/v1/crates/tree-sitter-markdown ·
  https://index.crates.io/tr/ee/tree-sitter-sql-bigquery
- **The other real risk is C-toolchain, not version skew:** ten `cc` build scripts is ten C
  parser compilations at first build. The repo already accepts this for one grammar
  (`Cargo.toml`: "这个文法要编一个 C parser，所以首次构建比纯 Rust 依赖慢"), and
  `ratatui-markdown` makes the same trade for 39 of them; but the note 01 contrast with
  `pulldown-cmark` (5 pure-Rust crates in total, §2.1) is the honest one.
  `Cargo.toml` · https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/dependencies

---

# 4. This repo's existing constraints (context, not recommendation)

| Constraint | Source |
|---|---|
| `ratatui = "0.30"`, `crossterm = "0.29"` (with `event-stream`) | `Cargo.toml` |
| `tree-sitter = "0.27"`, `tree-sitter-highlight = "0.27"`, `tree-sitter-rust = "0.24"`; lockfile pins 0.27.0 / 0.27.0 / 0.24.2 / `tree-sitter-language 0.1.8` | `Cargo.toml`, `Cargo.lock` |
| Deliberate decision: **do not take syntect's Oniguruma path**; the tree-sitter Rust grammar's C parser is accepted | `Cargo.toml` comments; `docs/render.md`; `docs/highlight.md` |
| Renderer interface is `pub fn to_lines(text: &str) -> Vec<Line<'static>>` — a single forward pass over `text.split('\n')` with a fence state machine; tables are rendered row-by-row (`table_row()`/`table_line()` join cells with `" │ "`, `is_table_separator()` drops the delimiter row) with **no column-width computation and no whole-table buffering** | `src/render/markdown.rs#L20-L75`, `#L96-L112` |
| The current wrapper is `pane::wrap_text(text, width) -> Vec<Line<'static>>` (not `wrap_line`), and the column arithmetic lives in `render/width.rs` (`text_columns`, `char_columns`, `truncate_columns`, built on `ratatui::buffer::CellWidth`) | `src/render/pane.rs#L353`, `src/render/width.rs` |
| The existing highlighter has no production consumer; its config is a single `OnceLock<Option<HighlightConfiguration>>` for Rust only (`rust_config()`), and `CAPTURES` is the 27-name list | `docs/highlight.md`, `src/render/highlight.rs#L24-L52`, `#L190-L206` |

Facts above that bear on those constraints, restated without recommendation:

- `tui-markdown` with `default-features = false` leaves code fences, paragraphs and all other
  blocks unwrapped and hands back `Text<'_>`; the only width-aware path is tables, which are
  wrapped in-cell when `Options::table_width` is set and can exceed that budget when the
  per-column grapheme minimums do not fit (§1.1–§1.3).
- `pulldown-cmark` 0.13.4 is 5 pure-Rust crates with `default-features = false`, carries the
  fenced language tag as a `CowStr`, exposes table alignments per column, and requires a
  consumer-supplied alias table; all four GFM constructs need their own flag and `ENABLE_GFM`
  does not imply them (§2.1–§2.5).
- All ten target grammars compile against this repo's `tree-sitter 0.27` /
  `tree-sitter-highlight 0.27` and export a highlights query, with two constant-name
  exceptions, two crates that each export two `Language`s, and six unmapped capture names in
  the worst case (§3.1–§3.4).
