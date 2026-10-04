# 01 — 会话级的文件索引（`@` 候选与将来的文件页签共用）

Type: implement
Status: ready-for-agent
Blocked by: —

> 来源：[`../spec.md`](../spec.md) §1。这是这一组的**第一张**：它只做一个可复用的值，
> 不碰输入区、不碰菜单。后面三张都建在它上面。

## 目标

- 新增一个会话级的 **`FileIndex`**：`Idle | Loading | Ready(Arc<Vec<PathBuf>>)`，路径相对会话 cwd。
- 遍历在 `tokio::task::spawn_blocking` 里跑 `ignore::WalkBuilder::new(root)`，**与
  [`grep` 工具逐字同一条规则**（[`src/tools/grep.rs:164`](../../../src/tools/grep.rs)）：
  遵守 `.gitignore`、跳过隐藏文件、非 git 仓库也能走、`sort_by_file_path` 固定字典序。
- **预热**：进 TUI 后触发第一次遍历。
- **重扫**：每次提交一条消息之后在后台重扫一次。
- 结果经 `tokio::sync::mpsc` 回到循环，`tokio::select!` 加一支
  （[`src/render/tui.rs:405-413`](../../../src/render/tui.rs) 那段已有三个来源加两个按需定时器）。
- 一个查询接口：按前缀过滤、字典序、给定行数上限，供票 02 的菜单用。

## 现状（2026-10-04 核实，改前先复核）

- **没有索引**：`ignore` 只在 `grep` 工具里被使用（[`src/tools/grep.rs:28`](../../../src/tools/grep.rs)、
  [:164](../../../src/tools/grep.rs)）；输入区那条路上没有任何遍历。
- **运行时已经是异步的**：`tokio` 开着 `rt-multi-thread`/`sync`/`macros`
  （[`Cargo.toml:38`](../../../Cargo.toml)），主循环是 `tokio::select!`，键盘是 crossterm 的
  `EventStream`（[`src/render/tui.rs:350`](../../../src/render/tui.rs)）—— **这一票不需要动架构**，
  也不要顺手改渲染循环的形状。
- **没有模糊匹配库**：`Cargo.toml` 里没有 nucleo 一类，过滤只能是前缀匹配（见票 02 与 §3）。
- **非 git 仓库也要能走**：`grep` 工具就是这么用的，索引照抄它的规则。
- **`Tab::Files` 的语义未定**：注释写着「这个会话碰过的文件。还没做」
  （[`src/render/tui.rs:3413-3414`](../../../src/render/tui.rs)）。索引做成共享件，但**不为它做任何
  形状上的妥协**。

## 收尾

- 单元测试：隐藏文件不进、`.gitignore` 生效、固定字典序、相对 cwd、空目录不炸。
- 循环侧测试：预热触发一次；提交后重扫一次。
- `cargo test` 与 `python3 scripts/check-language.py` 通过。

## 不做什么

- **不接输入区**：这一票不画菜单、不改键位（票 02 才做）。
- **不动 `grep` 与 `repo_map`**：索引借用同一条规则，不改它们一条。
- **不给 `Tab::Files` 定型**：它到底列什么，等真做它的时候再定。
- 不加模糊匹配依赖；不跟随符号链接（与 `grep` 保持一致）。
