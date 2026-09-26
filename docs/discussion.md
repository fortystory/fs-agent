# 讨论

一场讨论是**两个讨论者在同一条共享事件流上回答同一个问题**，外加一次收尾调用，把选项空间
铺开。它是 spec §15，也是这个项目存在的理由：单一模型的判断没有对照，所以 harness 把两个
模型**真的**在哪里分歧展示给用户，而不是替他们收敛。两者**本来就该**异构 —— 两家厂商，好让
第二个判断是独立的，而不是第二次采样 —— 但协议本身只要求两个**身份**
（见[跑一场](#跑一场)）。

## 谁做什么

协议分成三层，这个分层本身是承重的：

| 层 | 管什么 |
| --- | --- |
| `discussion`（`src/discussion.rs`、`src/discussion/protocol.rs`） | **规则**：两个结论是否一致、这一轮里谁作了答、能不能再开一轮、私有指令与合成器提示词。纯的，而且从不碰 `provider`。 |
| `agent`（`run_turn`、`run_discussion`、`run_single_shot`、`agent::executor`、`agent::cancel`） | **控制流**：轮次循环、两个并发的回合、收尾调用、`task` 调用派出的执行者，以及提前叫停它的那一个手势。这一层是事件流的唯一写者 —— 每一次追加都经过 `agent::append_event` —— 也是 provider 的唯一调用者（spec §3）。 |
| 组装（`assemble_discussion`） | **名册**：一个 `SessionScaffold` 开成两个讨论者会话与一个合成器会话，三者共享同一个日志、同一张工具表、同一张锁表与同一份权限策略。 |

`discussion` 判定，`agent` 写流。正因如此，轮次边界事件由 `agent::record_round_*` 追加，
尽管「它们何时发生」是协议判定的。

## 轮次结构

**独立首轮 → 揭示 → 定向第二轮 → 合成。** 一场讨论是 **3 次 provider 调用**（没有分歧）或
**5 次**（有分歧）。轮次之间的判定是机械的，不花任何东西：

- 每个讨论者都在作答末尾、另起一行，以 `CONCLUSION:` 开头写一行自由结论
  （`discussion::protocol::CONCLUSION_MARKER` —— 提示词与解析器共用这个常量，所以两者漂移
  不了）。带装饰的标记（`**CONCLUSION:** x`）照样读成一条结论。
- 把两条结论归一化（装饰、标点、大小写、空白），然后**相等**、或者**一条包含另一条**，就
  算一致。没有裁判、没有 `response_format`、没有极性标签：那两行结论**就是**分歧记录
  （`DivergenceRecorded { round, topic, positions }`）。
- 不一致就恰好开一轮定向轮，封顶 `DEFAULT_MAX_ROUNDS = 2`（可由
  `DiscussionParts::max_rounds` 配置）。第三轮并不需要新机制 —— `RoundStarted { mode }`
  那个位置本来就在 —— 但给子串匹配的误报划界的，正是这个封顶。
- `RoundEnded` **只由结束辩论阶段的那一轮**发出，所以它的终态原因（`NoDivergence` /
  `Consensus` / `RoundsExhausted` / `BudgetExhausted`，加上一次取消手势叫停本轮时的
  `Aborted`）永远是终态的，渲染器可以直接照它们行事。开出一轮定向轮的那一轮，则是由下一条
  `RoundStarted` 收尾的。

## 两条不能破的规矩

**第一轮是构造上独立的，不是靠时间上凑巧。** 两个讨论者同时在飞，所以一个快的 provider
答完了，另一个还在想。因此一个讨论者的投影在它那一轮的 `RoundStarted` 处切断：它看得到那个
`seq` 之前的一切，加上它自己后来的事件，永远看不到对方的同轮事件
（`agent::TurnScope::Round`）。任何重放（`sessions replay`，票 17）都必须重现这个窗口，
否则就对不上实际发出去的内容。

**协议指令是一个常量式的私有身份。** 它以打头的 `system` 消息到达模型，永不进事件流 ——
「后一轮的 `messages` 可以从流上重算」这句话能成立，靠的就是这一点。它对整场讨论都是常量
（包括定向轮的规则），因为 system 提示词是前缀的头，讨论中途改写它会把整个前缀缓存作废
（与钉住 `reasoning_effort` 是同一条理由，spec §4）。模型现在处在第几轮，是通过投影的
`[轮 N · 名字]` 前缀与对方揭示出来的作答到达它的。

## 失败

每一次失败都被记下来，讨论照常往前走；**没有任何东西会被重跑**。

| 情况 | 会发生什么 |
| --- | --- |
| 一方失败 | 那一方在这一轮**缺席**；辩论阶段以 `NoDivergence` 收尾，合成器照样跑。缺席是对流的一次*查询*（`round_attendance`）：那一方自己的 `TurnEnded { Error }`，加上本轮没有 `MessageCompleted`。 |
| 两方都失败 | `RoundEnded { Error }` + `SessionError`，没有收尾调用。 |
| 合成器失败 | `RoundEnded { Error }` + `SessionError`，产物为空。 |
| 一次取消手势 | 本轮以 `RoundEnded { Aborted }` 收尾 —— **不是** `Error`，也没有 `SessionError`，因为这是用户叫停的，不是讨论失败了。辩论阶段就此收口、不再去开合成器，而每一个已经开始过的 `tool_call`（包括某个还在跑的执行者的 `task`）照样各拿到它那一条结果（spec §6）。 |

合成器的提示词是从流上重建的，并且把每一个缺席方点出来（「这一轮没有作答（本轮缺席）」），
因为这一整条路径要防的那一次误读，正是把单方作答读成共识。它把每个讨论者的发言全文揭示
出来，从不揭示其私有推理。

## 跑一场

`fs-agent discuss "问题"` 是前端：配置挑名册，子命令把它组装起来，转录就是界面。

```toml
[discussion]
debaters = ["kimi-k3", "deepseek-v4-pro"]   # the pool, at least two (short form)
# max_rounds = 2                            # 1 independent + at most 1 targeted

[[discussion.debaters]]                     # or named personas
name = "张三"
model = "deepseek-v4-pro"
soul = "法外狂徒，思路不受限制"

[[discussion.debaters]]
name = "李四"
model = "deepseek-flash"
soul = "守法好公民，先找依据"
```

```sh
fs-agent discuss "问题"                      # draws two of the pool at random
fs-agent discuss --debaters 保守,激进 "问题"   # or names the two
```

- **是一个池子，不是一对。** `[discussion] debaters` 是一场讨论抽取的那个集合，因为谁来
  讨论是配置事实，而**哪**两位上场是关于某一次运行的决定；也因为讨论者是唯一一个路由永远
  不能交给别的模型的参与者（spec §17 —— `[routing]` 有 `synthesizer_model` 与
  `executor_model`，刻意没有讨论者的键）。解析会拒掉一个撑不起一场讨论的池子：成员不到
  两个、某个模型没配置过、某个名字当不了身份。
- **一个讨论者就是一个人物：一个 `name` 加一个 `model`。** 名字是它在流上的**身份**（下游
  的一切都是 `speaker_id` 的函数，spec §5），是转录给它打的标签，是它自己那句私有身份里
  叫它的称呼，也是模型可见的 `[轮 N · 名字]` 前缀写进去的东西。它可以是任意单个词，中文
  也一样：投影把名字原样写进前缀，只对 `name` 这个**字段**做净化，因为承载归属的是正文，
  而那个字段的字符集没有文档。
  简写 `debaters = ["kimi-k3", …]` 拿模型名当讨论者的名字，所以一个把**同一个模型列了两次**
  的池子**必须给它们起名字** —— 一个 model id 当不了两个身份，而报错会写清楚该写什么。
  没有自动加后缀这回事。
- **一个人物可以带一个 `soul`：它的性格，用用户的原话写。** 它是关于一个讨论者的、唯一
  无法从名字推出来的东西，所以它**落在流上** —— 一条署名给那个讨论者的
  `ContextInjected { source: Persona(name) }`，由 `discussion::persona_brief` 框住 —— 而
  投影只把它交给那一方，**不交给其他任何人**：另一位讨论者是在跟这个性格辩论，合成器读
  的是作答，不是性格。两个后果值得点名：
  * 它保持**可重算**：`sessions replay` 从流上重建那次调用，这就是灵魂不放进私有身份的原因
    （私有身份必须始终是名字的纯函数，spec §15）；
  * 它是**钉住的**：注入永不被裁（spec §10），所以灵魂有 `MAX_DEBATER_SOUL` 字符的上限，
    而不是让它去吃窗口。
- **哪两位上场是抽的。** `discussion::pick_pair(len, seed)` 按池子顺序、确定性地抽两个不同
  的池子成员（运行时用时钟当种子，测试里用常量）。`--debaters a,b` 则点明这一对。
- **异构是设计的前提，不是硬性要求。** 来自同一厂商的两个讨论者 —— 甚至同一个模型两次 ——
  是允许的，因为一个到期的订阅不该让讨论跑不起来，而对同一个模型的两次调用，在采样不
  一致时照样会分歧。损失掉的东西会说出来（每场讨论一行提示，由
  `Config::debaters_share_a_vendor` 对**真正上场**的那一对判定）。
- **合成器**是路由表的另一个落点（`[routing].synthesizer_model`）；没有路由任何东西时，它
  就是第一个讨论者的模型。一个值，一种写法。
- **问题**来自命令行（`fs-agent discuss "…"`），或者来自 stdin：终端上会提示你打一行，管道
  则会一直读到结尾（`echo 问题 | fs-agent discuss`）。
- **看它跑**：终端上得到 TUI（包括权限覆盖层 —— 讨论者想跑什么都走它），管道里得到 plain
  转录 —— 轮次分节行与两个讨论者都走 stderr，合成器的产物单独走 **stdout**。TUI 的转录活
  不过进程（ADR 0002），所以一次运行以打印会话 id 收尾：`fs-agent sessions show <id>` 才是
  那份持久的记录，`sessions stats <id>` 则把花费按讨论者拆开。
- **退出码**：一场跑起来的讨论是 0（包括被取消 —— 那正是用户要的），辩论阶段本身失败则是
  非零。

底下那条库接缝没变：先 `assemble_discussion(DiscussionParts)`，再
`harness.discuss(question)`。一个 harness 就是一场讨论，所以第二个问题意味着第二次组装 ——
子命令每次调用做的正是这件事。

## 在会话里（`/discuss`）

`/discuss [--debaters a,b] [问题]` **在你已经身处其中的那个会话的流上**跑一场讨论。
讨论者与合成器是那个会话的*兄弟*：`Session::fork` 把它的日志、工具表、写锁、权限策略、
作答者、技能与渲染器交给它们，只有各自的模型与私有身份不同（`Harness::discuss`）。两个
后果正是要害所在：

- **讨论继承这个会话的上下文。** 投影把发言那一方自己的回合变成 `assistant`，把**其他所有
  人的**变成 `user`（spec §5），于是会话的那个问题、以及它已经给出的作答，会作为「到目前为止
  的对话」到达讨论者。所以 `/discuss` 问的是「两个模型怎么看我们正在做的事」，而不是「在真空
  里回答这个」。
- **它追加到同一条流上。** 一个会话可以装一个回合、一场讨论、又一个回合、又一场讨论 —— 而
  `sessions show` 会把它们按顺序全读回来。会话事后照样能用；讨论只是它身上多发生的一件事。

共享一条流带来了什么后果，以及它们各自怎么被处理：

| 共享一条流意味着 | 由谁处理 |
| --- | --- |
| 轮次编号必须说清自己属于**哪一场**讨论 | 讨论接在流上已有的轮次之后编号（`discussion::last_round`），所以同一个会话里的第二次 `/discuss` 编成第 4、5、6 轮 —— 在会话内唯一，`round_attendance` 与 `sessions show --round N` 靠的正是这个唯一性 |
| 合成器不能拿到更早那场讨论的作答 | `discussion::debate_phase_start` 把它的材料限定在本次合成收口的那一个辩论阶段；`sessions replay` 问的是同一个函数，所以发出去的内容与重算出来的内容漂移不了 |
| token 额度已经在花掉了 | `total_usage` 读的是**整条**流，这是对的：额度是会话级的事实（spec §17） |
| 权限门是这个会话的 | 讨论者共享它的策略与询问端口，所以讨论者的一次工具调用提出的问题，与这个会话本来会提出的是同一个 |
| 轮次边界需要一个写者 | 一个日志、一个互斥锁、一个 `seq`：兄弟会话追加事件走的是这个会话用的同一个 `EventLog` —— 与一场全新讨论的两个讨论者本来就有的那个「单一写者」是同一条 |

一个不带问题的 `/discuss` 从会话里取问题：最后一条 user 角色的 `MessageCompleted`
（`Harness::last_question`）。还没问过任何东西时就没什么可讨论的，循环会直说，而不是编一个
问题出来。

`fs-agent discuss "问题"` 仍是另一条入口：一场**自己有一条流**的讨论 —— 一个新会话、没有
继承来的上下文、事后有它自己的 id 可以交给 `sessions show`。两者的差别恰好就是 `fork`：
`assemble_discussion` 从它刚创建的那个 scaffold 开出会话，`Harness::discuss` 则 fork 那个
活着的。

## 合成器的产物去了哪

一次独立的单发调用：没有回合、没有工具、在轮次里不占席位。它的产物作为一条署名 `System` 的
`MessageCompleted` 落地，而 headless 渲染器放到 **stdout** 上的就是这个。每个讨论者的回合
也以 `Completed` 收尾，所以在一轮之内，一个完成的回合是叙述（stderr），不是最终产物 ——
把两者分开的是轮次边界。

## 刻意不在这里的

N > 2 个讨论者（那会重新打开「N = 2 不做仲裁」这个决定 —— 组装拒掉除两个之外的任何名册）、
仲裁者或裁判（已被证据否掉，spec §15）、在**一个** harness 上问第二个问题（一个 harness
就是一场讨论，问两次意味着再造一个 —— `/discuss` 做的正是这件事，只不过在同一条流上）、
以及裁剪合成提示词（spec 要的是**全文**；那道预算闸门归票 14）。
