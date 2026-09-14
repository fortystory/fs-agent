# 09: repo map

**What to build:** 模型能按需取一份仓库符号地图帮忙定位——成本可预期（固定预算）、结果可解释（朴素排序），且**不注入**（所以不会把历史反复挤出缓存前缀）。

Blocked by: 07

Status: done

**参考:** spec §9（skills 与 repo map）

- [x] 内建**只读**工具 `repo_map(focus?)`，与 `skill(name)` 同形：产物是工具结果、`effect()` = `ReadOnly`、落进同一套截断与丢弃；**不注入**
- [x] **固定预算 1k、可配上限 4k**；**不接受**模型传 `tokens`
- [x] 抽取 = tree-sitter + 官方 `tags.scm`（**只有名字**，v1 不做签名渲染；增量解析后置）
- [x] 排序 = **朴素可解释的纯函数**（会话相关度为主 + 结构信号为辅），**不做**整图 PageRank；`rank()` 是可替换的纯函数接缝
- [x] 大仓库下取一次 map 的耗时与产物体量在预算内（记下实测数字，作为回归基线）

## Comments

实现落点：`src/context/repo_map.rs`（新：`RepoMap` / `extract` / `rank` / `render` / `RankContext` / `RepoMapInput` / `SymbolKind` / `Definition` / `Scored`，含 mtime 缓存与 `parses()` 计数）、`src/tools/repo_map.rs`（新：`RepoMapTool`，薄壳）、`src/tools/{mod,tool,registry}.rs`（`builtin()` 挂 `repo_map`；`ToolContext`/`PendingCall` 增 `repo_map: RepoMapInput`）、`src/config.rs`（`SessionConfig::repo_map_tokens` 默认 1k、`MAX_REPO_MAP_TOKENS = 4_096`、`with_repo_map_tokens` 钳制）、`src/agent.rs`（仅当 `tool_name == repo_map` 时从事件流重算 `RankContext`）。文档：`docs/repo-map.md`（边界规则 + 预算 + 排序 + 实测基线）+ `CONTEXT.md` 词条「仓库地图（RepoMap）」。测试：`tests/repo_map.rs` 16 例（纯函数 + 集成）+ 1 例 `#[ignore]` 计时基线。

实现期把票面留白写实的几处（都不改 spec 的决定）：

1. **公共 API 不暴露 tree-sitter 类型**。`extract(text)` 是便利入口，自建 `Parser`+`Query`；`RepoMap` 在组装时编译一次官方 `TAGS_QUERY`（`tree_sitter_rust::TAGS_QUERY`，直接 `include_str!` 官方 `queries/tags.scm`）。版本事实照研究票：`tree-sitter 0.27.0` + `tree-sitter-rust 0.24.2`（ABI 15 ∈ 0.27 支持的 13–15）。迭代 Query 匹配用 `tree_sitter::StreamingIterator`（re-export），**不另引** `streaming-iterator` 依赖。
2. **`tags.scm` 的一个官方行为被照单全收**：`impl` / `trait` / `mod` 体内的 `function_item` 全走 `(declaration_list …) @definition.method`，所以 `mod` 里的顶层 `fn` 也判为 method——map 跟随官方查询、不二次猜测。去重**只折掉同名的「method 阴影」**：同一节点既命中 `@definition.method` 又命中 `@definition.function` 时，保留 Method、丢掉那份 Function；两个 `impl` 各自定义的 `new`（同为 Method）**不折**，`definitions` 结构信号才不欠账（两轴复查收口）。
3. **「会话相关度」是纯查询不是状态**：`RankContext::from_session(events, cwd)` 重算（同 `loaded_skill_names` 的形状）。路径只取 `read_file` / `write_file` / `edit_file` 三工具的 `file_path` 参数、做 `./` / `..` 词法归一、留最近 16 条；标识符取最近 6 条 `MessageCompleted`、小写、`len ≥ 3`、去重、上限 128。为省一次 O(事件流) 扫描，只在确实要跑 `repo_map` 的那次调用里算。
4. **排序 = 元组比较，不引 petgraph**：`(relevance.score desc, references desc, definitions desc, name asc, file asc)`，其中 `relevance.score = focus_matches*4 + recent_path*2 + recent_identifier`。专注度 > 近期路径 > 近期标识符 > 结构信号，逐项可解释；`rank()` 是 spec 里点名的直接测纯函数接缝。
5. **渲染 = `<相对路径>: name, name` 按文件分组、只整符号截断**，且每放进一个符号都先确认「正文 + 省略注记」仍在预算内——注记是模型唯一的「图不完整」信号，宁可少打一个符号也要保住它（截断的 map 永远以注记收尾）。空仓库 / 查询失败返回空串，工具翻成「no Rust symbols found under <cwd>」，**绝不失败该回合**。
6. **`tokens` 参数被无视**（schema 无此键，模型硬传也被忽略），预算在 `with_repo_map_tokens` 与 `RepoMap::build` 两处钳到 4k，绕过 config 也上不去（有测试）。**「可配」= `SessionConfig` 这条接缝**（同票 07 的 `max_tool_result_tokens`：builder 可设、默认 1k、钳 4k）；本票**不**新开 config.toml / env / CLI 面——那是 `SessionConfig` 整体的组装问题，不属于 repo map 一票。
7. **mtime 缓存**：key = `path + modified + len`；`parses()` 计数器供测试证明命中。改长 / 改名（len 变）也能失效，不单靠 mtime。**缓存是决议票定的**（`.scratch/multi-agent-architecture/issues/12` 的 Answer §2：「重复调用便宜靠工具内部的 mtime 缓存」，aider 同思路），不是本票的 scope creep；「增量解析」才后置（spec §9）。
8. **实测基线（记进 `docs/repo-map.md`，可复现）**：合成 400 文件 / 204 800 字节 → 冷 **92.8 ms**（400 次解析）、热（全缓存）**19.0 ms**、输出 4 093 字符 ≈ **1 024 估 token**；另有 600 文件用例证明产物体量钳在 4k。研究票的真实 crate 数字（237 文件 / 3.97 MB → 解析 395–466 ms）说明本实现不保留语法树、成本以「解析改动过的文件」为主，量级一致。
9. **交接**：既有 e2e `a_tool_call_with_no_arguments_records_an_empty_object` 原本借 `repo_map` 当「未注册工具名」，注册后它在空 workspace 上真跑出「no Rust symbols found」；该断言只查 args，不受影响。`bash` / `task` 未做；`rank()` 将来换带权 PageRank 是局部替换不是重构。
