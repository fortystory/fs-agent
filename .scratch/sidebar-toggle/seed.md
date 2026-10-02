# 左栏开关（种子与访谈存档）

**这不是 spec，也不是票。** 它是 2026-10-02 一次 `/wayfinder` 会话在 charting 之前那轮 grilling 的
十问十答，外加当时核过的一手事实。产物是 [`spec.md`](spec.md)。

## 原始意向

> 我要添加一个新操作，隐藏和展示左侧栏。

## 路线判定

按 wayfinder 技能自己的判据走的：**先钉 destination，再看有没有 fog**。

- **destination**：`fs-agent` 的 TUI 左栏获得一个用户可控的收起 / 叫回；
- 十问答完，分岔（形态、键名、寿命、与宽度档的关系、可发现性、边界）全部钉死，
  **没有「还说不清形状」的东西** ⇒ 不需要决策图。

所以落在用户清单的第三个分支：**直接实现，不建 map**。同仓库先例：
[`tui-chrome`](../tui-chrome/spec.md)（五条口头要求）、[`tui-input-pulse`](../tui-input-pulse/spec.md)、
[`suspend-gesture`](../suspend-gesture/spec.md)（九问存档 + spec + 两张票）。

## 访谈（十问，答案由维护者拍板）

| # | 问题 | 答案 |
| --- | --- | --- |
| Q1 | 要动的是哪一个「左侧栏」？ | (A) `fs-agent` 的 TUI 左栏 —— 不是本 GUI 的会话侧栏 |
| Q2 | 「一个新操作」的形态？ | (A) **一个键位** toggle；**但不要 `Ctrl-B`**（它在 tmux 与 herdr 里是 prefix） |
| Q3 | 隐藏状态的寿命？ | (A) **只活在当前进程**，与选中的页签同级；不落配置、不跨会话 |
| Q4 | 用户意愿与宽度阶梯谁优先？ | (A) **相乘**：可见 = 意愿 **且** 宽度 ≥ 80；窄档下「展示」静默无效 |
| Q5 | 这一轮走到哪里为止？ | (A) **直接实现路线**：spec + 实现票，**不建 wayfinder 图** |
| Q6 | 用哪个键？ | (A) **`Ctrl-O`** |
| Q7 | 可发现性走哪条路？ | (B) **提示行常驻加一条**（不是「什么都不加」，也不是「只在按下时回执」）—— 与我的推荐相反，按维护者的选择记 |
| Q8 | 隐藏期间 `todo` 标签首次出现，自动弹回吗？ | (A) **不弹回**，标签在后台就位，按回来才看见 |
| Q9 | busy 时这个键算不算数？ | (C) **busy 时可用**（纯视图操作），但**详情覆盖层立着时不管用**（覆盖层独占键盘） |
| Q10 | 提示行新条目放第几位？ | (A) **放最末**（第 6 位），文案 `ctrl-o 左栏`，不随状态换字 |

## 访谈里核过的一手事实

- **tracker 是本地 markdown**（`docs/agents/issue-tracker.md`）：`map.md` + `issues/NN-*.md`，
  **没有子 issue、也没有原生依赖边**，靠图正文的 `## 任务清单` + 每张票的 `Part of:` /
  `Blocked by:` 表达，由 `scripts/wayfinder-check.py` 对账。已有四张图全部走完。
- **这个需求没做过，但会推翻一条已冻结的决定**：`CONTEXT.md` 的**左栏**词条写着
  「**去留只由宽度决定**」（≥120 列 40 列宽 / 80–119 列 28 列宽 / 更窄整栏隐藏），
  [`tui-sidebar/spec.md`](../tui-sidebar/spec.md) §2 同句；`.scratch/` 里没有「手动隐藏左栏」的票。
- **`SidebarKind::Hidden` 已经存在**，但只由纯宽度函数触发：`src/render/layout.rs` 的
  `sidebar_tier(width)` 在 `w < 80` 时返 `None`，`sidebar_content` 据此给 `(Hidden, 0)`。
- **空闲控制键占用集合**：`c d a e u k w p n g j z`（`map_key` 的 CONTROL 分支）。候选里
  裸 `Ctrl-O` 空闲，且无强约定。
- **复用器占用**：维护者的 `~/.config/tmux/tmux.conf` 在 root 层（`-n`）绑的是
  `C-M-j` / `C-M-k` / `C-M-l` 与 `M-b` / `M-s` / `M-v` / `M-f` / `M-j`；裸 `Ctrl-B` 是 prefix。
  `herdr`（`herdrdev/herdr`，`/usr/bin/herdr`）的 prefix 同样是 `Ctrl-B`，它的默认键表不在
  `config.toml` 里（只有 `[ui]` / `[theme]`），所以「裸 `Ctrl-O` 不被 herdr 拦」是**推断**，
  未拿到权威清单 —— 真机上按一下即可证伪。
- **提示行是「从前往后填、超宽就停」**（`wording::hint_line`），所以**位置就是优先级**：
  `KEY_HINTS` 现在是 5 条 + 出口 + 状态词。既有阶梯数字（`src/render/wording.rs` 的
  docstring 与 `tests/wording.rs:403` 那一组断言）是**实测**来的。
- **测试接缝是 `draw_frame`**（`tests/render_layout.rs` 的抬头明写「一个状态进去、一块缓冲区
  出来」，从不涉及布局路上算出来的矩形），所以 `layout::plan` 的签名改动**不波及测试**；
  `plan` / `content_width` / `input_text_width` 在 `src/render/tui.rs` 里一共 5 个调用点。

## 一处如实记录：Q10 里报错的列宽账

我在 Q10 里报「现有 5 条 + 出口 = 78 列、加一条 = 90 列」是按**错的字宽**推的（把 `发送`
当成 2 列，中文是全角、每个字 2 列）。实测口径下：

- `ctrl-o 左栏` = **11 列**，加一个 ` · ` 是 14 列；
- 现有 5 条 + 出口本身就要**接近一整行**，所以第 6 条只在**最宽的档位**（大约 120 列那条线
  以上）才出现。

**位置结论不变**（放最末 ⇒ 最宽的档位才看得见它），但具体数字**以实测为准**：spec §4 与
[票 02](issues/02-hint-line-entry.md) 都要求按 `tests/wording.rs` 的口径重取一遍，不许沿用
这里的推算值。
