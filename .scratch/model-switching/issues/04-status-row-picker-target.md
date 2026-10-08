# 04 — 状态行：多一段档位，两格可点

Type: implement
Status: done
Part of: ../spec.md
Blocked by: 03

状态行从四段变五段，模型格与档位格记下可点的矩形，点一下就上行一个「打开选择器」的手势。

规格见 [`spec.md` §7、§11、§12](../spec.md)。

## 要做什么

### `wording::status_row` 多一段

`src/render/wording.rs:1939`。签名多一个 `effort: &str` 参数（渲染器算好了传进来，措辞层不
知道档位是什么）。段落变成：

```
模型 kimi-k3 ┆ 档位 high ┆ 询问 ┆ 上下文 42% ┆ 🌑 就绪
```

降级顺序重排（spec §11）：**模型与档位一起丢或一起留**，然后丢模式，最后剩「上下文 + 状态词」。

```rust
let segments: [Vec<StatusPart>; 5] = [
    vec![label("模型 "), value(model), separator-ish …, label("档位 "), value(effort)],
    …
];
```

注意模型与档位之间**不**放 `┆` —— 它们是同一格的两半（同生共死，所以一个分隔线会把它们说成
两件事）。这一格画成 `模型 kimi-k3 · high`：`·` 是段内分隔，与 `tui-visual-language` 的字形语法
同族（框架 `┄`/`┆`、内容 `─`/`│`，这里要一个更轻的）。**字形常量加进 `wording.rs` 的符号表**，
不内联字面量。

`档位 固定` 是表为空时那一格的值（spec §8）。

### `Regions` 加两格

`src/render/tui.rs` 的 `Regions` 与 `HitAction`（`tui.rs:1340`）：

```rust
enum HitAction { …, SwitchModel, SwitchEffort }
```

由画状态行的那一处（`draw_status`）每帧把两格的 `Rect` 记进 `Regions` —— 与页签、回合条同一条
纪律（「记住读的人真看到了什么」，`Regions::clear` 每帧调一次）。`click_at` 的
`regions.action_at` 那一支已经会读到它，**不**要把它排进 `take_input_keyboard` 之前 ——
状态行在输入区上面，`take_input_keyboard` 会先把它吃掉。检查一下顺序，必要时在
`click_at` 里把这两格提到 `take_input_keyboard` 之前判。

### 上行

`FrontEndEvent` 加一个变体：

```rust
/// 点状态行或按 ctrl-t：打开模型/档位选择器。
OpenPicker(PickerKind),   // enum PickerKind { Model, Effort }
```

`FrontEndEvent` 现在是 `Copy` 的无载荷枚举；`PickerKind` 也是 `Copy` 的，所以加它**不破坏**
`Copy`。三个 `events.recv()` 分派点（`cli.rs` 的 1079 / 1115 / 1232 与 2343）各加一条转发给票 03
那个函数。

### `SessionUpdate` 落地

`TuiState::request`（`tui.rs:3730`）加一格：并进 `facts` 的 `model` / `context_window` /
`speaker_order`，档位存进 `TuiState` 自己（它是渲染器状态，与 `mode` 同一格），然后
`mark_dirty`。

## 测试

- `tests/wording.rs`：状态行的纯文本按 §11 那行；三条宽度阶梯（丢掉模型+档位 → 丢掉模式 →
  只剩上下文与状态词），以及「连最后一档都放不下时截断而不整行丢掉」。
- `tests/render_tui.rs`：
  - `SessionUpdate` 之后状态行显示的是新模型与新档位，`上下文 n%` 的分母也换了。
  - 点模型格 → 上行 `OpenPicker(Model)`；点档位格 → `OpenPicker(Effort)`；点状态行别的地方
    （分隔符、状态词）什么都不上行。
  - 点状态行**不**把键盘交给输入区（`take_input_keyboard` 那一格不包含状态行 —— 看 `files_page_contains`
    那种区域判定怎么写的，照它写一条 `status_contains`）。
  - `speaker_order` 换名之后新名字拿到一个没人认领的颜色槽位，而已经画过的行不改名。
- `tests/render_layout.rs`：状态行区域的宽高（它是 `layout::Regions::status` 那一行，不动）。

## 评论

2026-10-08 实现完毕，`Status: done`。

- 状态行五段；段内的 `·` 直接用符号表里已有的 `SEP`（「一行里各条目的分隔」），没有内联字面量。
- 两格的矩形由 `draw_status` 每帧顺着 `status_row` 的段落结构累列号算出来
  （`status_cells`），所以**降级之后点它就什么都不会上行** —— 上一帧的位置不会被沿用。
- `take_input_keyboard` **不含**状态行（它只看 `layout::plan(...).input`），所以票上担心的
  「点击被它先吃掉」不成立，`regions.action_at` 那一支就够；`tests/render_layout.rs` 有一条
  `clicking_a_status_cell_does_not_take_the_keyboard_away_from_the_input` 把这个事实钉住。
- `SessionUpdate` 并进 `facts` 的三个字段，档位存进 `TuiState` 自己（与 `mode` 同一格：渲染器状态）。
  `SpeakerColors::adopt_roster` **不替换**组装时的名册（它是槽位按位置分配的依据，换掉会让同一个
  名字换一次颜色），只把新名字登记到「没人认领的槽位」那条路上。
- `SessionFacts` 多一个 `switchable: bool`（11 处字面量跟着改）；讨论 CLI 那条路径传 `false`。

**一处既有断言的修正**：`tests/render_layout.rs` 的尺寸矩阵里 80 列两档，状态行**放不下**
「模型 + 档位」了（100 列才回来）—— 这是状态行多一段的代价，矩阵按新语义更新并在原地写了理由。

### 代码审查之后的修正（2026-10-08）

- `SpeakerColors::adopt_roster` 的两条契约（槽位按位置分配、组装时的名册不被替换）补上测试：
  `a_new_speaker_name_takes_an_unclaimed_colour_and_the_old_one_keeps_its` 在帧里读发言者名字
  那一格的**前景色**，断言换名之后新名字与旧名字**不同色**，而已画过的行不改名。
- `status_row` 的注释补上一句「视觉上五段、数组里四段」——模型与档位之间没有段分隔符，所以
  它们是同一段的两半，而降级时两半一起丢。逐面文档与词表说的是**视觉**读法，代码是四段数组。
