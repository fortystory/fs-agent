# 08: skills

**What to build:** skill 作为「渐进披露的指令包」可用——描述清单每轮都在场（便宜），全文按需用一个工具取（贵，但只在真需要时付）。

Blocked by: 07

Status: ready-for-agent

**参考:** spec §9（skills 与 repo map）、§7（工具）

- [ ] 全文加载靠一个内建**只读**工具 `skill(name)`；产物是流上一条工具结果（单一收口：可记账、可计量、可被权限语言覆盖），**追加在尾部**所以不破坏前缀缓存
- [ ] 发现路径：**项目 + 用户两级、各三处**（`.fs-agent` > `.agents` > `.claude`，项目 > 用户）
- [ ] `disable-model-invocation` 的 skill **既不进清单也拒绝加载**
- [ ] 预算：**单 skill 5k / 已加载总 25k / 描述清单独立封顶 3k**
- [ ] 已加载的 skill 全文在丢弃序列里**比普通工具结果更黏**（插入「老 skill body」一类）
- [ ] 「已调用的 skill 集合」**可重算**，不新增状态（将来重注入不返工）
- [ ] 边界规则写进文档：>80% 每轮都要 → `AGENTS.md`；按任务 / 长 → skill；必须无条件执行 → hook
- [ ] v1 的 skills **只装指令**（不把工具打包进 skill——那会与「工具表组装期固定」冲突）
