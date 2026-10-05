# 提交信息

提交信息是写给下一次读 `git log` 的人（也是下一个 agent）的，所以它写**变更本身**：一行
`type(scope): 中文一句话`，然后正文 —— 分点写动机与理由，末尾两行收口。

## 标题

- `type` 取 `feat` / `fix` / `docs` / `refactor` / `test` / `chore` / `style`。
- `scope` 是模块名或 `.scratch/` 的 feature 目录名（`tui`、`trace`、`goals`、`permissions`…）。
- 直陈改了什么，不带句号，50 字符上下。

## 正文

分点写**动机与理由**：为什么这么改、推翻了哪条旧约定（指到具体 spec 的节，如 `trace-tab` §6），
标识符用反引号包起来。只写变更与它的后果，不复述 diff。

## 收尾两行

- `tracker：` 指到 `.scratch/` 下的 spec 与票（含状态）。
- `验证：` 列出跑过的命令与实测数字 —— `cargo test` 的通过数、`cargo clippy` /
  `cargo fmt --check` / 两道文档护栏 / 启动检查各自的结果。红的那条写清**是基线也红的，还是
  本次引入的**。

信息以 `验证：` 那一行收尾，后面不再添段落。

## 骨架

```text
feat(tui): 一句话说清改了什么

（来源与形态：谁提的、哪些问题定下了它；推翻了哪条旧约定，指到 spec 的节）

- **第一块**：做了什么、代价是什么。
- **第二块**：…

其余：顺带收掉的东西。

tracker：`.scratch/<feature>/spec.md`（票 01–03 done、04 ready-for-walkthrough）；
`CONTEXT.md` 与 `docs/render.md` 跟上。

验证：`cargo test` 1243 passed / 0 failed；`cargo clippy --all-targets` 无警告；
`cargo fmt --check` 干净；`scripts/check-doc-size.py` 与 `scripts/check-language.py` 绿。
```
