# task：虚线清单 —— CLI 不走但存在的路径

Type: task
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

冻结项 7 说「可注入但 CLI 不注入」与「尚未实现」的路径画虚线。**哪些路径属于这两类，是一件要
取证的事、不是一个要 decide 的问题** —— 所以它是 task：做完它，五张图才知道哪条边该虚。

[`research/01-runtime-lifecycle-facts.md`](../research/01-runtime-lifecycle-facts.md) 已经点出三处，
但不全：

1. **hook**：交互式路径上永远是 `None`（`src/cli.rs:396`），只有库调用方 / 测试能挂 —— 可是
   `hook.pre` / `hook.post` 在 turn 里是实打实的一步。
2. **headless 渲染器**：`Renderer::headless` 存在并被导出（`src/render/mod.rs:171-173`），
   `cli.rs` 从不构造它。
3. **MCP**：`mcp-support` 的实现票还没落地（9 张 `ready-for-agent`）。

要考出来的：

- 把 `src/cli.rs` 的交互式组装路径与 `src/lib.rs` 的 `assemble` / `assemble_discussion` 逐段比对，
  列出**每一个「类型里存在、端口存在，但 CLI 那条路上传的是 `None` / 默认值 / 根本不构造」的
  能力**。
- 对每一处记下：事实（`文件:行号`）、属于哪一类（可注入 / 未实现 / 只有库调用方）、以及「如果它
  被插上，图上哪条边会变成实线」。
- 顺带核 `research/01` 的「存疑」那 5 条里哪些属于这个清单。

## 交付

清单落到 `research/02-dashed-inventory.md`，票底 `## 作答` 写一张分三类的表 + 每一类的画法建议
（虚线 / 旁注 / 不画）。

## 不做什么

- 不改代码、不给 CLI 加 hook 注入点。
- 不判断「该不该给 CLI 加 hook 注入」—— 那是另一个 effort。

## 作答

（2026-10-03）交付物：[`research/02-dashed-inventory.md`](../research/02-dashed-inventory.md)，103 行 ——
逐项比对 + 判据 + 存疑核对 + 结论。

**一句话结论：该画虚线的只有 2 条。**

1. **`hook`**（图 4 现有的 `pre -.-> hooks` 就是它，`pre` / `post` 共用一个 `None`、画一条即够）：
   trait 与两个挂载点都完整（`src/hooks.rs:169-183`、`src/agent.rs:896-1005`、`:1229-1231`、
   `:2118-2176`），但**三个生产组装点全传 `None`**（`src/cli.rs:396` / `:715` / `:2232`），
   `src/` 内零 `impl Hook`（只有 `tests/support/hook.rs:89`），`src/config.rs` 里 hook 零命中
   —— **没有配置面**。
2. **MCP**：`src/` 全仓零命中，但 spec 已定并拆出 9 张 `ready-for-agent` 实现票
   （[`.scratch/mcp-support/spec.md`](../mcp-support/spec.md)）—— 也就是说它**有一个已写下来的
   接缝**，不是一句愿望。图上**先用旁注**（不占节点）。

**生产组装点只有三个**（这是判据的基准）：交互式 `src/cli.rs:380`、`discuss` `:700`、
`probe` `:2214`。其余构造点全在测试（`#[cfg(test)]` 自 `:3312` 起）。

**如实排除的一条**：headless 渲染器**不是虚线** —— 它在 `probe` 里真被构造（`src/cli.rs:2238`）。
`research/01` 的存疑 1 就此关闭。

**另外 13 项全部实线 / 旁注 / 不画**（headless、asker、questions、沙箱、`goals_dir`、Provider、
`PathLocks`、渲染 sink、`SessionFacts`、动态工具、`web_service`、执行者的 `questions: None`、
`discuss` 没有 `--mode`），判据写成四种**不该画虚线**的情形：① 默认值 + 运行期补齐（沙箱的
`availability` 从 `Untested` 到 `src/lib.rs:315-322` 探一次）；② 前端差异（probe 没有 `asker` /
`questions`、非交互式没有目标循环）；③ 配置可选（`[web] enabled`、`custom__*` 声明、`--tui`）；
④ 测试替身（`FakeProvider` / `ScriptedHook` / `AlwaysAllow` 只在 `tests/`）。

**判据已写成可直接抄进 `docs/lifecycle.md` §1 的一节**（报告 §2）：**实线** = 这条路径在至少一个
生产组装点上会真实走到（含配置开关打开的，以及一条 `None` 的**降级分支** —— probe 的 `asker: None`
让 `Ask` 降级为 `Deny` 也算走到了）；**虚线**只给上面那两类；另外写死「**只有 `seed.md` 的 roadmap
不进图**」（否则生命周期图会变成路线图）。

**图已按它的三条建议修正**（[`prototype/01-drafts.md`](../prototype/01-drafts.md) v3）：

1. 图 3 的 `hook.pre` / `hook.post` 两行**降级成一条 `Note`**（CLI 下恒不发生），固定顺序仍在 Note
   里写全。
2. 图 4 的 `pre -.-> hooks` **保留**。
3. 图 2 的 `tools` 节点加旁注「MCP 元工具尚未实现」。
