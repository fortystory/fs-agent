# Crate 与模块布局

Type: grilling
Status: resolved

## Question

决定 `fs-agent` 的 crate 与模块骨架。这是其余所有接缝的落脚点。

**本票已按新目的地改写**（旧图同号票的判据是「极简 v1」）。新增的约束来源见 map 的 Notes。

要回答：

1. **单 crate 还是 Cargo workspace？** 仍是自用单一二进制（外加可能的库入口），workspace 的价值（多产物、编译隔离、多消费者）可能不存在。
2. **顶层模块边界在哪？** 综述隐含的候选是 agent loop / tools / providers / permissions / context / session / cli，但**新目的地要求至少再加三处**：
   - **事件流**（唯一真相源：讨论介质 + hook 总线 + 渲染来源 + 持久化载体，四者共用一个机制）
   - **投影**（`(流, 当前说话者, provider 能力) → messages`）——它住在 provider 适配器侧
   - **讨论/编排层**（轮次推进、分歧检测、合成；以及派发执行者）
3. **谁是承重墙、谁依赖谁？** 需要一条明确的依赖方向，且事件流**不能**依赖 provider（否则 provider 反向依赖流，投影就没地方放）。
4. **二进制入口与库入口是否分离**（`main.rs` + `lib.rs`）？分离让端到端测试能直接驱动循环，不经过 CLI 参数解析——这与旧图切片 9（mock provider + e2e）直接相关。
5. **循环必须可重入**：同一个进程里要能跑第二个独立会话（子 agent / 执行者）。这对模块边界与状态归属有直接影响——状态不能藏在全局单例里。

答案定到：**模块列表 + 每个模块职责一句话 + 依赖方向（谁可以依赖谁）**。

不要定到文件级，也不要定到逐个函数签名（本图整体是边界级，见 map 的 Notes）。

## Answer

**已定（2026-09-12，grilling 与用户逐轮确认）。本票只产决策，不含实现。**

### 1. 打包：单 crate，`main.rs` 薄 + `lib.rs` 公开

- **单 crate**，不用 Cargo workspace。自用单二进制、单一消费者，workspace 的三项价值（多产物、编译隔离、多消费者）一项都不成立，而每个边界一份 `Cargo.toml` + 公开 API 面的成本立刻发生。将来若 repo map（票 12）真的引入 tree-sitter 且编译时间成为问题，再从单 crate 机械拆出 workspace 不迟——反向（workspace 合回单 crate）才是痛的。
- `src/main.rs` 只做「解析 CLI → 调 `lib.rs` 的组装入口」；`lib.rs` 是公开 API。
- **组装入口的 provider 由外部注入**（真实 client 或 mock），不由入口自己 new。这样端到端测试能用 mock provider 直接驱动循环、不经过参数解析（旧图切片 9 的前提）。边界级表述：`lib.rs` 暴露一个 `run` 形入口，`main.rs` 与测试都调它。

### 2. 顶层边界（12 个）

| 边界 | 职责（一句话） | 允许依赖 |
|---|---|---|
| `events` | 只追加事件流：事件枚举 + 日志 + 游标/订阅。**承重墙，零内部依赖** | — |
| `config` | `~/.config/fs-agent/config.toml` 加载 + 模型档案（`base_url`/key/model + 能力覆盖），纯数据 | — |
| `provider` | `Provider` 抽象 + 唯一一个 OpenAI-compatible client + 流式 chunk + 能力表；**投影**是它的 `projection` 子模块 | events, config |
| `tools` | `Tool` 抽象 + 注册表 + 副作用标记 + 内建工具 + 编辑策略分层（票 03/04） | provider, events |
| `permissions` | 权限表达（主体/作用域/动作/传播性）+ 判定与继承 + 权限门（票 05/10/20） | tools, config |
| `hooks` | `PreToolUse` / `PostToolUse` 挂载点与内建消费者；产出「只能收紧」的约束（票 05） | events, permissions |
| `context` | 预算公式 + 工具结果截断 + 超预算丢弃 + 注入点（`AGENTS.md` / skills / repo map）（票 06/09/12） | provider, events |
| `agent` | **可重入**的单 agent Turn：投影 → 调 provider → 过权限门 → 执行工具 → 追加事件；同时负责触发 hook | events, provider, tools, permissions, hooks, context |
| `discussion` | 讨论协议：轮次推进 + 分歧检测 + 合成 + 派发 Executor（票 14/16/17） | events, agent |
| `session` | 事件流落盘 + 会话拓扑（Debater/Executor 的父子关系）+ `--continue`（票 07） | events, config |
| `render` | plain / TUI / headless 三消费者，只读事件流（票 13） | events |
| `cli` | 参数解析 + 唯一的组装者 | 全部 |

### 3. 依赖方向（DAG）

```
events      → （无）
config      → （无）
provider    → events, config
tools       → provider, events
permissions → tools, config
hooks       → events, permissions
context     → provider, events
agent       → events, provider, tools, permissions, hooks, context
discussion  → events, agent
session     → events, config
render      → events
cli         → 全部
```

不变量：

- `events` 与 `config` 零内部依赖；**`events` 绝不依赖 provider / tools**（否则投影无处安放）。
- 依赖只能沿上表向下；**禁止跨层**。要新增一条边，先改这张图。
- 只有 `cli` 依赖全部；任何模块都不依赖 `cli`。
- `discussion` **不直接依赖 provider**——合成是一次普通 agent Turn，分歧检测只读结论。

两处反直觉的边及其理由：

1. **`tools → provider`**：发给模型的工具 schema 是**线级契约**，属于 provider 一侧；`tools` 产出该类型的值。反过来会让 transport 依赖 ACI。
2. **`hooks → permissions`**：把「hook 只能收紧、不能放松权限」（票 05 引 Claude Code 的规则）表达成类型——hook 产出 `permissions` 拥有的约束类型，权限门合并它。于是**权限门不必知道 hook 存在**，两边都不成环。

### 4. 投影与三个新边界的落点

- **投影 = `provider::projection` 子模块**（不是独立顶层边界）。理由：它只由 provider 能力表驱动（连续同 role 合并、`name` 支持），没有独立生命周期。纯函数规则由**票 17** 定。
- **`hooks` 是独立顶层边界**（用户在本票 grilling 中把它从原方案的子模块拆了出来）；v1 的 hook 是内建消费者，挂载点在 agent Turn 内触发。
- 六个 ratified 扩展各有一个落点，都不新增顶层边界：**skills / repo map** 是 `context` 的子模块；**动态工具注册**落在 `tools`（注册表）与 `permissions`（可信度策略）之间（**票 11 已取消后半句**：动态工具的声明里没有副作用类别字段，门的这一侧**没有额外信任策略要落**；落点只有 `tools` 的注册表）；**硬 plan 模式** = `permissions` 的一次档位预设；**TUI** = `render`；**子 agent** = Executor（见下）。

### 5. 状态归属与可重入

- **`Session` 是唯一持有可变状态的值**：`EventLog` + 名册（Debater/Executor）+ 预算 + config。
- **`AgentIdentity`（system prompt / 身份）不进流**，由参与者自己持有（map Notes 已定）。
- **单 agent Turn 是一个值**（借用 `&Session`），不是 `static` / 全局单例；同一进程里能构造第二个——这就是 Executor 的基础。
- **Executor = 带 `parent_id` 的嵌套 `Session`**，持**独立预算**、从父级**预留**（否则一个跑飞的 Executor 吃掉整个会话配额，正是票 14 里 GOOSE `MAX_TURNS` 预算的动机）。回传路径是往父流**追加事件**，不是共享同一个日志对象。
- **`EventLog` 的追加由日志自己加锁**：票 08 已查明 `OpenOptions::append` **不保证**跨线程追加不交错；多个 Executor 并发时这是真实故障点。（**已被票 15 取代**：改为**单一写入者**独占文件句柄，**不需要锁**；此处描述的问题仍然成立，解法以票 15 为准）。

### 6. 词汇

本票确立的领域词汇已落进根目录 [`CONTEXT.md`](../../../CONTEXT.md)（新建，纯术语表）：`Debater` / `Executor` / `Speaker` / `Event` + `EventLog` / `Projection`(`project()`) / `Session` / `Round` / `Turn`。`agent` 保留为泛称（程序名 `fs-agent`、"一个 agent 回合"），**不作类型名**。

### 本票明确不决定的（交接）

- `Provider` / `Tool` trait 的具体形状与分发方式（`async-trait` / `trait-variant` / 泛型单态化 / enum）→ **票 02**（Provider）、**票 03**（Tool）。票 08 已定前提：`async fn` in trait 不 dyn-compatible。
- 事件 schema 与枚举 → **票 15**。
- 投影的纯函数规则（role 合并、`name`）→ **票 17**。
- 权限门与 hook 的控制流次序 → **票 05**。
- 会话拓扑的具体表达 → **票 07**。
- 端到端验收契约（mock provider + 临时 git 仓库）→ `/to-spec`（本图 Out of scope）。

**票 13 交接来的一条边界事实（2026-09-13）**：

- **`render` 与 `events` **没有新增边**（`render → events` 本来就在票 01 的 DAG 里；投递是经**组装期注入的通道**，不是互相调用）——12 个顶层边界之间没有新增边。** 机制是：广播通道由 `cli`（组装边界）在组装期创建，**sender 注入给 `EventLog` 的 writer、receiver 注入给渲染任务**。所以 `events` 不认识 `render`（它只是"追加后发布到一条被注入的通道"），`render` 也不认识 `events` 之外的任何东西。
- 这条对你有用的地方：**"新增一个消费者"不应该在边界图里新增一条边**。将来若要加第二个订阅者（例如一个诊断 tap），形状照旧——组装期多注入一个 receiver 而已。
- 另：**三个渲染模式（plain / TUI / headless）是同一个 trait 的三个实现，启动时选定一个**（互斥，不是并发订阅者）；`render` 边界内部因此是一个 trait + 三个实现，不随模式数增长。
