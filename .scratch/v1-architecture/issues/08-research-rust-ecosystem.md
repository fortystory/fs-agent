# research：Rust 生态的 crate 选择与当前惯用法

Type: research
Status: resolved

## Question

本票为架构票提供**外部事实**。它是 AFK 的：由子代理读文档与 crate 元数据，产出带来源链接的简报，**不给方案**。

需要查明（判据：只报告事实与来源，不下结论）：

1. **流式 HTTP + SSE 客户端。** `reqwest` 手写 SSE 解析 vs 现成 crate（`async-openai` / `genai` / `rig` / 其他）：
   - 各自是否支持**流式 tool_call 增量的逐块解析**（这是综述切片 1 的核心接口）；
   - 依赖树重量、维护状态、最近一次发布；
   - 是否强制绑定某家 provider 的协议（综述要求"只要一个 OpenAI-compatible client，`base_url` 可配"）。
2. **`async fn` in trait 的当前状态。** 在目标 Rust 版本下是否稳定可用；有无 object safety 限制（这直接决定 `Provider` / `Tool` trait 能不能做 `dyn`）；若不能，生态里的惯用替代是什么（`async-trait` crate？泛型单态化？枚举分发？）。
3. **事件挂载点的惯用表达。** Rust 生态里 agent / hook / 中间件类项目怎么表达"工具调用前后各一个挂载点"：typed enum、trait object、还是 channel（`tokio::sync::mpsc`）。给出 2~3 个真实项目的做法与来源。
4. **几个小件的当前惯用法**：CLI 参数解析、错误处理（`anyhow` vs `thiserror` 的分工惯例）、JSONL 追加写的做法。
5. **并发原语**：按路径 key 的串行化在 tokio 里的惯用做法（Semaphore / `Mutex` map / actor），以及需要注意的坑。

**产物**：一份带来源链接的中文简报，写到 `.scratch/v1-architecture/research/08-rust-ecosystem.md`，并在本票留下指针。

**约定**：wayfinder 原本要求研究产物落在 `research/<name>` 一次性分支上；本仓库的 tracker 是本地 markdown，所以改用文件形式，不建分支。

## Answer

**已解决**（2026-09-12，research 子代理）。产物：`.scratch/v1-architecture/research/08-rust-ecosystem.md` —— 399 行，按本票 5 个编号问题组织，93 条一手来源链接（13 个唯一域名），未能核实处集中在附录 B（10 处标注 UNVERIFIED）。主代理已抽查核对该文件真实存在、结构与引用齐全。

**必须传给架构票的关键事实**（细节与来源在产物文件里，此处只记指针级摘要）：

1. **`async fn` in trait 自 Rust 1.75 稳定，但不是 dyn-compatible**（Rust Reference 明确）。直接后果：**`Provider` / `Tool` 不能直接写成 `dyn Trait`**（票 02、03 必须正面回答）。替代路线：`async-trait` 0.1.92、`trait-variant` 0.1.3、`dynosaur` 0.3.1，或泛型单态化 / 枚举分发。官方 2026 项目目标「Native async fn dynamic dispatch」状态 Accepted，先走 nightly。
2. **流式 tool_call 增量有现成实现可参照**：`async-openai` 0.42.0（`FunctionCallStream.arguments: Option<String>` 片段，且 `with_api_base` 可配）、`genai`（`ChatStreamEvent::ToolCallChunk`）、`rig` 0.42.0（`StreamedAssistantContent::ToolCallDelta`）。纯传输层 `reqwest` 0.13.5 的 `bytes_stream()` 需 `stream` feature。**传递依赖实测**（默认特性，2026-09-12）：reqwest 103 / async-openai 95 / genai 148 / rig-core 149；专用 SSE parser（eventsource-stream）只有 6。
3. **hook 挂载点的惯用表达是「trait + typed enum 返回值」，不是 channel。** 调研在 Rust agent 生态里**未发现用 mpsc 做 hook 的例子**。最贴近的先例：`rig-agent` 0.42.0 的 `AgentHook` —— `on_tool_call -> ToolCallAction`、`on_tool_result -> ToolResultAction`。**票 05 的原型应先读这一条。**
4. **JSONL 无标准库或 serde 官方 API**；`std` 的 `OpenOptions::append` 文档明确**不保证**多进程/多线程追加不交错、单次 write 可能部分写入。→ 影响票 07。
5. **per-path 串行化的真实先例**：`HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>`；`Semaphore` 公平但取消会丢排队位置；`dashmap` 每个方法都带死锁警告；`async-lock` **不存在** KeyedMutex（原题设不成立）。→ 影响票 03。

**时点提醒**：调研日 2026-09-12，Rust stable 已是 **1.98.1**，而本机 `rustc` 是 **1.94.0**。票面原写的「2025 末/2026 初」已过时。
