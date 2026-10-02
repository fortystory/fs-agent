# 左栏开关：`Ctrl-O` 收起与叫回

Status: 2 done + 1 ready-for-walkthrough（一次 grilling 的十问十答折叠；票 `01`、`02` 已落地，
票 `03` 的文档已写完 —— 剩下的是人在真终端里照手工清单 ㉖ 走一遍）

左栏现在**去留只由宽度决定**。这个 effort 给它加一层**用户意愿**：`Ctrl-O` 按下收起、再按叫回，
意愿与宽度档**相乘**。宽度是物理可行性，意愿是偏好 —— 两者不同层，谁也不能替谁说话。

来源：2026-10-02 一次 `/wayfinder` 会话在 charting 之前的那轮 grilling（十问十答）。**路线判定落在
「直接实现」**：钉完 destination 之后没有 fog —— 分岔一轮问完、没有说不清形状的决定 —— 按 wayfinder
自己的判据就不需要决策图。十问全文、候选与核过的事实存档在 [`seed.md`](seed.md)。

它**推翻**两处已落地的表述。原文都不改写，各加一条**带日期的补记**（落点见
[票 03](issues/03-docs-and-sweep.md)），因为那是当时的理由：

- [`../tui-sidebar/spec.md`](../tui-sidebar/spec.md) §2 的「**去留只由宽度决定**」—— 现在还有意愿这一层；
  宽度那一层本身（40 / 28 / 隐藏三档）不动；
- [`../../CONTEXT.md`](../../CONTEXT.md) 的**左栏**词条里同一句，以及「页签是点出来的、不给键位」被
  读成「左栏没有自己的键位」的那个引申 —— 那条约束是给**页签**的。

## Problem Statement

1. **左栏的去留，用户说不上话。** 120 列以上它恒占 41 列（40 列内容 + 1 列分隔线），想读一段长行、
   或者多要几列转录宽度，唯一的办法是把终端缩窄 —— 而那会同时触发别的降级（状态行、提示行、回合条
   跟着变）。为了一栏，付掉整个外壳的账，不划算。
2. **隐藏机制已经在那里，缺的是入口与状态。** `SidebarKind::Hidden` 是现成的：`sidebar_tier(width)`
   在 `w < 80` 时返 `None`，`sidebar_content` 据此给出 `(Hidden, 0)`，`plan` 里 `sidebar` / `divide`
   是 `None`、主列拿回整屏宽。缺的只是「用户想不想」这一位，以及一个按得下去的键。
3. **`CONTEXT.md` 把左栏钉成了「没有键位」。** 那条决定的原话是「页签是点出来的、不给键位（`Tab`
   归 `/` 菜单、`Shift+Tab` 归模式循环）」—— 它说的是**页签切换**不走键盘，不该顺带把「把整栏收起来」
   也钉死。本 spec 只动后者。

## Solution

- **一个键位**：`Ctrl-O`，一次进出。收起是立刻看得见的（左栏消失），不需要额外回执。
- **意愿 × 宽度**：`可见 = sidebar_wanted && sidebar_tier(width).is_some()`。宽屏下意愿说了算；
  `w < 80` 时左栏本来就放不下，按「叫回」**静默无效** —— 不报错、不弹文案、不进事件流。
- **意愿只活在进程内**：与选中的页签同级（`TuiState` 的一个字段），重开回到显示；不落配置、不跨会话。
- **收起之后主列把那一列也拿回来**：左栏那 41 列（40 内容 + 1 分隔）整列还给主列，几何照常重算；
  页签的命中矩形随帧消失（命中一律是「这一帧真的画了什么就记什么」），所以没有第二处要同步的状态。
- **提示行最末加一条** `ctrl-o 左栏`（十问的 Q7 选了「常驻加一条」，Q10 选了「放最末」）。

## Implementation Decisions

### §1 状态与寿命

`TuiState` 新增一个字段，名字取 `sidebar_wanted: bool`（默认 `true`）：

```rust
/// 用户想不想看见左栏（`Ctrl-O` 切换）。**只活在这一次进程里**，与选中的页签同级：
/// 重开回到显示，不落配置、不跨会话（spec §1）。
///
/// 它与宽度是两层：这个字段说偏好，[`layout::sidebar_tier`] 说可行性。两者相乘才是
/// 屏幕上真的有那一栏（spec §2）。
sidebar_wanted: bool,
```

- **不落配置**：`config.toml` 不动，`--continue` 重播也不恢复它（进程内字段，重播不碰）。
- **不进事件流**：它是纯视图状态，与「选中的页签」同类 —— 没有 `FrontEndEvent`，没有回执行，
  `sessions replay` 读不到它（也读不到页签选中页，两者一致）。

### §2 几何：意愿进入排版

**意愿必须进入排版函数**，否则「收起后主列不变宽」就错了。落点是给纯几何函数加一位入参，
一路贯穿（`src/render/layout.rs`）：

```rust
fn sidebar_tier(width: u16, wanted: bool) -> Option<u16>   // !wanted ⇒ None，宽度那一层原样
fn sidebar_content(width: u16, content_rows: u16, wanted: bool) -> (SidebarKind, u16)
fn main_width(width: u16, wanted: bool) -> u16
pub fn plan(area: Rect, draft_rows: u16, wanted: bool) -> Regions
pub fn content_width(area: Rect, wanted: bool) -> u16
pub fn input_text_width(area: Rect, wanted: bool) -> u16
```

- **`plan`**：`let tier = sidebar_tier(area.width, wanted);` —— 之后的一切（`sidebar` / `divide` /
  `main_x` / `main_width`）都从 `tier` 派生，所以**只有这一处要改**。`wanted == false` 时
  行为与今天 `w < 80` 的那一支**逐格相同**：`sidebar_kind = Hidden`、`tabs` / `sidebar_page` 是
  `None`、`main_x = area.x`、`main.width = area.width`。
- **`content_width` / `input_text_width`**：它们决定编辑器折行与问卷要几行，**必须跟着意愿走** ——
  否则收起左栏后转录变宽了、输入区还按旧宽度折行。
- **调用点一共 5 处，全在 `src/render/tui.rs`**：`plan` 两处（`draw_frame` 的主绘制、`detail_width`
  那一处）、`content_width` 一处、`input_text_width` 两处。它们从 `self.sidebar_wanted` /
  `state.sidebar_wanted` 取值。
- **测试不受波及**：`tests/render_layout.rs` 的接缝是 `draw_frame(state, area, …)`，测试从不直接
  调 `plan`（抬头明写这件事）。所以没有一处既有测试因为签名而返工，新用例通过构造
  `state.sidebar_wanted = false` 来断言。

**去掉入参的替代方案**（不采纳）：让 `TuiState` 把意愿折成一个「有效宽度」再传下去。那会让
「终端宽 120、左栏收起」与「终端宽 79」在排版函数里长得一样，而这两件事在**意图**上不同
（一个能叫回来、一个不能），把它们折叠进同一个数字里，下一个读代码的人就得反推。

### §3 键位与守卫

- **`Key::CtrlO`** 加进 `Key` 枚举；`map_key` 的 CONTROL 分支加 `'o' => Some(Key::CtrlO)`。
- **分派位置**：在 `TuiState::key` 里，排在 **`replay` 与 `detail_open` 两个提前返回之后、
  `expire_exit_gesture` 之前**：

  ```rust
  if key == Key::CtrlO {
      self.sidebar_wanted = !self.sidebar_wanted;
      return;
  }
  ```

  由此得到的边界正好是十问里定的那几条：

  | 视图 | `Ctrl-O` | 为什么 |
  | --- | --- | --- |
  | 空闲 / busy（回合跑着、`discuss` 跑着） | **生效** | 纯视图操作，与 `PgUp/PgDn` 同类；恰恰是跑着的时候想多看几行 |
  | 详情覆盖层立着 | **不生效** | 覆盖层是自成一体的「查看态」，独占键盘（既有分层） |
  | 历史重放中 | **不生效** | 重放期间键盘归 `replay_key`；重放是一次性的界面临时态 |
  | 问卷 / `/` 菜单 / 举手立着 | **生效** | 它们不吞键盘独占权，也没有「左栏此刻不能动」的理由 |

- **不做举手、不做双击、不进事件流**：一次按键一次切换，与 `Ctrl-Z` 的「单下生效」同一种手感。
- **`Ctrl-O` 不是「退出/取消」族**：不清举手（它排在 `expire_exit_gesture` 之前就返回，所以旧举手
  原样留着 —— 这是刻意的：按一下左栏键不该让半分钟前的退出举手作废）。

### §4 提示行

`wording::KEY_HINTS` 末尾追加一条：

```rust
const KEY_HINTS: [&str; 6] = [
    "enter 发送",
    "ctrl-j 换行",
    "esc 取消",
    "shift+tab 模式",
    "PgUp/PgDn 滚动",
    "ctrl-o 左栏",          // 新增：放最末，位置就是优先级（spec §4）
];
```

- **文案写「左栏」不写「侧栏」**：`CONTEXT.md` 的左栏词条把「侧栏」列在 `_Avoid_` 里。
- **不随状态换字**：收起时也照旧显示 `ctrl-o 左栏`（它是 toggle，一条就够），**不换**成
  「ctrl-o 显示左栏」。
- **放最末**：`hint_line` 从前往后填、超宽就停，所以最末那条**只在最宽的档位出现**。
  这是 Q10 的选择：`PgUp/PgDn` 比侧栏开关常用得多，不该为后者让位。
- **阶梯数字按实测重取**：`src/render/wording.rs` 的 docstring 与 `tests/wording.rs:403` 那一组
  断言记的是实测档位（40 / 60 / 80 / … 列各显示几条）。加第 6 条之后要重新量一遍再改注释，
  **不许沿用 [`seed.md`](seed.md) 里那处已更正过的推算值**。
- **`VIEWER_HINTS` 不加**：那是「前端没有在读行」时的状态行（`esc` / `PgUp`）。一条键位只在一个
  地方被提示，两份清单迟早漂移。`Ctrl-O` 在那种视图里照常可用，只是没有常驻提示 —— 提示行从来
  不是全量键位清单（`Ctrl-Z`、`Ctrl-P/N`、`Ctrl-A/E` 都不在里面）。
- **隐藏那一刻不给回执**：Q7 选的是「常驻加一条」，不是「按下时回执」。本仓库现在也只有
  「状态词 + 出口」两段，没有 toast 通道 —— 不为这一件事新开一条。

### §5 与既有决定的关系

- **`todo` 标签「出现过就常驻」不动**：收起期间模型产出 `todo`，标签在后台就位（`Tab::Todo`
  的可见性判定与 `sidebar_wanted` 无关），按回来就看得见。**不自动弹回**（Q8）：用户刚说了
  「我现在不要这栏」，一次工具调用不该覆盖它，界面也不该在用户没要求时自己变宽变窄。
- **宽度三档不动**：`SIDEBAR_WIDE_FROM` / `SIDEBAR_NARROW_FROM` 与 40 / 28 两个宽度照旧；
  意愿只在「有档」与「没档」之间加一个与门，不新增档位。
- **高度阶梯不动**：`sidebar_content` 里「先丢标记、再丢身份行、再从尾部丢字段」逐档照旧。
- **状态行、回合条、转录、输入区、提示行都不动**：它们唯二受影响的量是主列宽（`main_width`）
  与编辑器折行宽（`content_width` / `input_text_width`），两者都从意愿派生。
- **详情覆盖层与问题覆盖层的基准不动**（分别是整屏与主列，`tui-chrome` §4 的刻意差异）：
  左栏收起时主列更宽，问题覆盖层跟着变宽 —— 这是「主列居中」的应有之义。
- **plain / headless 不动**：`Key` 与 `TuiState` 都是 TUI 的东西。

## Testing Decisions

- **行为进 `cargo test`**，用 `tests/render_layout.rs` 的既有接缝（`draw_frame` + `TestBackend`），
  不需要 pty。
- **新增用例**（都通过构造 `state` 来摆意愿，不动 `plan` 签名）：
  1. **默认帧逐格不变**：`sidebar_wanted = true` 时 120×24 的帧与今天相等 —— 这一条是回归的锚，
     防止「加开关」顺手改了默认外观；
  2. **收起后主列拿回整屏**：`sidebar_wanted = false` 时，120×24 的转录文本宽 = `w − 2`（滚动条 +
     回合条），分隔列那一格不再是 `┆`（整列没有结构性框线）；
  3. **页签消失且不可点**：收起帧里 `调用量` / `todo` 标签不再出现，且原先点标签的那个坐标
     **不再切换页**（命中矩形随帧消失）；
  4. **意愿 × 宽度**：`40×10` 与 `60×24` 下按 `Ctrl-O` 后帧**逐格相等**（`w < 80` 时叫不回来，
     也没有任何反馈）；`80×24` 下收起 → 主列变宽，按回来 → 与默认帧相等；
  5. **`todo` 常驻不受影响**：有过非空列表的会话收起左栏，按回来后 `todo` 标签仍在、页内容不变；
     收起期间收到 `todo` 调用**不会**自己弹回（帧仍是收起的帧）；
  6. **守卫**：`replay` 进行中按 `Ctrl-O` 不改帧；详情覆盖层立着时按 `Ctrl-O` 不改帧且覆盖层不关；
     busy（一次运行在飞）时按 `Ctrl-O` **生效**；
  7. **编辑器跟着走**：收起后 `input_text_width` 变大 —— 一条长草稿在 120 列下收起左栏后折行数变少
     （或者直接断言 `layout::content_width(area, false) > layout::content_width(area, true)`）。
- **`tests/wording.rs`**：`the_status_line_keeps_the_way_out_and_gives_up_the_state_word_when_narrow`
  那一组档位断言按新的 6 条清单**重取实测值**；新增一条「`ctrl-o 左栏` 在最宽档位里，
  在中档/窄档里不出现」。**不要**为了让新条目早出现而改顺序 —— 顺序是 Q10 定的。
- **`scripts/tui-startup-check.py`**：锚点不受影响（`STATUS_ANCHOR` / `MARK_ROW` / 退出提示都不变），
  但它跑的是 260×30 的宽屏 —— 新条目会出现在提示行里，若脚本有「提示行必须逐字相等」的断言就
  要跟着改；否则一行不动。实现时**先跑一遍再说**，别预改。
- **手工清单**（`docs/tui-manual-checklist.md`）：新增一节，覆盖「120 列以上按 `Ctrl-O`：左栏整列
  消失、转录变宽、页签点不到了」「再按回来：页签与选中页还在」「80–119 列（28 档）收起/叫回」
  「< 80 列按 `Ctrl-O`：什么都不该发生」「跑一个回合时按下去也生效」。

## Out of Scope

- **键位表的其他增删**：这一轮只有 `Ctrl-O` 一个。
- **持久化**：不写 `config.toml`、不跨会话记住、不做 CLI 开关（`--no-sidebar` 之类）。
- **鼠标入口**：不点分隔线切换、不给「隐藏态」留任何可点区域。收起后叫回来只有键位一条路
  （提示行最末那条就是它的可发现性）。
- **左栏宽度可调 / 拖拽 / 记住宽度**：三档是定死的。
- **轨迹 / 文件两个页签的内容**、**`todo` 页的语义**：一个字不动。
- **`MIN_WIDTH` / `MIN_HEIGHT` 地板**与**左栏三档阈值的调整**。
- **plain / headless 渲染器**、事件 schema、模型可见文本（ADR 0001 / 0005 冻结）。
- **给「隐藏」加动画或过渡帧**。

## Further Notes

### 十问速查

| # | 决定 | 落在 |
| --- | --- | --- |
| Q1 | 目标是 TUI 左栏，不是别的界面 | 全篇 |
| Q2 | 一个键位 toggle，不用 `Ctrl-B` | §3 |
| Q3 | 意愿只活在进程内 | §1 |
| Q4 | 意愿与宽度相乘，窄档静默无效 | §2 |
| Q5 | 直接实现路线，不建 wayfinder 图 | [`seed.md`](seed.md) |
| Q6 | 键名 `Ctrl-O` | §3 |
| Q7 | 提示行常驻加一条 | §4 |
| Q8 | `todo` 首次出现不自动弹回 | §5 |
| Q9 | busy 可用；详情覆盖层立着时不可用 | §3 |
| Q10 | 提示条目放最末，文案 `ctrl-o 左栏` | §4 |

### 我在实现层面替你定的（写在这里以便否决）

- **`sidebar_wanted` 是布尔字段，不是三态**（`Auto` / `Shown` / `Hidden`）。今天没有「强制显示」
  这一档 —— `w < 80` 时按「叫回」无效，所以布尔就够。若以后想要「窄终端也强行画 28 列」，
  那是一次新的决定（并且要处理 40×10 地板上画不出来的那一档）。
- **`Ctrl-O` 排在 `replay` 与 `detail_open` 之后、`expire_exit_gesture` 之前**（§3）：所以它
  **不清举手**、也不被问卷 / 菜单拦下。若真机上出现「按一下左栏键、退出举手被清掉更对」的手感，
  把它挪到 `expire_exit_gesture` 之后即可，一行位置。
- **`VIEWER_HINTS` 不加这条**（§4）：一条键位只提示一处。
- **`todo` 常驻标签的判定不看 `sidebar_wanted`**（§5）：意愿只决定栏在不在，不决定标签存不存在。
- **提示行最末那条在窄档看不到**（§4）是接受的代价：想隐藏左栏的人，终端通常就是宽档
  （左栏 40 列那一档）；窄档左栏只有 28 列，收起的收益本来就小。
