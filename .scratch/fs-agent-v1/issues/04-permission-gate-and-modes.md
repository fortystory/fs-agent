# 04: 权限门与三个内置模式

**What to build:** 每一次工具调用都要过权限门。用户在 `readonly` / `ask` / `auto` 三档之间切换时行为可预测，且最坏情况有硬底——断路器短路 deny，任何规则与允许都翻不动它。

Blocked by: 03 · 工具循环与文件工具

Status: done

**参考:** spec §12（规则、模式与传播）、§20（凭据路径）

- [x] 规则 = `subject` + `Scope`（`Tool(glob)` / `CommandPrefix(argv)` / `Path(pattern)` / `PathSet(exact)` / `All`）+ `action` + `propagate`
- [x] 优先级 `deny > ask > allow` 且**忽略具体程度**；作用域是「对这次调用的**谓词**」，不靠具体程度取胜
- [x] 评估次序：**断路器（短路 deny，早于一切）→ 规则取上确界 → 无交互时 `Ask→Deny`**（降级由循环在门外做，门的裁决如实保持 `Ask`，理由进 `reason`）
- [x] 断路器（是**短路**不是规则）：敏感路径写入永不自动批准；`rm` 打到 `/` / `~` 及其父目录一律 `Deny`，任何 allow 都翻不动
- [x] 三个模式（`readonly` 非只读即拒 / `ask` 写问读放 / `auto` 默认放行但**不等于跳过权限**）+ 四条不变量（断路器最外、hook 只能收紧、传播的是 `Deny`/`Ask`、无交互降级）
- [x] `.env` 家族默认 `deny`（`*.example` / `*.sample` / `*.template` 除外）
- [x] deny **不从工具表移除**工具（要「不存在」就在组装期不注册）
- [x] 「总是允许」只改会话内策略：不写 `config.toml`、不进事件流
- [x] 权限拒绝 / 用户拒绝 / 无交互降级三条异常路径各**由循环合成**一条错误结果 ⇒ 加上正常路径，「恰好一条结果」仍成立
- [x] 门是纯函数；真值表测试覆盖 `模式 × 动作 × Scope × propagate`，外加断路器短路与上确界合并

## Comments

实现落点：`src/permissions.rs`（门）、`src/tools/{tool,registry,paths}.rs`（调用事实）、`src/session.rs` / `src/lib.rs`（策略与注入）、`src/agent.rs`（循环）。测试在 `tests/permission_gate.rs`（纯函数真值表）与 `tests/permission_loop.rs`（组装接缝）。

实现期收口、把票面留白写实的几处：

1. **门的输入是「已解析的调用事实」，不是原始 args。** 路径解析（唯一会 canonicalize 的一步）留在 `Registry::facts()`；门收到的是 `Call { tool_name, effect, 已解析的写/读目标, argv, cwd, home, path_error }`。这样「门是纯函数」成立，而 `Path` / `PathSet` 作用域拿到的是绝对路径（相对模式按会话 cwd 匹配）。
2. **模式 = 默认值 + 地板；规则覆盖默认，但压不过地板。** 「总是允许」要真的有用，`ask` 的 `Ask` 就必须是**默认**（有规则命中时由规则的上确界取代）；而 `readonly` 的 `Deny` 是**地板**，因为「没有任何写豁免」就是这个模式的定义。断路器更外一层：硬 `Deny` 短路（`rm`、`.git`/`.ssh` 写入），「永不自动批准」的 shell rc 写入是 `Ask` 地板（`Deny` 仍可叠加，不会被它放松）。`rm` 的相对参数按会话 cwd 词法折叠（`rm -rf ../..` 只要折到 home 或其父级就拒），`~/a/../..` 也折。
3. **`.env` 家族的例外写成代码内的默认约束，而不是一条窄 `allow`。** 在「忽略具体程度」的代数下窄 allow 压不过宽 deny，所以 `*.example` / `*.sample` / `*.template` 的例外只能进谓词本身；这里作为策略地板实现（`deny` 是上确界，等价于不可放松）。家族判据是 `.env` / `.env.*` / `*.env`（不误伤 `.envrc`、`.environment`）。
4. **「总是允许」= 追加一条 `Allow` 规则**（`subject = 当前 participant`、`scope = Tool(工具名)`），只改 `Session` 的值：不写 `config.toml`、不进事件流；`propagate` 按动作默认 `false`，不会漏给执行者。
5. **无交互降级的记录方式：** 门的裁决保持 `Ask`；循环在没有 asker 时合成错误结果，并写 `PermissionDecided { decision: Deny, source: Policy, reason: "…; downgraded to deny: no interactive answerer" }`，审计能把「没有终端」与策略拒绝分开。有 asker 时先写 `PermissionAsked`，再按回答写 `PermissionDecided { source: User }`。每次调用**恰好一条** `PermissionDecided`（放行也写，票 19 的 `decision × source` 计数器才有数据源）。
6. **`Tool` 多一个 `command(args) -> Option<Vec<String>>`。** `CommandPrefix` 作用域与 `rm` 断路器要在进程启动前看到 argv；`bash`（后续票）是它的主人，v1 其余工具默认 `None`。这是三件套加 `read_paths` 之后的第五个方法，属实现期决定。
7. **cwd 路径限制是门的一道 `Deny` 地板。** `Registry::facts()` 把无法解析的目标留在词法形态并记进 `CallFacts.path_error`；门据此 `Deny`，所以**每次工具调用都有裁决**，而且记录与结果一致（不会出现「记了 Allow 却被别处拒掉」的偏斜）。`CallFacts::guardrails()` 仍保留同一道检查作为纵深，`auto` 也拦得住。（「用权限规则放宽 cwd」尚未接线，留给凭据票。）
8. **传播语义不复制模式。** `Policy::inherited_rules()` 只交出 `propagate` 为真的规则（默认 `Deny`/`Ask`），执行者的模式是它自己的组装决定——复制 `auto` 父级的模式等于把 Allow 也继承了进去。`Decision::join` / `Rule::with_propagate` 目前调用点确实偏少，但它们正是票 05（hook 收紧）与票 15/11（plan 与执行者）要用的那套代数，先按票面钉住。
9. **装了 asker 的 e2e 夹具**（`tests/support/asker.rs`）：既有 e2e 用「`ask` 模式 + 永远批准的用户」跑，行为与票 03 一致；权限专属场景各自脚本化回答或注入 `None`。

**明确留给后续票的**：模式的**选择面**（手势 / CLI / `config.toml` 读取）随交互式入口一起落（票 18，`--continue` 回到配置值在票 12）；`bash` 落地前 `CommandPrefix` 与 `rm` 断路器只有门的单测覆盖，这正是先钉住「门能看到 argv」的原因。

