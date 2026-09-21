# 16: 凭据暴露面与打码流水线

**What to build:** 密钥不会顺着事件流、落盘文件或跨 agent 的发言漏出去，同时工具执行仍然拿得到真值。agent 自己的 key 就在本机配置文件里，所以这条是自用场景的真风险。

Blocked by: 07, 12

Status: done

**参考:** spec §20（凭据暴露面）、§10（截断落盘）

- [x] 流水线固定为 **打码 → 截断 → 落盘**，打码发生在**入流前** ⇒ **流上的文本 == 模型看到的文本**，而**工具执行时用真值**
- [x] 打码是**值级、best-effort**，范围**含消息正文**（跨 agent 那条路径的唯一剩余形态就是「他在发言里复述了密钥」）
- [x] `outputs/<tool_call_id>.txt` **打码**；`outputs/<tool_call_id>.before` **不打码**（它是 `/undo` 的字节级还原源，内容本来就是用户自己的工作区内容）
- [x] 文件工具的**模型供路径**限定在会话 cwd 及其子树；例外走权限规则（限的是模型给的路径，不是 harness 自己读的 skill / 会话目录）
- [x] `.env` deny + cwd 限制**两条一起**挡住凭据文件读取；命令回显靠入流前打码
- [x] root / sudo 下**拒绝启动**、**不给 bypass flag**
- [x] 定位写进文档：prompt injection 是「降低上限」而非解决问题——门是 `(policy, tool, args)` 的纯函数、从不读对话文本，所以破坏半径 = 你的策略允许的半径
- [x] e2e：让一个假密钥流经工具输出与消息正文，断言事件流与 `.txt` 里是打码值、而工具实际拿到真值；`.before` 里是真值

## Comments

实现落点：`src/events.rs`（`Redactor` / `REDACTED` / `EventPayload::redact`）、`src/config.rs`（`Config::redactor` 与 `SessionConfig::redactor` / `with_redactor`）、`src/agent.rs`（`append_event` 入流前打码、`emit_completed` 的「打码 → 截断 → 落盘」、`end_turn` / `run_single_shot` 的返回值同源打码）、`src/agent/executor.rs`（两处生命周期事件）、`src/cli.rs`（`root_refusal`，`main` 的第一件事）。测试：`tests/credentials.rs`（12 例）。文档：`docs/credentials.md`（新增）、`CONTEXT.md`（新增「安全」节与「打码（Redactor）」条目）。

实现期把票面留白写实的几处：

1. **打码挂在「唯一写入路径」上，不挂在落盘层。** spec §3 不变量 3 说只有循环写流，`agent::append_event` 就是那条路径（执行者的生命周期事件也走它）。于是它的签名多收一个 `&Redactor`，在 `log.append` 之前把 payload 的每个自由文本字段打码：流、文件、渲染器、投影四处同时一致——「流上的文本 == 模型看到的文本」是构造性质，不靠每个调用点自觉。工具结果那条另有一处前置打码（见下），因为 `.txt` 落盘发生在 append 之前。
2. **`Redactor` 住在 `events`，不住在 `context`。** 流水线（打码 → 截断 → 落盘）本来在 `context` 更顺，但依赖 DAG 里 `events` 零内部依赖，而需要它的两侧——`config`（知道密钥）与 `agent`（唯一的写者）——都在它上面；放 `context` 会让 `config → context → config` 成环（`context/repo_map.rs` 已经依赖 `config`）。`events` 本来也承担「流上允许出现什么」这类 schema 约定（`hook_format` 是同一形状的先例）。
3. **`Redactor` 搭 `SessionConfig` 走，不新开注入面。** spec §17 把 `Config::session_config()` 定为「文件里的配置 → 每个 agent 的注入值」的唯一一处，所以 `Config::redactor()`（所有已解析 provider key 的值集）在那里填进 `SessionConfig`：任何组装路径都自动拿到打码，不依赖谁记得拷贝；执行者 clone `SessionConfig` 时自然继承，讨论的两个讨论者与合成器也一致。`SessionConfig` 名义上是 per-agent 值，字段文档里写了它为什么是 session 级事实。
4. **`emit_completed` 先打码再截断**，所以 `outputs/<tool_call_id>.txt` 落盘的就是打码后的全文（票面点名的顺序）。`<tool_call_id>.before` 由编辑工具直接写、不经过这条管线，保持真值——它是 `/undo` 的字节级还原源，内容本来就是用户自己的工作区内容。
5. **args 也打码**（JSON 叶子级，对象键不动）。不这么做的话，投影里「他人工具调用只留一行摘要」会把模型塞进参数里的密钥摘要给另一个 agent 看。代价写进了 `docs/credentials.md`：若被替换/插入的区段本身含已登记密钥，记录的 args 就是打码值，`/undo` 的重放校验无法确认区段，于是**拒绝**（`Stale` / `Ambiguous`）而不是猜——安全方向，但确实是真后果。
6. **返回值与流同源**：`TurnOutcome.text` 与 `DiscussionOutcome.synthesis` 也过一遍打码，免得同一个值在内存里留一份没打码的副本被前端打印（流本身已经打码，这里是第二个出口）。
7. **root 拒绝是纯函数**：`cli::root_refusal(euid)` 只读 uid，`main` 在解析参数、建 runtime 之前调用——「没有 bypass flag」因此是结构性质而不是承诺（连 `--help` 也拒）。
8. **`.env` deny 与 cwd 限制是既有护栏**（票 04 / 票 03），本票补的是把它们与打码放在同一条 e2e 上：`.env` 读被策略拒、`/etc/hostname` 读被 workspace 限制拒，两条各只落一条错误结果，流上不留真值。无密钥配置时 `Redactor` 为空、`redact` 立即返回，行为与改动前完全一致（全量既有测试未改一行仍绿）。

**两轴 review（Standards / Spec）抓到的真问题与处理**：

*Standards*：无硬违规。五条判断性意见都改了——① 三处「文本离开 harness」各自手写 `session.config().redactor.redacted(&text)`，收成 `Session::redacted()`（另加 `Session::redactor()` 访问器，`append_event` 的取用点也走它），把这条形状收到一处；② `Redactor::redact` 里先 `any()` 预扫描再逐条 `contains`，预扫描是纯冗余，删掉（`mem::take` + 写回是移动不是拷贝，无密钥时只付扫描）；③ 长度单位不一致（门限用 `chars().count()`、排序用 `len()`）统一成 `chars()`；④ 文档里唯一一处 rustdoc 式 `[\`x\`]` 改成普通反引号（`docs/*.md` 的既有写法）；⑤ `unsafe` 补 `// SAFETY:` 标记，与 `bash.rs` 的 `killpg` 同格式。

*Spec*：两条缺失、一条「实现正确但代价真实」、一条我自己另查出的 spec 缺口。

1. **短密钥不打码（< 8 字符）** 保留门限：值级替换一个三字符串会把整场会话的普通文本洗烂，且这不是厂商密钥的长度。但把它从代码注释提升为**文档里的明确边界**（`docs/credentials.md`「What value-level redaction is not」第一条），并在这里写明这是有意取舍——配置一个病态短的 key 就不会被打码，`Redactor` 的单元测试把这个行为钉住。
2. **讨论名册不校验 redactor 一致** 是真空洞（一个讨论者被打码、另一个没有，同一条流），照 `budget` 校验加了一条同样形状的校验（`assemble_discussion`，`Error::Discussion`），新增 e2e `a_discussion_refuses_a_roster_whose_redactors_disagree`。
3. **args 打码确实会让 `/undo` 在「被编辑的区段里含已登记密钥」时拒绝**（review 把它定性为不变量破坏）。保留 args 打码：不打的话，投影里「他人工具调用只留一行摘要」会把模型塞进参数的密钥摘要给另一个讨论者，JSONL 也留下真值——直接违背本票的 headline「密钥不会顺着事件流、跨 agent 的发言漏出去」。代价按票面要求写进文档，并补两条 e2e 把边界钉死（review 指出原先没有测试碰 `/undo`）：`undo_refuses_when_the_recorded_region_held_the_secret`（拒绝而不是猜，工作区保持编辑后的样子）与 `undo_still_works_when_the_edit_does_not_touch_the_secret`（同一文件里别处的编辑照常可撤销 ⇒ 爆炸半径就是被编辑的那一段）。
4. **spec §20 的「例外走权限规则」并没有实现**（review 没报，自查发现）：门把工作区外的目标当成**任何规则都压不下去的 deny floor**（`permissions::decide`，`tests/permission_gate.rs::the_path_limit_is_a_deny_floor` 钉着），而 `paths.rs` 的错误文案却写着「a permission rule is how you widen this」。本票只把文案改成真话（`no permission rule widens that`），并把这条分歧写进 `paths.rs` 的模块文档与 `docs/credentials.md` 的 Known edges；**没有实现那个例外**——§12 的规则代数忽略具体程度，「任一 allow 规则即可放宽」等于一条宽泛的 `Tool("read_file")` 规则会静默放开工作区外的读，那是需要重新拍板的安全模型改动，不该在凭据票里顺手做。**待定**：要么回改 §20 把 cwd 限制写成硬底，要么单开一张权限模型的票（本机场景下，密钥仍由打码兜住）。
