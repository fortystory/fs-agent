# 15: 硬 plan 模式

**What to build:** 一个「硬」计划模式——模型能读、能写计划文件，别的写操作一律拒绝；进出**只由用户的手势**决定，派执行者也绕不过去。

Blocked by: 04

Status: done

**参考:** spec §13（硬 plan 模式）、§12（模式表与四条不变量）

- [x] plan 模式 = 策略里的**一条预设**（`effect()` 非 `ReadOnly` → `Deny`），**不新增状态机**；`bash` 也拒（`Exclusive`，shell 里能写文件）
- [x] 进出**只用手势**（`/plan` / `/endplan` / Shift+Tab），**不进工具面**（给模型一个 `exit_plan_mode` 等于把「能不能写」交回给模型）
- [x] 计划落项目根 `PLAN.md`，它是该模式下**唯一**的写豁免；豁免形状必须是「`WritePaths` 的**全部**路径都是它」（否则借道）；`Exclusive` **不可豁免**
- [x] 撞上已存在的 `PLAN.md` → **先问**用户（覆盖 / 追加 / 保留）；「覆盖」由 CLI 清空（read-before-write 会拒写已存在文件；工具绝不擅自删用户的文件）
- [x] 模式是 `Session` 的策略值、**不进事件流**；`--continue` 回到配置值；审计靠 `PermissionDecided`
- [x] 进入时注入一条短指令（`ContextInjected { source: PlanMode }`，与钉住的首条 user 消息同档、永不裁剪），**compaction 后不重新注入**
- [x] plan 模式的拒绝**沿委派链继承**（派执行者去写也绕不过）
- [x] 模式表四条不变量对 plan 同样成立（断路器最外、hook 只能收紧、传播 `Deny`/`Ask`、无交互降级）
- [x] e2e：进 plan 模式 → 断言写别的文件被拒、写 `PLAN.md` 被放行、`bash` 被拒、退出后恢复

## Comments

实现落点：`src/permissions.rs`（`Mode::Plan` / `PlanConflict` / `is_plan_write` / `Asker::ask_plan_conflict`）、`src/lib.rs`（`Harness::enter_plan_mode` / `exit_plan_mode` / `mode`）、`src/session/mod.rs`（`Session::mode` / `set_mode`）、`src/context.rs`（`plan_mode_instruction` + `trim` 的 pin 判据）、`src/provider/mod.rs` + `provider/projection.rs`（`Message::User` 的 `injected` 标记）、`tests/support/asker.rs`（三问脚本）。测试在 `tests/permission_gate.rs`（门真值表 4 例）与 `tests/plan_mode.rs`（e2e 11 例）。文档：`docs/plan-mode.md`。

实现期把票面留白写实的几处：

1. **豁免是 deny 里的一个与项，所以 `stance` 从「看 effect」改成「看整个 call」。** 在「`deny > ask > allow` 且忽略具体程度」的代数下窄 allow 压不过宽 deny，`PLAN.md` 的豁免只能进谓词本身（与票 04 的 `.env` 家族同一个理由）。判据复用既有的 `write_set_equals(call, [cwd/PLAN.md])`：整集相等、`Exclusive` 天然借不到道、`<repo>/sub/PLAN.md` 不算。`Deny` 仍是**地板**，规则与 hook 都压不动；断路器照旧最外（票 04 的代数原样成立，plan 只是换了个默认+谓词）。
2. **钉住标记：给 `Message::User` 加一个 wire 不可见的 `injected: bool`。** 这是票 07 明确留给本票的那一处（`context.rs` 原注释：*"that ticket must extend the pin marker when it lands"*）。`trim` 的判据从「开头那一段的长度」改成「消息种类」：`System` 或 `injected` 即钉住，`round_starts` 只认非注入的 `user`。这样中途注入既不会被旧轮次丢帧带走，也**不会**被当成轮次边界把一轮劈开（丢整轮时按「丢掉除钉住项外的全部」做，因此不会空转）。副作用是顺手修掉一处潜在误判：多说话人合并成的那条 `user`（`name: None`）以前会被当成钉住项。
3. **`injected` 不进化事件流、也不上线级形状**：`openai::message_json` 无视它，`#[serde(default)]` 让既有的反序列化继续成立。它只服务于 `trim`。
4. **手势是前端状态，不入 `Session`**：`Harness` 拿一个 `plan_restore: Option<Mode>` 记住「进 plan 之前是什么模式」，`/endplan` 恢复它——所以从 `auto` 进、出的是 `auto`，不是写死的 `ask`。`--continue` 组装的是新 harness ⇒ 回到配置值，这条 e2e 直接断言（同一个 log 用 `auto` 重新组装，事件流里没有任何字段带着模式；审计只有 `PermissionDecided`）。`enter_plan_mode` 重复调用是 no-op（不二次注入；判据是**当前模式**而不是 `plan_restore`，免得配置值就是 plan 时误判）。记录注入失败会把模式回滚，模式与解释它的那条指令不拆开。`exit_plan_mode` 返回 `Result<bool, Error>`：它要追加一条撤回事件，所以它和进入一样会失败。**没接的一端**：若会话是**组装期就被配成 plan**（今天不可达：没有配置键、CLI 写死 `ask`），它没有「进 plan 之前」的模式可回，因此停在 plan——那个目的地属于前端的选择面（票 18），连同「配置成 plan 时也要在骨架期注入一次」一起。
5. **`Asker` 从一个问题变成两个：** 权限门的 `Ask`，加 `ask_plan_conflict(path) -> PlanConflict`。同一个键盘出来的两个问题，一个端口；前端（含测试夹具）实现两个方法，编译器强制它表态。无 asker 时**降级为 `Keep`**：手势本来就只发生在交互前端，但脚本能调手势，而它绝不能替用户清空一个文件——这与门里 `Ask→Deny` 是同一条价值取向（无人应答时不做破坏性动作），但**不是**同一个机制（那条由循环在门外做，这条由手势自己选）。
6. **三个答案里只有「覆盖」动文件，`追加` 与 `保留` 靠注入指令区分。** 票面只规定了「覆盖由 CLI 清空」，但那样两个答案在磁盘上完全等价（死枚举）；实现选择把答案写进那条短指令：追加=读它并在其后追加，保留=读它、当作现行计划、不要整体重写。清空由 `Harness` 做（`std::fs::write(path, "")`），因为 read-before-write 会拒写未读过的已存在文件，而工具绝不擅自删用户的文件。
7. **`bash` 没有人认领（票面之外发现）。** spec §7 把 `bash` 列进 v1 内建工具集，但 19 张票里没有它的主人（03/04 的注释都写「后续票」，09 写「`bash` / `task` 未做」，`task` 由 11 补上、`bash` 漏了）。所以「`bash` 被拒」这条 e2e 用一个注入了 `Exclusive` 的测试工具断言（门的真值表另有 4 例直接覆盖）；缺口另开 **票 20**，票 20 的验收里包含「把这里的测试替身换成真工具」。

8. **离开 plan 模式要撤回那条指令（两轴 review 抓到的真缺陷，已回改 spec）。** 指令描述的是一个**状态**；只在进入时注入、离开时什么都不做，会让门已经放行而模型还被反复告知「只能写 `PLAN.md`」——这是本票最糟的一档不一致。修法走仓库自己的机制：`HistorySuperseded { targets: [那条注入], reason: ModeChange }`（`agent::retire_plan_instructions`），历史一条不改，投影不再重放它；`ExitPlanMode` 先撤回再换模式，追加失败就不会出现「模式没换但指令没了」或反过来。`--continue` 同理：模式不进流、恢复后是配置值，所以被杀掉的进程留下的那条指令在 `OpenedSession::start` 里补一次撤回（与恢复悬空 `tool_call` 同一处）。代价写明：模式切换点必有一次前缀缓存未命中——这本来就是手势该付的。`HistoryReason` 因此多一个 `ModeChange`（spec §2 的枚举已同步，§13 补了这条决定）。

**这一票证明到哪、没证明到哪**（`tests/plan_mode.rs` 11 例 + `tests/permission_gate.rs` 4 例）：

- 证明了：进模式换策略 + 恰好一条注入（重复进入不重复）；出模式恢复原模式、写操作随之恢复，**且下一条请求里不再有那条指令**（撤回是按投影断言的，不是按日志：日志一条不少，只多一条 `HistorySuperseded`）；`--continue` 用别的模式回来时补撤回陈旧指令；`auto` 起步 + 问 `AlwaysAllow` 时执行者照样写不动（**沿委派链继承**是 e2e 断言的，不是靠推断）；注入落在请求里是**它自己的一条** `user` 消息（不并进首条钉住块）；中途注入在丢整轮时存活且不劈开轮次（`trim` 纯函数）；模式不是事件流上的任何字段。
- **键位没接**：`/plan` `/endplan` / Shift+Tab 的按键属于渲染器（票 18），库里只有 `enter_plan_mode` / `exit_plan_mode` 两个调用入口（与票 13 的 Esc、票 12 的 `--continue` 旗标同一种交接）。
- **`config.toml` 里没有模式键**，所以「配置值」目前是前端组装时给的那个 `Mode`（today：`ask`/`auto`）。模式的选择面随票 18 落；「组装期就是 plan」这一档（初始注入 + 离开目的地）也随它落，本票只保证它不会让 `exit_plan_mode` 乱回一个模式。
- **hook 只能收紧 / 无交互 `Ask→Deny`** 两条对 plan 是**继承**而非新测，而且第一条是**类型性质**而不是可单测行为：`Constraint::Tighten` 只接受 `Tightening`（`Ask | Deny`，**按构造**没有 `Allow`），生效裁决恒为 `gate.join(tightening)`——plan 的 `Deny` 是 `Decision` 的全序顶，再测一遍等于测 `Deny.join(_) == Deny`。而 plan 的拒绝是 `Deny` 不是 `Ask`，所以没有「无交互降级」可测（那一条仍由循环在门外对 `ask` 生效）。
