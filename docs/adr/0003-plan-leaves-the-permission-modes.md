# 「计划」从权限模式里搬出来：模式回到三档，计划交给模型的 `todo` 工具

权限**回到三档** `readonly` / `ask` / `auto`，`plan` 整个退场：`Mode::Plan` 与它的 stance、`is_plan_write`、`PLAN_FILE_NAME`、`/plan` 与 `/endplan`、进出/撤回/重放那套补丁、`docs/plan-mode.md` 全部删掉；`Shift+Tab` 从「切进 / 切出 plan」变成**在会话内循环三档**，`--mode` 与 `[permissions] mode` 补上入口。「计划」这件事改由模型自己的内建工具 `todo(list)` 承担（一次提交整份列表，列表就活在 `tool_call` 的 args 里，零 schema 改动），规则段里引导它开工前立待办、每完成一项更新。这**推翻 `.scratch/fs-agent-v1/spec.md` §13 的一整节**（及用户故事 78/80/82–84 关于 plan 的那几条），也把 §12 的模式表从四行改回三行。来源是用户提出的一轮改动 + grilling 的九个决议，折叠在 `.scratch/todo-and-modes/spec.md`。

## 为什么当初是模式，而不是一条规则

不是审美，是代数的形状。门的合并语义是 **`deny > ask > allow` 且忽略具体程度**（§12：规则优先级、hook 收紧、执行者继承共用一套合并），所以一条窄 `allow` 永远压不过一条宽 `deny`。「可以读、且**只**能写 `PLAN.md`」这句话里带着一个 deny（只读），它是允许一部分写的——于是那条豁免必须写成 **deny 条件里的一个与项**（「非 `ReadOnly` **且** 写入集的全部路径 ≠ `{PLAN.md}`」），而这样的谓词不是一条规则能表达的位置：它得是模式自己的 stance 才能保证没有任何 allow 翻过它。形状也必须是「写入集的**全部**路径都是它」，否则一次「顺手也写别的」的调用就借道过去了；`Exclusive`（shell）没有写入集，所以永远借不到。这套推理今天仍然成立——它是 `Scope` 那条「作用域是对这次调用的谓词」的来由，`Scope::PathSet` 也还在。

## 为什么现在搬出来

1. **两件事被绑在一档里。** plan 模式同时承载「权限」（只读 + 一条写豁免）与「计划的载体」（`PLAN.md` + 一条钉住的注入指令 + 一套进出/撤回/重放修补）。用户想「先想清楚再动手」时被迫接受「连写都不能写」；想要一份能看进度的清单，也不得不进这一档。权限与计划是两件事，一个是用户对 agent 的授权，一个是 agent 自己的工作记录。
2. **权限那一轴被浪费了。** 四档里 `readonly` 与 `auto` 实现完整、测试齐，但**没有任何入口**（三处组装点硬编码 `Ask`），而 `plan` 占了唯一的运行期手势。用户要的「工作区里随便干、外面要问」本来也是被这一条卡住的（`.scratch/sandbox/seed.md` 那份意向仍在，本轮不新增档位）。
3. **计划的载体不该是权限。** 一份待办列表是模型的工作记录：它该是一个它能随时读写的**工具**、在界面上看得见（侧栏一个 `todo` 标签），而且工具表是缓存前缀的一部分——组装期决定，中途不增删。零 schema 改动：列表就活在那条 `tool_call` 的 args / result 里，`--continue` / `replay` / 审计 / 侧栏重算全部免费。
4. **「能不能写」还回给用户。** 原来的论证是「给模型一个 `exit_plan_mode` 等于把能不能写交回给模型」，所以进出只用手势。本决定把选择权交给用户：三档由用户选（配置 / 旗标 / 手势），模型只在被拒时从 `PermissionDecided.reason` 读到档位。**引导不等于强制**：规则段请模型先立待办，但不做首轮 `tool_choice` 强制、不在门层强制——门层强制会把「列表」与「权限」重新绑在一起，正是这次要解开的东西。

## Consequences

- **模型不再被强制只读。** 「先想清楚再动手」现在是**引导**：规则段加一条（英文，模型可见），列表由模型自己维护。清单不完整、不更新，门都不会说话；想要硬约束就选 `readonly`。
- **循环不注入任何东西。** 切档会改 `messages` 头部就等于废掉前缀缓存，所以循环**只改策略值**：代价是模型不会事先知道档位变了，它第一次被拒（`PermissionDecided.reason` 里写着「mode readonly: …」）才知道；好处是手势不花缓存，也不动历史。这一条与 §12「模式是 `Session` 的策略值、不进事件流」一致，`--continue` 回到配置里的那一档。
- **`Shift+Tab` 从「切一档」变成「循环三档」。** `FrontEndEvent::TogglePlan` 改名 `CycleMode`；TUI 的状态行照旧显示 `模式 X`，前端按 `Mode::next()` 自己走一步、循环再走同一步（`ModeCycle` 句柄，理由同 `CancelSignal`：运行期那个被 pin 住的 future 借着 harness）。
- **老的 `PlanMode` / `ModeChange` 事件仍在，但不再发射。** `events::ContextSource::PlanMode` 与 `events::HistoryReason::ModeChange` **必须留着**：老会话的流里有 `ContextInjected { source: PlanMode }` 与 `HistorySuperseded { reason: ModeChange }`，删变体会让 `--continue` 在旧会话上直接反序列化失败。它们只是不再被产生；重放时那两行照旧读得懂（措辞层保留「计划模式」/「模式变更」两个标签），但**不再改会话的档位**——档位是组装期注入的值。
- **`PLAN.md` 文件本身不动，只是不再是任何东西的特例。** 它在 `readonly` 下不可写、在 `ask` 下要问、在 `auto` 下随便写；用户的文件也不再有任何 CLI 代码会去清空它（原来那条「覆盖」清空路径随 `/plan` 一起删了）。
- **模型可见文本的一侧只增不改（ADR 0001）。** 规则段那条待办指令是新加的一行，落在每个会话的缓存前缀里；改动它等于让所有现存会话的前缀缓存失效一次，所以措辞写死在代码里、用英文。
- **测试的落点跟着搬。** 门的真值表（`tests/permission_gate.rs`）、配置解析（`tests/config_profiles.rs`）、端到端的三档与继承（`tests/modes.rs`，原 `tests/plan_mode.rs`）、帧上的循环（`tests/render_layout.rs`）、老流的向后兼容（`tests/history_replay.rs`）。
