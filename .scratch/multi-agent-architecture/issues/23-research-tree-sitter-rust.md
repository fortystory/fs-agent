# research：tree-sitter 的 Rust 绑定与符号抽取 / 图排序的实现代价

Type: research
Status: resolved

## Question

本票为**票 12（repo map / 符号检索）**提供外部事实。它是 AFK 的：由子代理读一手来源，产出带来源链接的简报，**不给方案、不下结论**。

**为什么需要它**：`docs/research/coding-agent-features.md` 把 repo map 描述成"需要 tree-sitter + 图排序的**独立工程**"，而票 08 的 Rust 生态调研产物（`.scratch/v1-architecture/research/08-rust-ecosystem.md`，399 行）经核实**完全没有覆盖 tree-sitter**——对全文检索 `tree-sitter` / `图排序` / `符号` / `repo map` 均为 **0 命中**。所以"这件事在 Rust 里到底多贵"目前没有事实底座，而票 12 的取舍完全压在这上面。

判据：**只报告事实与来源，不推荐方案、不选赢家**。凡一手来源未写明者标 ⚪ 未证实。

需要查明：

1. **tree-sitter 的 Rust 绑定现状**（crates.io API 元数据 + docs.rs + 官方仓库）：
   - `tree-sitter` crate：最新版本与发布日期、`rust-version`（MSRV）、normal 依赖闭包大小（照票 08 的方法：`cargo tree -e normal -q --prefix none`，去重、排除 root 与 dev/build 边）、仓库最后提交时间。
   - `Parser` / `Language` / `Query` 的当前 API 形状：要从源码里取"函数/类/方法签名"该用什么（`Query` + S-expression、还是 `node.kind()` 遍历）？tree-sitter 官方的 **`tags.scm`** 约定是什么、有没有一手文档？
   - **增量解析**：`Tree::edit` + 重新 parse 的一手文档与语义。
2. **作为对照的非 tree-sitter 方案**（只报事实，不比较优劣）：`syn` crate（版本/维护/能解析到什么粒度、是否只能解析合法 Rust 源）、`rust-analyzer` 的索引/SCIP 输出、`universal-ctags`。各自的一手现状与已知限制。
3. **grammar crate**：`tree-sitter-rust` 等 grammar crate 的最新版本与日期、维护状态；**有没有一手数据点说明 grammar 的编译时间或产物体积**（仓库 CI、文档或 issue 里的实测数字）。
4. **图排序要不要自己实现**：
   - aider 的实现事实（仓库源码）：算法名、是否 PageRank 变体、输入是什么（文件节点 + 依赖边？）、是个人排序还是整图迭代；`--map-tokens` 的默认值与"没加文件时扩张到几乎整仓"的**原文出处**。
   - Rust 侧有没有现成构件：`petgraph` 的最新版本/维护状态、它是否提供"带权图上的重要性/中心性排序"（若有一手文档，给出具体 API 名）。
5. **整仓解析的代价**：有没有关于"解析一个中小型 Rust 仓库需要多久 / 多少内存"的一手数据点（项目文档、基准、issue）。若找不到，显式标 ⚪ 未证实，**不要推断**。

产出：`.scratch/multi-agent-architecture/research/04-tree-sitter-and-symbol-extraction.md`（带完整来源清单与逐条引用；英文来源保留原文措辞）。

**本票不决定任何事**——它是票 12 的事实输入。票 12 的答案里只放指向它的链接。

## Answer

**已解（2026-09-12，AFK，由 research 子代理执行）。**

**产物**：`.scratch/multi-agent-architecture/research/04-tree-sitter-and-symbol-extraction.md`（593 行：来源 ID 表 + 逐条引用 + 第 6 节 ⚪ 缺口清单 + 完整 URL 清单）。仓库内只新增该文件，票 / map / 既有简报未动。

对票 12 最相关的几条（细节与来源见简报）：

- **代价低**：`tree-sitter 0.27.0`（2026-08-30）+ `tree-sitter-rust 0.24.2` 合并依赖闭包 **9 包**（grammar 只依赖 `tree-sitter-language`，不依赖运行时）。本机实测 237 文件 / 3.97MB / 120k 行 → 解析 395–466ms、tags 查询 226–228ms、峰值内存 ≈133MiB（最坏情况：保留全部 Tree）。官方单文件数据点：2157 行 **6.48ms**；增量更新 **<1ms**。
- **版本兼容已核实**：grammar 生成 `LANGUAGE_VERSION 15`，运行时 ≥0.25 支持 ABI 13–15 → 0.27.0 可加载（两张一手表的交集）。注意 **0.27.0 的 MSRV 跳到 1.90 / edition 2024**（0.26.13 仍是 1.77），本机 `rustc 1.94.0` 可用。
- **`tags.scm` 只有名字**：官方约定 `@role.kind` + 必有 `@name`、可选 `@doc`，位置约定 `queries/tags.scm`；Rust 版抽 struct/enum/union/type→class、fn/method/trait/mod/macro、call/impl→reference——**没有签名捕获、没有作用域捕获**。
- **图排序没有现成构件**：`petgraph 0.8.3` 的 `algo::page_rank(graph, damping_factor, nb_iter)` **无 edge weight、无 personalization**，`algo` 模块也没有其他中心性函数；aider 用的是带权（边权 ∝ √引用数）+ personalization（chat 文件 50×）的**整图** PageRank，再分派到 (文件, 标识符) 并对 token 预算做二分搜索。
- **aider 的 `--map-tokens`**：官方**文档**说默认 1k；**源码**未传参时 `clamp(max_input/8, 1024, 4096)`；`map_mul_no_files=8`，无 chat 文件时 `target = min(8×, 上下文窗口 − 4096)`。
- ⚪ **缺口**：一手来源里没有整仓解析时间/内存数字、没有 grammar 编译时间数字、没有 `--features wasm` 闭包、`syn` 的错误恢复与名字解析未声明、ctags 的 Rust 引用能力无一手声明——简报第 6 节逐条列了 14 条。上述"整仓"量级由子代理在 `/tmp` 本地实测补齐并**明确标注为非一手来源**。
