# 14 — 交给外部工具那一档

Type: implement
Status: done
Part of: ../map.md
Blocked by: 11

> 规格：[`../spec.md`](../spec.md) 的实现决定 §8 与用户故事 E。前置是
> [11 — 点开一份 diff](11-open-the-diff.md)（要有弹窗才能换呈现）。
> 决策的来路见 [grilling：外部工具那一档的形状](06-grilling-external-viewer.md)。

**What to build:** 可以把这份 diff 交给一个外部命令去看 —— `[ui] diff_viewer` 写程序名、
`diff_viewer_args` 给参数；那份 diff 从 **stdin** 进、我们**只给环境变量**；它吐的颜色被认下来、
画进**同一个弹窗**；命令不可用或超时**回退内置那一档**并给回执。

## 验收

- [x] `[ui] diff_viewer`（程序名）+ `diff_viewer_args`（argv 数组，按**整元素**替换、**绝不过
      shell**）接进配置；不写 = 内置那一档；非法值在启动时报错（与既有 `[ui]` 项同一形状）
- [x] 那份 diff 从 **stdin** 进那个程序；我们只给环境变量 `PAGER=cat` / `GIT_PAGER=cat` /
      `COLUMNS=<正文宽>`，**一条命令行参数都不注入**
- [x] 输出按**只认 SGR** 解：认重置 / 前景 / 背景 / 粗体 / 暗淡；**其他 CSI 序列整段丢掉**；
      认不出的颜色档退成 `PLAIN` 或按最近似的一档收下
- [x] 解出来的行仍走我们的正文管线（按正文宽折行、有界截断、可滚）
- [x] 命令不在 `PATH`、非零退出、超时（2 秒）→ **回退内置那一档** + 提示行一句回执；
      **不把 stderr 画进正文**
- [x] 弹窗标题右端标出**工具名**，内置那一档不标
- [x] SGR 解析有纯函数单测（含「认不出 → PLAIN」与「非 SGR 的 CSI 丢掉」两条）
- [x] 起真进程那一档用一个**假的工具脚本**测（prior art 是 MCP 那个假 server）
- [x] 落一条 **ADR**：这一档的颜色不经过语义色板（与「一块外来屏幕不受视觉纪律约束」同族，
      但那是外来**屏幕**、这是外来**输出**）

## 评论

落地：`[ui] diff_viewer` + `diff_viewer_args`（空串 = 内置；写了参数没写程序名是启动错误）、diff 走
**stdin**、只给环境变量（`PAGER` / `GIT_PAGER` / `COLUMNS`）、只认 SGR 子集的解析（认不出的颜色保持现状，
其它 CSI 丢掉）、失败 / 超时回退内置 + 提示行回执、标题右端标工具名；[ADR 0018](../../../docs/adr/0018-external-diff-viewer-colours-sit-outside-the-palette.md)。
这一档的真进程测试用一个假工具脚本（这台机器上没有任何真外部 diff 工具）。
