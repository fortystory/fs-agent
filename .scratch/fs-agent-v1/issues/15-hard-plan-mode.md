# 15: 硬 plan 模式

**What to build:** 一个「硬」计划模式——模型能读、能写计划文件，别的写操作一律拒绝；进出**只由用户的手势**决定，派执行者也绕不过去。

Blocked by: 04

Status: ready-for-agent

**参考:** spec §13（硬 plan 模式）、§12（模式表与四条不变量）

- [ ] plan 模式 = 策略里的**一条预设**（`effect()` 非 `ReadOnly` → `Deny`），**不新增状态机**；`bash` 也拒（`Exclusive`，shell 里能写文件）
- [ ] 进出**只用手势**（`/plan` / `/endplan` / Shift+Tab），**不进工具面**（给模型一个 `exit_plan_mode` 等于把「能不能写」交回给模型）
- [ ] 计划落项目根 `PLAN.md`，它是该模式下**唯一**的写豁免；豁免形状必须是「`WritePaths` 的**全部**路径都是它」（否则借道）；`Exclusive` **不可豁免**
- [ ] 撞上已存在的 `PLAN.md` → **先问**用户（覆盖 / 追加 / 保留）；「覆盖」由 CLI 清空（read-before-write 会拒写已存在文件；工具绝不擅自删用户的文件）
- [ ] 模式是 `Session` 的策略值、**不进事件流**；`--continue` 回到配置值；审计靠 `PermissionDecided`
- [ ] 进入时注入一条短指令（`ContextInjected { source: PlanMode }`，与钉住的首条 user 消息同档、永不裁剪），**compaction 后不重新注入**
- [ ] plan 模式的拒绝**沿委派链继承**（派执行者去写也绕不过）
- [ ] 模式表四条不变量对 plan 同样成立（断路器最外、hook 只能收紧、传播 `Deny`/`Ask`、无交互降级）
- [ ] e2e：进 plan 模式 → 断言写别的文件被拒、写 `PLAN.md` 被放行、`bash` 被拒、退出后恢复
