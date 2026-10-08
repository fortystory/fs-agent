# 会话中途换模型与思考强度

Status: 5 done + 1 ready-for-walkthrough（2026-10-08 维护者列出四个入口与快捷键后直接折成 spec 并拆六张票；四个待定的选择当场问定：只在空闲时切、按厂商给全档、列配置里登记的全部模型、讨论会话不切换；`Ctrl-M` 在终端里就是回车，所以快捷键取 `ctrl-t`；档位含一档「默认」。代码与测试全部落地，票 05 剩下的只有真终端里的手感走查 —— `docs/tui-manual-checklist.md` ㊱）

MiniMax 接进来之后，切模型仍然只能靠重启：`heng --model MiniMax-M3.1-Flash-Preview`。
一次会话里人想换个模型、或想把思考档位从 `high` 调到 `max`，都得退出、重开、把上下文重新
喂一遍 —— 而重启丢掉正在进行的对话，这是**换模型**这件事在今天唯一诚实的做法。

这份 spec 给的是四个入口（状态行点击、`/model`、`/effort`、快捷键）与它们背后那条
**会话中途换 provider** 的路。

## 问题陈述

1. **换模型要重启整个进程。** 模型在组装期就被焊进三样东西：provider（`OpenAiProvider` 里存着
   `model` 与 `caps`）、[`SessionConfig`]（`model`、`params`、以及从它派生的预算与价目表）与渲染器
   注入的 [`SessionFacts`]（状态行显示用的 `model` 与 `context_window`）。三样都是只读的，所以
   会话中途没有任何路径能改它。
2. **思考档位在会话开始时定死。** `GenerationParams::reasoning_effort` 的文档注释写着「整个会话
   钉住；中途从不切换」，理由是换档会扔掉前缀缓存 —— 这条理由成立（Kimi 官方也是这么说的），
   但它今天表现为**完全不能换**，而不是「换要付代价」。代价是真的，不能换也是真的，于是这里
   收的是后者。
3. **状态行那一格明写着「都不可点」。** [`CONTEXT.md`](../../CONTEXT.md) 的**状态行**词条与
   `tui-visual-language` §17 都把状态行定位成只读的一行。四个入口里有两个（点状态行、快捷键）
   要落在它身上，所以那条「不可点」要收回。
4. **换模型不是纯显示问题。** 换掉模型 id 的同时，能力表里的十几项事实全跟着变：上下文窗口
   （决定状态行那个 `n%` 的分母与 `usable_input`）、`max_tokens_field`、`reasoning_split`、
   `min_cacheable_tokens`、`supports_reasoning_effort`。而厂商特有行为（`supports_temperature`
   在 K3 上是 false）意味着**换模型不只是换一个字符串** —— 请求体的形状都变了。

## 方案

```text
入口        点状态行两格 ┐  ctrl-t  ┐  /model  ┐  /effort  ┐
                          └────────┴─────────┴──────────┘
                                   │
                          FrontEndEvent::OpenPicker
                                   │
                          循环（cli.rs）：判忙闲 → 拒绝或放行
                                   │
                    重建 OpenAiProvider + SessionConfig + SessionFacts
                                   │
                    ConsoleRequest::SessionUpdate 推回前端（状态行改写）
                                   │
                          下一回合用新模型、新能力表
```

**候选清单由循环出、前端画**：渲染器从不伸手去够配置（`SessionFacts` 文档注释的原话），而
「哪些模型可用」这件事要读 `Config`、`caps_for` 与环境变量 —— 所以它住在循环侧，前端只画
拿到的清单。

## 实现决定

### §1 档位集合从 `bool` 升级成一张表

`ModelCaps::supports_reasoning_effort: bool` 今天只够回答「有没有这个旋钮」。要做选择器，得回答
**这个模型有哪几档** —— 三家厂商不一样（§4）。所以换成：

```rust
pub reasoning_efforts: &'static [ReasoningEffort],   // 空 = 没有旋钮
```

`ReasoningEffort` 加两档，凑齐五档：

```rust
pub enum ReasoningEffort { Low, Medium, High, Xhigh, Max }
```

`build_body` 里那个 `if caps.supports_reasoning_effort` 改成查表：`effort` 在表里就发，不在就
照今天那样告警并丢掉（**绝不发一个模型不认的值**）。空表与今天 `false` 的行为一致。

`GenerationParams::reasoning_effort` 保持 `Option<ReasoningEffort>`：`None` 就是 §5 的「默认」。

### §2 `Session` 加一个换挡口

`Session::config` 是私有的。加一个显式的方法，而不是把字段改成 pub：

```rust
pub fn retarget(&mut self, config: SessionConfig)   // 会话中途换模型与档位
```

它换的是 `config` 这一个字段，其余（流、工具表、策略、读集）原样。**只有循环调它** —— 这是
`/model`、`/effort`、点状态行、快捷键四个入口的共同落点，渲染器碰不到。

`Harness` 侧要跟着换 `self.provider`（`Arc<dyn Provider>` 是 `provider.into()` 得来的，换成
`Arc::new(...)`）与 `self.session.config`。所以 `Harness::switch_model(&mut self, provider,
config)` 一个动作做完两件事 —— 它们必须同时变，否则下一次请求会拿新 id 去问旧 caps。

### §3 上下文窗口变了，`SessionFacts` 得跟着变

`SessionFacts.context_window` 是状态行那个 `n%` 的分母，也是 `usable_input(&caps)` 的结果。
换模型后它可能变（1M → 256K，或反过来）。它是注入的只读值，所以循环换完之后要**推一条新的
事实回去**：

```rust
ConsoleRequest::SessionUpdate {
    model: String,
    effort: Option<ReasoningEffort>,
    context_window: u64,
    speakers: Vec<String>,     // 见 §4
}
```

这是 `RunState` / `Muted` 那个形状：**循环推、前端收，发完就完**。前端把它并进 `facts` 的那
三个字段，然后请一帧。

### §4 换 provider 会换发言者的名字

`SpeakerId::Debater(profile.name)` —— 发言者名是 **provider profile 的名字**，不是模型 id。切
到另一个厂商的 profile（`kimi` → `minimax-cn`），发言者名就变了，而 `SpeakerColors` 已经把颜色
按名字分配过了。

处理：**跟着变**。新名字走 `SpeakerColors` 已有的「没人认领的槽位」那条路（`tui.rs` 里
`SpeakerColors::extra` 那套，`/discuss` 中途加入的讨论者就走它），于是同一场会话里先后两个名字
各有各的颜色 —— 这正是那一段注释写明的意图。`SessionFacts::speaker_order` 因此也要跟着改，
它就是名册。推 `SessionUpdate` 时带上新名册。

代价说在明面上：转录里已经画过的行不会改名（事件流只追加），所以一场会话里能看到两个名字。

### §5 「默认」是一档，不是缺省值

厂商自己的默认档各不相同（K3 是 `max`、DeepSeek 是 `high`），把「厂商默认」展开成某个具体值会
让人以为是自己选的。所以它在选择器里**单列一档**，写 `默认`，选中即 `reasoning_effort: None`
（不发这个字段）。状态行显示的就是那两个字。

### §6 忙闲：运行中拒绝，给回执

模型与档位要换 provider 与请求参数，与模式手势（纯渲染器状态）不同形。所以：

- **运行中**（`RunState.running` 为真）：拒绝，提示行给一句「这一回合跑完再切」。
- **空闲**：立刻生效。

判据在循环侧（`interactive_loop` 的 `tokio::select!` 那个 select 里已经有 `events.recv()` 分支，
在那里加一条），不在渲染器 —— 渲染器不知道什么在跑。

### §7 三个入口

**`/model` 与 `/effort`**：`wording::BUILT_IN_COMMANDS` 加两条，进 `Submission` 的解析。
`/model <id>` 直接切（模型 id 可能含点号，`MiniMax-M3.1-Flash-Preview` —— 记号解析按整段空白切，
不按点号切，所以没问题）；`/effort <low|medium|high|xhigh|max>` 直接切。**不带参数时**不开弹窗，
而是说一句用法 —— 与 `/goal-new` 缺参数是同一个形状（`goal_new_without_a_name_and_a_source_is_a_usage_error`）。
选���器是 TUI 独占的交互，plain 前端拿不到键盘。

**点击状态行**：`Regions` 加两个 `HitAction`（`SwitchModel` / `SwitchEffort`），由画状态行的那一处
每帧记下那两格的矩形 —— 与页签、回合条同一条纪律（「记住读的人真看到了什么」）。

**快捷键 `ctrl-t`**：`Ctrl-M` **不能用** —— 它在终端里就是回车（`\r`），crossterm 会报成
`KeyCode::Enter`，与「提交」正面撞车。仓库里 `Ctrl-J` 已经因为同一件事改掉了（`tui.rs:134` 的
`Newline` 与增强协议下的 `Shift+Enter`）。`Ctrl-T` 没被占，也不撞任何终端内建手势。

### §8 档位与模型的来源

- **模型候选**：`config.models` 的全部键（用户答案：列全部，缺 key 的标灰并说明缺哪个环境变量）。
- **档位候选**：`caps_for(当前模型).reasoning_efforts`，加 §5 的「默认」。空表时那一段显示
  `固定`（思考常开、没有旋钮），点它给一句说明。

两者都由循环算好塞进 `ConsoleRequest::Picker`（§9），前端只画。

### §9 选择器：`ConsoleRequest::Picker`

一条**一问一答**的通道，与 [`Ask`](../../src/render/input.rs) 同形：

```rust
ConsoleRequest::Picker(PickerRequest {
    title: String,                 // 「模型」/「思考强度」
    options: Vec<PickerOption>,     // { label, detail, current: bool, enabled: bool }
    reply: oneshot::Sender<Option<usize>>,
})
```

`Option<usize>` 是 `None` = 取消（`Esc` / 框外点击）。

plain 前端**不支持**：那条路径上没有选择器，`Picker` 在 `spawn_plain_console_with` 里回
`None`（与 `Catalog` 一样是「发完就完」的那一格）。于是 `/model` 在 plain 下不带参数时给用法。

### §10 弹窗不是覆盖层，是一块独占输入区的浮层

选择器**不**复用 `DetailView`（那个是只读正文，且独占整个指针）。它更接近**问卷**：一块浮层、
上下键移动、回车确认、`Esc` 取消。

但它与问卷有一处**必须不同**：问卷是**模型发起**的，答完它那条工具调用就结束了；选择器是
**人发起**的会话属性。所以它不进 `questionnaire()` 那条路径，而是 `TuiState` 上一个独立的
`picker: Option<Picker>` 状态，键盘归属按「谁立着谁拿」判（同 `detail` 与 `sidebar_keyboard`
那套）。

选哪个键盘区域？**立着时键盘归它**（`j/k` 移动、`Enter` 确认、`Esc` 取消），输入区此刻禁言；
关掉后键盘原样还回去 —— 与 `questionnaire` 立着时输入区禁言是同一条纪律。反过来，点击**不**被
它独占：框外点击关掉它（与详情覆盖层同一条），其余点击照旧分派。

### §11 状态行多一段

今天四段 `模型 X ┆ 模式名 ┆ 上下文 n% ┆ 状态词`，加一段变成：

```
模型 kimi-k3 ┆ 档位 high ┆ 询问 ┆ 上下文 42% ┆ 🌑 就绪
```

降级顺序要重排。**模型与档位是同一件事的两半**（这一场会话由谁答、答多深），所以它们**一起
丢或一起留**：先丢「模型 + 档位」，再丢模式，最后剩「上下文 + 状态词」—— 「在跑」仍然最后
丢，它住在永远在场的那一行上。

这**推翻** `tui-visual-language` §17 的四段与它的降级顺序（那一条的其余部分 —— 标签退后、
值靠前、分隔符只是线 —— 不动）。

### §12 两格可点，只有空闲时才有反应

状态行明写过「都不可点」。现在模型格与档位格可点，但：

- **运行中点击**：不弹窗，提示行给一句「这一回合跑完再切」（与 §6 同一句）。
- **讨论会话**：`facts.model` 是两个模型的拼法（`wording::discussion_pair`），点它给一句「讨论
  里的两位是配置事实，不在会话中途换」。快捷键与 `/model` 同样回这一句。判据是
  `SessionFacts` 多一个 `switchable: bool`，组装时定下来（讨论 CLI 那条路径传 false）。

### §13 换档会丢前缀缓存，这条代价要摆出来

Kimi 官方明写换 effort 会让上下文缓存失效、要重新 prefill（DeepSeek 与 MiniMax 同理）。所以
切换成功后的回执要带这句 —— 不是每次都拦着不让换，而是让读的人知道自己刚付了什么：

> 档位 → high（前缀缓存重来，下一次调用会重新读一遍上下文）

换模型同理。

## 明确不做

- **不做「关掉思考」这一档。** MiniMax 的 `thinking: {type: disabled}` 与 DeepSeek 的
  `thinking: {type: disabled}` 是**另一个请求参数**，且两者语义不同（M3.1 明确不收 disabled）。
  把它折成 effort 的一档会是假统一。`kimi-for-coding-highspeed` 那种「思考常开、没有旋钮」的
  模型显示 `固定`，那才是诚实的读法。
- **不在模型切换时重开上下文。** 换 provider 之后历史照旧投影 —— 新模型看得懂 OpenAI 形状的
  消息（三个厂商都是 OpenAI 兼容）。真要重来是 `/clear` 的事。
- **不换执行者与合成器的模型。** `SessionConfig::executor_model` / `synthesizer_model` 是**配置
  事实**，在组装期定下。换模型时它们原样留着；一个指向别的 profile 的 executor 覆盖是配置
  自己的事，不在这里解决。
- **不在 plain 前端开选择器。** 那条路径上没有键盘；`/model <id>` 与 `/effort <档>` 是它能用的
  全部。
- **不改 `--model` 旗标的语义。** 它仍然是**起手**那一个，只是不再是唯一一个办法。
- **不换键位增强协议。** `ctrl-t` 与 `Ctrl-J` 一样在普通终端里到得了。

## 决定速查

| 问 | 落点 |
| --- | --- |
| 四个入口是什么 | 点状态行两格、`/model`、`/effort`、`ctrl-t`（§7） |
| 为什么不是 `ctrl-m` | 它就是回车（§7） |
| 什么时候生效 | 只在空闲；运行中拒绝并给回执（§6） |
| 候选从哪来 | 模型 = `config.models` 全部（缺 key 标灰）；档位 = 该模型的能力表 + 「默认」（§8） |
| 档位有哪些 | `low`/`medium`/`high`/`xhigh`/`max`，每模型一份（§1、§4） |
| 讨论会话里能切吗 | 不能，只显示（§12） |
| 换 provider 换了发言者名字怎么办 | 跟着变，新名字走空槽位（§4） |
| 上下文窗口变了怎么办 | 循环推一条 `SessionUpdate` 回前端（§3） |
| 换档的代价 | 前缀缓存重来，回执里说（§13） |
| 状态行变成几段 | 五段，模型与档位同生共死（§11） |

## 一手材料

- <https://www.kimi.com/code/docs/en/kimi-code/models.html>：Kimi Code 四个 model id 的档位
  （K3 与 K2.8 Preview 是 `low`/`high`/`max`，K2.7 HighSpeed 是 `Thinking:ON` 没有档位）、换档
  与换模型的缓存代价、第三方工具的 effort 映射表。
- <https://platform.kimi.ai/docs/api/models-overview>：Kimi 开放平台一侧，`kimi-k3` 的
  `reasoning_effort` 是 `low`/`high`/`max`（默认 `max`），K2.x 两个 id 都不支持。
- <https://api-docs.deepseek.com/guides/thinking_mode/>：OpenAI 格式下
  `reasoning_effort` 是 `low`/`high`/`max`，默认 `high`；思考模式默认开。
- <https://platform.minimax.io/docs/api-reference/text-openai-api.md>：`reasoning_effort` 对
  `MiniMax-M3.1-Flash-Preview` 有效，取 `low`/`medium`/`high`/`xhigh`/`max`（默认 `max`，
  **不支持** `none`），M3 不收这个参数。

## 与既有决定的冲突（明着挑明）

1. **推翻 `minimax-provider/spec.md` 的「明确不做」那一段**：「不做 `thinking` / `service_tier` /
   `reasoning_effort` 的新档位。heng 的推理档位只有 `low` / `high` / `max`（会话开始定死）」。
   那个 spec 立的时候没有选择器，所以「只有三档」与「会话开始定死」是同一个约束的两面。现在
   有了会话中途切换这个需求，「定死」这一半被推翻，而「三档」这一半由 §1 一起推翻（补上
   `medium` 与 `xhigh`，因为 MiniMax M3.1 与 DeepSeek 都认）。
2. **推翻 `config.rs` 里 `GenerationParams::reasoning_effort` 的注释**（「整个会话钉住；中途从不
   切换」）与 `ReasoningEffort` 的文档注释（「档位在会话开始时就定死」）。缓存那条理由留着 ——
   它变成了 §13 的回执，而不是一条禁令。
3. **推翻 `tui-visual-language` §17**：状态行从四段变五段，降级顺序改成「先丢模型 + 档位」。
   §17 的行内三档不变。
4. **推翻 `CONTEXT.md` 的**状态行**词条**：「都不可点」改成「模型与档位两格可点」。词条本身要
   改写（见票 06）。
