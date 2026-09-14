# 08: skills

**What to build:** skill 作为「渐进披露的指令包」可用——描述清单每轮都在场（便宜），全文按需用一个工具取（贵，但只在真需要时付）。

Blocked by: 07

Status: done

**参考:** spec §9（skills 与 repo map）、§7（工具）

- [x] 全文加载靠一个内建**只读**工具 `skill(name)`；产物是流上一条工具结果（单一收口：可记账、可计量、可被权限语言覆盖），**追加在尾部**所以不破坏前缀缓存
- [x] 发现路径：**项目 + 用户两级、各三处**（`.fs-agent` > `.agents` > `.claude`，项目 > 用户）
- [x] `disable-model-invocation` 的 skill **既不进清单也拒绝加载**
- [x] 预算：**单 skill 5k / 已加载总 25k / 描述清单独立封顶 3k**
- [x] 已加载的 skill 全文在丢弃序列里**比普通工具结果更黏**（插入「老 skill body」一类）
- [x] 「已调用的 skill 集合」**可重算**，不新增状态（将来重注入不返工）
- [x] 边界规则写进文档：>80% 每轮都要 → `AGENTS.md`；按任务 / 长 → skill；必须无条件执行 → hook
- [x] v1 的 skills **只装指令**（不把工具打包进 skill——那会与「工具表组装期固定」冲突）

## Comments

实现落点：`src/context/skills.rs`（新：`Skills` / `Skill` / `discover` / `parse` / `catalog` / `load` / `loaded_skill_names`）、`src/context.rs`（`TrimPolicy::loaded_skill_budget` + `trim` 的聚合上限前置 pass）、`src/tools/skill.rs`（新：`SkillTool`）、`src/tools/{mod,tool,registry}.rs`（`builtin()` 挂 `skill`；`Skills` 经 `ToolContext` / `PendingCall` 下发）、`src/session.rs`（`skills` 值）、`src/lib.rs`（组装期发现 + 记一条 `SkillsCatalog` 注入）。文档：`docs/skills.md`（边界规则 + 发现路径 + 预算）。测试：`tests/skills.rs` 18 例（纯函数 14 + e2e 4）。

实现期把票面留白写实的几处（都不改 spec 的决定）：

1. **清单与 `AGENTS.md` 是两条 `ContextInjected` 事件，但投影合并成同一条钉住的 `user` 消息**。流上仍按 `source` 各记一条（`AgentsMd` / `SkillsCatalog`，spec §2 的枚举因此保留），`project` 把**开头连续的**注入拼进同一条消息（spec §10、决议票 09「与 `AGENTS.md` 同一条」），所以线级不会出现连续两条同角色消息（§5 合并规则的初衷）。中途注入（票 15 的计划指令）前面已有历史，因此自成一 `user` 消息；`trim` 的钉住判据「开头连续的 `User{name:None}`」不变（现在通常只有 1 条）。**第一版实现曾让两条注入各成一消息**，第二轮两轴复查指出它与决议票 09 冲突，已按决议改为合并、并把 spec §10 回改成「合并注入为第一条 `user` 消息」。
2. **「已加载总 25k」是整条请求的真实总量（含当前回合）**，落在 `trim` 的窗口 fit 检查之前：总量超 25k 就把最老的 skill body stub 掉，即使窗口还有富余；**普通工具结果一概不碰**。窗口驱动的丢弃顺序照旧（老普通结果 → 老 skill body → 老整轮），两条合起来正是决策票那句「老 skill body（超出 25k 的最老先丢）」。当前回合自己加载超预算时，该回合最老的 body 会被 stub（模型可以再 load 一次）——窗口的「当前回合永不丢」保护的是问题与回答，不是这条独立的 skills 预算（评审收口：只扫 `pinned..active_start` 时一回合能超 25k，不是真总量）。判定复用 `TrimPolicy.sticky_tool_names`，与票 07 的黏性类别共用一个判据。
3. **frontmatter 用极小标量解析器，不引 YAML 依赖**：只认 `name` / `description` / `disable-model-invocation`，值支持裸串、单引号与双引号（双引号内 `\"` / `\\` 反转义，实测本机 38 个 skill 全覆盖）。**缺 `description` 的 `SKILL.md` 不算 skill**——清单就是靠 description 构成的。
4. **单 skill 超 5k 是截断不是拒绝**：截到 5k 估算 token 并附「全文在 <SKILL.md 路径>（工作区允许时用 read_file 读）」的指针（决策票第 64 行口径）。项目级 skill 的路径在 cwd 内，`read_file` 够得着；**用户级 skill 在 cwd 外，`read_file` 受 cwd 限制够不着**——这是决策票没覆盖的一处限制，指针的措辞已改成不承诺「一定可读」。缓解：本机实测最大的 skill ~3k token（决策票同口径），5k 是安全阀、罕见路径。
5. **`skill` 工具是无状态的，库经 `ToolContext` 下发**（与 `read_paths` / locks 同一形状：`PendingCall.skills: Arc<Skills>`）。这样 `tools::builtin()` 仍返回完整的内建集合、工具表组装期固定，而库随会话的 cwd 走。它 `effect()` = `ReadOnly`、**不声明任何 read path**：读的是 harness 自己发现好的库，不是模型给的路径，所以 cwd 限制不适用（spec §20），readonly / plan 模式下也能用。工具名只在 `context::skills::SKILL_TOOL` 写一次（工具声明、黏性类默认值、`loaded_skill_names` 共用），不散落字面量。
6. **发现顺序**：项目 `.fs-agent` → `.agents` → `.claude`，然后用户 `~/.config/fs-agent` → `~/.agents` → `~/.claude`；同名**先到者胜**（项目 > 用户）。每个 root 内的目录按名排序，保证清单文本确定。实测本机：发现 38 个 skill、其中 17 个可被模型调用，清单 ~977 估算 token（< 3k）。
7. **「已调用集合」= `loaded_skill_names(&[Event])`**：扫 `ToolCallStarted{tool_name=="skill"}` 与其 `ToolCallCompleted{ok:true}` 配对，按首次加载序返回去重名。纯查询、零状态，compaction 后要重注入直接站它上面。
8. **交接票 12（会话恢复）**：`--continue` 复用既有日志时**不要再记 `SkillsCatalog` 注入**（同票 07 评论第 10 条对 `AgentsMd` 的规矩）。库本身在组装期重新发现即可，清单内容由 cwd 决定，**不随会话变化**。
9. **交接票 09（repo map）**：`skill` 的形状可直接照搬——`ReadOnly`、无 read path、结果走同一套截断与丢弃；`repo_map` 不需要会话里的库，比它更简单。
10. **第一轮两轴评审收口**：Spec 轴指出「已加载总 25k」原来只扫非当前回合（一回合内可超）、用户级 skill 的截断指针够不着、清单是第二条钉住消息；Standards 轴指出工具名字面量散落（收进 `SKILL_TOOL`）、`old_result_indices` 与聚合 pass 重复候选扫描（收进 `live_tool_indices` / `active_round_start`）、`is_skill_body` 名不副实（改 `is_sticky_result`）、`Skills::len` 无调用者（删）。
11. **第二轮两轴复查（固定点 `196c7e8`，含上一条修复）**：25k 真总量与截断措辞判定已解决；Spec 轴判定第一轮那次 §10 改写「削弱了决定」——决议票 09 明确清单与 `AGENTS.md` **同一条** message，于是改为**投影合并开头连续的注入**、§10 回改成「合并注入为第一条 `user` 消息」并写明两条事件各带 `source`。Standards 轴：无硬性违规。**遗留判断项**（未改，留待需要时）：`live_tool_indices` 之上的 sticky 过滤仍是两处各写一次；`"name"` 参数键在 `tools/skill.rs` 与 `loaded_skill_names` 各写一次；`names` / `is_empty` / `get` 与 `Skill` 的 pub 字段偏测试面；CONTEXT.md 缺「技能（Skill）」词条（按 `docs/agents/domain.md` 记给 `/domain-modeling`，且该文件当前未纳入 git）。

