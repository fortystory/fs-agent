# 工具 trait、注册表、副作用标记与写串行化

Type: grilling
Status: resolved

## Question

决定工具层的接缝。这是整个 ACI（agent-computer interface）的骨架，综述说它是"最影响成败的一环"。

要回答：

1. **副作用标记放在哪。** 综述从五个数据点（Claude Code 明文规则、Gemini 默认并发但强制写串行、Codex 默认串行、Cline 按模型家族开关、Continue 只对只读开放）收敛出一条规则：**按「有无副作用」给工具打标，只读并发、有副作用串行**。这个标记是 trait 的一个方法、一个关联常量、还是注册表里的元数据？
2. **注册表形状**：内建工具如何注册？v1 是编译期静态注册，还是运行期注册表？注意用户已选扩展档位 (iii)——**留挂载点，但不做动态加载**，所以要留的是接缝而不是插件系统。
3. **`read-before-edit` 在哪一层强制。** 综述标 ✅：「一行检查，直接消灭一类『凭想象改文件』的事故」。放工具实现里、注册表里、还是循环里？
4. **同文件写入串行化在哪一层。** 综述：opencode 用按解析后路径为 key 的进程内 `Semaphore`；「做并行工具调用的那一刻就变成必需项」。
5. **并行工具调用 v1 做不做。** 综述的建议是先做串行、真的嫌慢再并行，但「写操作必须串行化」是共识。请明确 v1 的取舍，并说明第 1 条的标记是否为将来的并行留好了位置。
6. **动态注册带来的信任问题（ratify 翻案后新增，交接给票 11）。** 工具面不再编译期封闭（见 map 的 Notes）。于是第 1 条那个「只读」标记成了**不可信输入** —— 动态注册的工具谎报只读，就会让两个写操作并发。**本票要给出内建工具侧的确定性答案**（内建工具的标记是编译期常量、可信），并把"动态工具的标记如何被信任"显式交接给**票 11**。请在本票答案里写下这条交接，别让它悬空。

答案定到 trait 签名与注册表数据结构，不要实现细节。

## Answer

**已定（2026-09-12，grilling 与用户逐轮确认）。证据来源：`docs/research/coding-agent-features.md` 第 1 节（并发五数据点）与第 2 节（ACI），以及票 02 的 `parallel_tool_calls` 事实。**

### 0. 一条决定形状的事实

综述的收敛规则是"按**有无副作用**打标，只读并发、有副作用串行"，但 opencode 的落地是按**解析后的路径**做 `Semaphore`。**二元标记不足以做写串行化**——两个 `edit` 改同一文件要互斥、改不同文件不必。所以标记是"**静态类别 + 本次调用受影响资源**"两段。

### 1. 副作用标记：trait 的必答方法 `effect()`

```rust
pub enum SideEffect {
    ReadOnly,                   // 不改变**工作区**状态 → 可并发（`task` 派发也走这一档：它自己不碰工作区）
    WritePaths(Vec<PathBuf>),   // 只改这些路径 → 按路径互斥
    Exclusive,                  // 全局串行：bash / 未知副作用
}
```

放在 `Tool` trait 上，作为**必答方法**（无默认实现——新工具不声明就编译不过）：

```rust
fn effect(&self, args: &serde_json::Value) -> SideEffect;
```

为什么不是注册表元数据、也不是裸关联常量：

- 受影响路径来自 `args`，**每次调用才知道**，一张静态表装不下；
- `async fn` in trait 不 dyn-compatible（票 08），`dyn Tool` 上关联常量也不可用；
- **内建工具的类别在方法体里就是编译期常量**（只读工具无条件 `return SideEffect::ReadOnly`）——这就是票面第 6 条要的答案：**没有任何运行期输入能让内建只读工具谎报成写、或反过来**。

**交接（票面第 6 条的要求）**：「**动态注册工具的标记如何被信任**」显式交接给 **票 11**。本票只保证内建侧可信；票 11 必须回答动态工具谎报 `ReadOnly` 的对策。（**票 11 取消了这个问题**：动态工具的声明里没有副作用类别字段，"谎报"没有地方可说）

### 2. `Tool` trait 与注册表

```rust
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;                                // name / description / JSON Schema（线级，票 02）
    fn effect(&self, args: &serde_json::Value) -> SideEffect;  // 第 1 节
    async fn call(&self, ctx: &ToolCtx, args: serde_json::Value)
        -> Result<ToolOutput, ToolError>;
}

pub struct ToolRegistry { /* 保序的 name → Box<dyn Tool> */ }
impl ToolRegistry {
    pub fn builtin() -> Self;                        // 内建集合：一个函数，不是全局 static
    pub fn register(&mut self, tool: Box<dyn Tool>); // 票 11 的挂载点
    pub fn specs(&self) -> Vec<ToolSpec>;            // 交给 provider 的 tools 数组
    pub fn get(&self, name: &str) -> Option<&dyn Tool>;
    // dispatch(...)：第 4、5 节的强制点
}
```

- **集合编译期固定、注册表是运行期值**：`builtin()` 是函数，由组装层（`cli`）构造，随 `Session` 走（可重入，票 01）；**不用 `static` 全局**（票 01 已定没有全局单例）。`register` 就是"留挂载点、不做插件系统"的那个挂载点。
- **args 是 `serde_json::Value`（类型擦除）**：`dyn Tool` 放不下关联 `Args` 类型，且票 11 的动态工具（shell 命令 + 参数 schema）本来就没有 Rust 类型。内建工具在 `call` 里解析成自己的强类型参数。
- **分发方式与 `Provider` 一致**（票 02 的跨票建议）：`async-trait` + `Box<dyn Tool>`。
- **本票不决定内建工具有哪些**——那是票 04（编辑策略）、票 12（repo map）、票 14（`task`）汇入的结果。
  - **补充（票 12，2026-09-12）**：`repo_map(focus?)` 是其中之一，**只读**（`effect()` = `ReadOnly`）。

### 3. v1 的并行取舍

**v1 默认串行执行整批 tool call，但调度器此刻就按 `effect` 分流**：`ReadOnly` 走可并行通道，`WritePaths` / `Exclusive` 走串行通道；并行开关默认关、可配开。

- 依据：Gemini 默认并发但**强制 `replace`/`write_file` 串行**；Codex `supports_parallel_tool_calls` 默认 `false`；OpenHands `tool_concurrency_limit = 1`；Continue 只对只读开放；综述的处置是"先做串行，真的嫌慢再并行"。
- **但 `effect` 分流与 per-path 互斥现在就要做对**：DeepSeek 的 `parallel_tool_calls` 是 **Ignored（恒开）**、Kimi 也允许多个 tool call → **模型无论如何都会一次返回多个调用**。"串行"指 harness 按 `index` 顺序执行这一批，不是模型不会批量发。
- 这样将来"并行只读"是**接线**而不是重构。

### 4. `read-before-edit`：工具层 dispatch 集中强制

- **强制点在工具层的 `dispatch`**，不在循环里（循环不该知道"哪些工具算编辑器"），也不在每个工具实现里（新写的工具会忘）。
- **状态归 `Session`**（每个 agent 一份）：Turn 3 读的文件必须对 Turn 5 的编辑可见 → 不是 Turn 局部状态。
- **规则**：`WritePaths(p)` 被拒 ⟺ `p` **已存在** 且不在 read set（新建文件不需要先读）。
- **read set 的唯一写入来源是 `ToolOutput.observed`**，dispatch 统一并入；工具实现不直接改会话状态。
- **补充（票 04 第 4 节）**：编辑匹配**全线失败**时**移除**该路径的 read set 条目（逼模型重读）——细节住在票 04，此处只标注它的存在。
- 拒绝的产物是一条**错误内容的工具结果**（不是中断循环），让模型看到"必须先读"。
- **交接**：Executor 是否继承父级的 read set → **票 14**。
- 可选加强（本票不决定）：read set 存 `path → 内容哈希`（Gemini 用 SHA-256 发现外部修改）。多 agent 共享工作区时它有价值，但读大文件要全量哈希，留作后置。

### 5. 同文件写入串行化

- **锁 key = 规范化后的路径**（绝对化 + 解析 symlink），否则 `./a.rs` 与 `a.rs` 会绕开互斥。
- **锁表是组装期创建的一个 `PathLocks` 句柄，注入给每个 `Session`**。**不是**全局 `static`（票 01），**也不是**每 Session 一份——两个 Executor 是两个 Session 却共享同一工作区，各自一把锁等于没锁。
- `WritePaths(p)` 取这些路径的锁，**多路径按排序后的顺序获取**（避免死锁）；`Exclusive`（如 `bash`）取一把"工作区锁"。
- **次序**：先取锁 → 再跑第 4 节的 read 检查 → 再执行（反了就有 TOCTOU）。
- v1 串行时这些都自动成立，但结构现在就在位。

### 6. `ToolOutput` / `ToolError`

```rust
pub struct ToolOutput {
    pub content: String,            // 回给模型的文本；截断与落盘由票 06 在 context 层做
    pub observed: Vec<PathBuf>,     // 第 4 节 read set 的唯一来源
}

pub enum ToolError {
    NotReadBeforeWrite { path: PathBuf },
    InvalidArgs { detail: String }, // 模型给了错参数
    Io { detail: String },
    // 权限拒绝不在这里：由权限门在工具层之外产生（交接票 05）
}
```

- 签名是 `Result<ToolOutput, ToolError>`；**由循环负责把 `Err` 转成一条错误内容的工具结果**，从而在**一处**保证票 02 §6 的不变量：**每个 `tool_call` 恰好一条结果（成功或失败），在下次调 provider 之前**。
- **本票不在工具层里做权限与 hook**：调用次序 `hook.pre → 权限门 → ToolRegistry::dispatch → hook.post` 由 `agent` 组装（控制流归属是 **票 05** 的题）。工具层只做 Q1–Q5 这四件事。

### 本票明确不决定的（交接汇总）

- **动态工具的 `ReadOnly` 谎报如何防** → **票 11**（本票只保证内建侧可信）。（**票 11 取消了这个问题**：动态工具的声明里没有副作用类别字段，"谎报"没有地方可说）
- **权限拒绝的工具结果由谁产生**、hook 与权限门的次序 → **票 05**。
- 编辑失败后的降级匹配分层 → **票 04**；工具输出截断与落盘 → **票 06**。
- Executor 是否继承父级 read set、权限如何沿委派链继承 → **票 14** / 票 20。
- 内建工具的具体清单、工具描述文案（poka-yoke）→ `/to-spec` 之后。

**票 05 交接来的答案（2026-09-13）**：上面两处交接（第 6 节与「明确不决定」里的"权限拒绝的工具结果由谁产生"）都已关闭。

- **由 `agent` 层的循环合成——不是权限门本身。** 你第 119 行的措辞"由权限门在工具层之外产生"要精确成：**门是纯函数**（`Policy → Verdict`，不能追加事件），**工具根本没被调用**（不可能产出结果），所以合成只可能发生在**循环**里。它在离开权限门时追加 `PermissionDecided { decision: Deny }` + 一条 `ToolCallCompleted { ok: false, error: "权限被拒绝" }`。
- **同一条规则覆盖另外三条路径**：用户拒绝 / hook 跳过（`skip`）/ `hook.pre` 失败——各合成一条结果 ⇒ **"每个 `tool_call` 恰好一条结果"在四条异常路径上都成立**，且与你的 `Result<ToolOutput, ToolError>` → 一条错误结果那条路是**同一个不变量**。
- **你的边界被确认**：`ToolRegistry::dispatch` 仍只管副作用分流、`read-before-edit`、per-path 写互斥；次序 `hook.pre → 权限门 → dispatch → hook.post` 由 `agent` 组装；**hook 与权限门都不是控制流的拥有者**，只是循环按次序调用的纯值变换。
- **`ToolError` 里那个注释可以定稿了**：权限拒绝确实不在 `ToolError` 里，但它的产物**由循环合成**（不是由门产生）。
- 原型（四条异常路径已 headless 验证）：`.scratch/multi-agent-architecture/prototype/05-hook-permission-ordering.html`。

**票 11 交接来的答案（2026-09-13）**：你第 128 行交接的"动态工具的 `ReadOnly` 谎报如何防"已关闭。

- **动态工具的 `effect()` 恒为 `Exclusive`**。理由不是"选择不信任"，而是**没什么可信任**：动态工具的声明语法里**没有副作用类别这个字段**（它声明的是"一条命令 + 参数 schema"），所以"谎报只读"**没有地方可说**——与票 05 的"放松意图在类型里不存在"同一手法。
- 因此你的并行判定**不需要为动态工具写任何特例**：它走 `Exclusive` 这条既有的最保守路径（连同"取工作区锁"）。
- **一条你需要知道的后果**：**`read-before-edit` 覆盖不到动态工具**——你第 4 节的规则是 `WritePaths(p)` 被拒 ⟺ `p` 已存在且不在 read set，而动态工具的 `WritePaths` **未知**（命令是用户声明的，harness 不知道它会改哪些文件）。那道护栏对动态工具**不生效**，约束只能落在权限门上（票 11 第 2 节）。
- 另：动态工具**复用同一套 `ToolSpec`**（你第 2 节已定"内建与动态同一套"），`register(Box<dyn Tool>)` 就是它的挂载点；工具集在**组装期**固定（票 11 第 4 节），不做运行中增删。

**票 14 交接来的三件事（2026-09-13）**：

- **read set 的交接问题有答案了：不继承，且任一方向都不流动。** 嵌套 Session（执行者）的 read set **从空开始**；父也不继承子的。理由：`read-before-edit` 防的是"凭想象改文件"，而"想象"是**每个 agent 各自**的——父读过不等于子见过，子改一个自己没看过的文件正是事故形态。代价是多几次读，但实现上比"合并两个 read set"**更省**。
- **`SideEffect` 需要一个语义澄清**：它判的是"**工作区副作用**"，不是"这次调用是否改变了任何状态"。所以 `task`（派发执行者）**不属于 `WritePaths` / `Exclusive`**——它自己不碰工作区——于是调度器允许同一批里的多个 `task` **并发**。这是"同时派多个执行者"能成立的前提。
- **你那份共享 `PathLocks` 正是并发执行者的写互斥保障**：你写的"两个 Executor 是两个 Session 却共享同一工作区，各自一把锁等于没锁"这句，在票 14 之后有了直接的用户——多个执行者内部的 `edit_file` / `bash` 全靠它串行化。所以 `PathLocks` **不是可选加强，是并发执行的前提**。

**票 19 交接来的一条约束（2026-09-13）**：你那些**诊断产物**（`read-before-edit` 的拒绝、匹配失败导致 read set 失效）**不需要结构化字段，也不会动事件 schema**。

- 它们已经以**错误内容的工具结果**落在流上（你第 4、6 节就是这么定的），票 19 只要求它们**可机器读**。
- **唯一要求**：渲染那段文本的地方（工具层 / 循环）与解析它的地方（查询层）**必须共用一个格式常量**，否则它会随重构静默漂移——而漂移之后，"模型在凭想象改文件吗"这个查询会悄悄开始返回 0。
- 一条判据（票 19 留的）：如果某个诊断量将来从"**诊断**"升级为"**权威**"（有东西按它做决定，而不只是给人看），那时才该给它结构化的位置。

**票 21 交接来的一道检查（2026-09-13）**：**文件工具的"模型供路径"要限定在会话 cwd 及其子树**。

- 这是 `read_file` / `write_file` / `edit_file` 的 **dispatch 层**要加的一道检查——与你已有的 `read-before-edit`、per-path 锁在**同一个地方**（都是"模型给的路径"要过的关）。
- **例外走权限规则**（票 20 的规则语言），不要硬编码在工具里。
- **它保护的东西很具体**：**agent 自己的 API 密钥就在 `~/.config/fs-agent/config.toml` 里**（仓库外）——cwd 限制让文件工具够不到它。这是票 21 里 ROI 最高的一条。
- **限制的是"模型给的文件路径"，不是 harness 自己读的东西**：`skill(name)` 读 `~/.claude/skills/`（票 09）、会话目录的写入（票 07）都是 harness 内部行为，**不受影响**。
