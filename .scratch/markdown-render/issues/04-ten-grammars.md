# 高亮文法扩到 10 种语言

Type: implement
Status: done

> 规格：`.scratch/markdown-render/spec.md` §4。
> Blocked by: 03（它把高亮接回了代码块）。
> 一手事实（crate 名、版本、常量名、兼容性实测）在 [research/02](../../markdown-render/research/02-parser-interface-and-grammars.md)。

## 目标

`render::highlight` 从只认 Rust 变成认 10 种语言，**全部硬依赖**，没有任何 feature 门控或 `#[cfg]` 分支。

## 落点

`Cargo.toml`、`src/render/highlight.rs`、`tests/render_markdown.rs`（或新建 `tests/render_highlight_langs.rs`）。

## 具体行为

1. **10 种语言与它们的 crate**（全部已实测与 `tree-sitter 0.27` / `tree-sitter-highlight 0.27` 兼容，lockfile 只有一份 0.27.0 + 一份 `tree-sitter-language 0.1.8`）：

   | 语言 | crate | 常量 | 备注 |
   | --- | --- | --- | --- |
   | rust | `tree-sitter-rust`（已是依赖） | `HIGHLIGHTS_QUERY` | |
   | bash | `tree-sitter-bash` | **`HIGHLIGHT_QUERY`（单数）** | |
   | json | `tree-sitter-json` | `HIGHLIGHTS_QUERY` | |
   | toml | **`tree-sitter-toml-ng`** | `HIGHLIGHTS_QUERY` | 原版 `tree-sitter-toml` 停在 2022 年、锁 `^0.20`，不能用 |
   | html | `tree-sitter-html` | `HIGHLIGHTS_QUERY` | |
   | javascript | `tree-sitter-javascript` | **`HIGHLIGHT_QUERY`（单数）** | |
   | typescript | `tree-sitter-typescript` | `HIGHLIGHTS_QUERY` | 用 `LANGUAGE_TYPESCRIPT`（不是 `LANGUAGE_TSX`） |
   | php | `tree-sitter-php` | `HIGHLIGHTS_QUERY` | 用 `LANGUAGE_PHP`（不是 `LANGUAGE_PHP_ONLY`） |
   | sql | **`tree-sitter-sequel`** | `HIGHLIGHTS_QUERY` | `tree-sitter-sql` 停在 2021 年、锁 `^0.19.3`；活的名字与语言对不上 |
   | python | `tree-sitter-python` | `HIGHLIGHTS_QUERY` | |

2. **别名映射归我们**：`rs` → rust、`py` → python、`js` → javascript、`ts` → typescript、`sh` / `shell` → bash、`yml` / `yaml` → ？（yaml 不在名单里，**不映射**）。映射表写在一处，别散在匹配分支里。
3. **每种语言一个 per-language `OnceLock<Option<HighlightConfiguration>>`**，**延迟到第一次用到该语言才编译 query**（实测首次编译 json 0.05ms ～ php 70ms；10 个全在启动时算会白付几百毫秒）。现在的 `rust_config()` 是单个 `OnceLock`，把它推广成一张按语言的表。
4. **`CAPTURES` 补齐**（漏 = 不上色，不是错，但补齐更完整）：html 的 `tag` / `tag.error`、php 的 `module` / `module.builtin` / `tag`、sequel 的 `conditional` / `field` / `float` / `parameter` / `storageclass`。补完要同步更新 `Class::of` 的前缀映射。
5. **不做 feature 门控**：10 个都是普通依赖，`highlight.rs` 里不出现 `#[cfg(feature = ...)]`。
6. **不拼接 typescript 的 `JSX_HIGHLIGHT_QUERY`**——`.tsx` 里的标签不上色，这是知情的取舍（spec §7）。

## 测试

- **10 种语言各一个最小样例**，断言至少拿到过一个非 `Class::Plain` 的 span——这是 10 个 grammar 的**接线测试**（接错了、常量名写错了、`LANGUAGE` 选错了都会在这里红）。
  - 比如：bash `echo $HOME`、json `{"a": 1}`、toml `a = 1`、html `<p>x</p>`、javascript `const a = 1`、typescript `const a: number = 1`、php `<?php echo 1;`、sql `SELECT 1 FROM t`、python `def f(): pass`、rust `fn f() {}`。
- **别名**：` ```rs ` 与 ` ```rust ` 得到同样的高亮；` ```py ` 同理。
- **未映射的语言**（`brainfuck`）不进高亮路径，不 panic、不返回错。
- 一条**只有 rust 是既有行为**的回归断言继续过。

## 验收

- [ ] `cargo test` 全绿。
- [ ] `cargo clippy --all-targets` 无新增告警。
- [ ] `Cargo.lock` 里 `tree-sitter` 与 `tree-sitter-highlight` **各只有一份**（0.27）。
- [ ] 冷构建时间记一笔实测值（10 个 C parser），写进 Comments——它是一次性的，但值得知道。
- [ ] 真机：10 种语言各贴一段看一眼。

## 评论

- 2026-10-01 落地：十种文法全部硬依赖（`Cargo.toml` 里新增九条 + 既有的 `tree-sitter-rust`），每种一个 `OnceLock`，第一次用到才编 query；别名表 `canonical`（`rs` / `py` / `js` / `ts` / `sh` / `shell`，`yaml` 不映射）；`CAPTURES` 补齐 html 的 `tag` / `tag.error`、php 的 `module` / `module.builtin` / `tag`、sequel 的 `conditional` / `field` / `float` / `parameter` / `storageclass`，`Class::of` 同步。sequel 的 `spell` 有意留在 `Plain`（那本来就是「不知道是什么」）。
- 两个常量名按实测：`bash` 与 `javascript` 是**单数** `HIGHLIGHT_QUERY`；crate 名按实测：toml 用 `tree-sitter-toml-ng`、sql 用 `tree-sitter-sequel`。
- **冷构建实测（2026-10-01，本机 debug）**：`cargo build --tests --target-dir target/cold` 整棵依赖树从零 **25 秒**；只把这十个文法清掉再编 **8 秒**（十个 C parser 各一次）。这是一次性的代价，增量构建不付。
- 接线测试：`tests/render_markdown.rs::all_ten_grammars_are_wired_up`（十种各一个最小样例，要求至少一片非 `Plain`），别名一条，未映射语言一条。
- `Cargo.lock` 里 `tree-sitter` / `tree-sitter-highlight` 各只有一份 0.27.0（`cargo tree` 复核）。
