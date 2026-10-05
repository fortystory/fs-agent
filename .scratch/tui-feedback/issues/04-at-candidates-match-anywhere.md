# 04 — `@` 候选放宽：分段前缀、可从任意一段起

Type: implement
Status: ready-for-agent
Part of: ../spec.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §4。

## 目标

在输入框里打 `@cli`，菜单第一条就是 `src/cli.rs`。改的是候选的匹配规则：query 的每一段只要按
前缀落在候选路径的某一段上、段序保持即可，第一段可以从**任意**一段起。

## 现状（改前先复核）

- `src/render/file_index.rs` 的 `FileIndex::candidates(query, limit)`：`text.to_lowercase().starts_with(&needle)`
  —— 整条路径的前缀匹配，所以 `cli` 配不到 `src/cli.rs`。
- 索引里的路径按字典序排好（`scan` 的 `sort_by_file_path`），目录带尾随斜杠（`src/`）。
- 用它的地方：`src/render/tui.rs` 的 `token_menu`（`@` 与 `/` 共用浮层），`/` 走的是另一套
  候选来源（内建命令 + 技能名），**不受本票影响**。
- `contains(path)`（上色与 chip 的「能兑现」判据）与 `scan` 一个字不动。
- 断言：`tests/file_index.rs:108` 起；`tests/render_layout.rs:3863` 起（`@src/` 那一组帧断言）。

## 落点

`src/render/file_index.rs`、`tests/file_index.rs`、`tests/render_layout.rs`。

## 具体行为

1. 判据换成 `fn matches(path: &str, query: &str) -> Option<usize>`：query 与 path 都按 `/` 切段、
   丢掉空段；query 的第 i 段要按前缀（大小写不敏感）匹配候选的某一段 `c[j]`，`j` 严格递增；
   第一段的 `j` 任意。返回的是**匹配起点段号**（用来排序；不匹配是 `None`）。
2. 排序：起点段号升序，同分按索引里的路径序（`sort_by_key` 稳定排序即可，不引第二把比较器）。
   于是 `cli` 下 `src/cli.rs`（起点 1）排在 `vendor/x/cli-tool`（起点 2）前面。
3. `candidates` 仍取前 `limit` 条、仍丢掉含空白的路径、索引未就绪时仍返回空。
4. 逐字保留 `@src/` 的既有行为：query 以 `/` 结尾时会切出一个空段并被丢掉，于是 `src/` 与 `src`
   等价 —— 而 `src/` 那条候选本身（目录、带尾斜杠）与 `src/main.rs` 都仍在前几条里。

## 验收

- `tests/file_index.rs`：
  - `candidates("cli", n)` 的第一条是 `src/cli.rs`（在 `workspace()` 里补一个 `src/cli.rs`）；
  - `candidates("nested/deep", n)` 命中 `src/nested/deep.rs`（跨段、起点 1）；
  - `candidates("src", n)` 与 `candidates("src/", n)` 给出同一串（前缀行为不退化）；
  - 大小写不敏感（`CLI` 同样命中）；`limit` 仍生效。
- 帧层补一条：打 `@cli` 之后菜单里出现 `┆ @src/cli.rs`（照 `@src/` 那组断言的写法）。
- `cargo test` 全绿。
