# 编辑策略的可插拔点与降级匹配分层

Type: grilling
Status: resolved
Blocked by: 03

## Question

决定 `edit_file` 内部策略的接缝。综述说编辑是"整条链路失败率最高的地方"——模型生成的 `old_string` 少一个空格就整块失败；而**真正的差异化在「编辑失败之后」**（opencode、cline、gemini-cli 三家各自独立收敛到同一类自愈层）。

要回答：

1. **降级匹配怎么分层。** 综述建议 v1 只做 **2 层**：**每行 trim 后匹配**、**忽略行尾空白**，理由是"就能吃掉大部分无谓失败"。这两层放在哪个接缝后面，才能将来加到更多层（opencode 有 10 层）而**不改工具接口**？
2. **护栏是否进 v1**：
   - 拒绝"省略占位符"的写入（Gemini 的 `write_file` 会因 `rest of methods ...` 直接报 `Provide complete file content.`）——综述标 🔷，「一行正则检查，直接消灭一整类事故」。
   - 防"匹配过大"的拒绝（opencode 的 `isDisproportionateMatch`：匹配到的片段远大于 `old_string` 时拒绝应用）。
3. **编辑格式是否留成可配置项。** 综述有一个尖锐观察：**编辑格式不是「哪个更好」，而是「哪个模型更能写对」**——opencode 按 model id 二选一（命中 `gpt-` 用 `apply_patch` 替换掉 `edit`/`write`），aider 有 `--edit-format`。v1 是固定为精确 search-replace，还是在第 1 条那个接缝上留出按模型选择的位置？
4. 明确**不做**：unified diff 解析、AST / tree-sitter 编辑（综述：10 个实现里零个把它当编辑路径）。

答案定到分层结构与每层的接口，不要实现。

## Answer

**已定（2026-09-12，grilling 与用户逐轮确认）。证据来源：`docs/research/coding-agent-features.md` 第 2 节；接口受票 03 的 `Tool` trait 约束。**

### 0. 落点与线级契约

- `edit_file` / `write_file` 都住在 **`tools` 边界内部**（票 01 的 12 个顶层边界里没有单独的 edit 边界）。依赖方向不变：`tools → provider, events`（票 01）。
- **线级契约（模型看到的那份，必须稳定）**：
  - `edit_file(file_path, old_string, new_string, replace_all?)`——`file_path` 要求**绝对路径**（综述点名：Claude Code 把相对路径强制成绝对路径后，模型的路径错误消失）；`old_string` **必须唯一且逐字符匹配**，无正则、无模糊匹配（这是**契约**；模糊只发生在下述降级梯里，对模型不可见）。
  - `write_file(file_path, content)`——整文件覆写，走票 03 第 4 节的 read-before-write。
- 描述文案本身当 prompt 工程做（poka-yoke）→ `/to-spec` 之后。

### 1. 匹配梯（票面第 1 条）

```rust
pub enum MatchOutcome { None, Unique(Range<usize>), Ambiguous { count: usize } }

pub trait Matcher: Send + Sync {
    fn name(&self) -> &'static str;                        // 级名，进工具结果 → 可观测
    fn find(&self, haystack: &str, needle: &str) -> MatchOutcome;
}

pub struct MatchLadder(Vec<Box<dyn Matcher>>);             // 顺序 = 优先级，first-success-wins
```

- **缝在 `tools` 模块内部**（`tools::edit`，模块私有 / `pub(crate)`），**不是新的顶层边界**。
- **票 03 已经把 `Tool::call(ctx, serde_json::Value)` 定成 JSON 泛型接口**，所以"将来加层会改到工具接口"这件事已经被免费排除。真正不能动的是第 0 节的**线级契约**。这道缝存在的意义是让"层"成为**一处可枚举、可命名、可观测**的东西，而不是散在 `edit_file` 里的顺序 `if`。
- **v1 三层**（含精确基线）：`exact` → **行尾空白无关** → **每行 trim 后匹配**（忽略行首缩进差异）。顺序 = **最小破坏性优先**。
- **每级必须报出自己是否命中**（级名进工具结果文本）。否则"降级成功"是静默的：票 19 查不到、模型也不懂这次为什么成了。三家自愈层都缺这一环，自研最容易省。
- **全线失败时的错误文本必须可行动**：说明已试过哪些归一化，并要求**重新 `read_file` 后再改**（与第 4 节联动）。

### 2. 三样护栏都进 v1（票面第 2 条）

- **唯一性（强制）**：精确匹配必须唯一；多命中 → 拒绝，要求模型扩大上下文或显式 `replace_all=true`。
- **防"匹配过大"**：opencode 的 `isDisproportionateMatch`——只在**降级级**可能触发（模糊匹配会过度吞并）；一个可调比例就能拦住"一次 edit 吃掉半个文件"。
- **拒绝省略占位符**：判据必须是**注释形状的短语**（`// rest of methods`、`# ... existing code ...` 这类），**不能用裸 `...`**——Rust 的 `..`（struct update）、`..=`（range）、JS 的 spread 全会误伤。作用面是**所有写盘内容**：`write_file` 的 `content` 与 `edit_file` 的 `new_string`。
- 三者的错误文本都必须**可行动**（这是 ACI 的原则，不是措辞问题）。

### 3. 编辑格式：v1 固定，不加预留字段（票面第 3 条）

- **v1 固定为精确 search-replace + 第 1 节的匹配梯**；**不**给能力表加 `edit_format` 之类现在只有一种取值的字段。
- 依据与票 02 同一条原则：**前向兼容靠机制，不靠预留字段**。
- **换格式的挂点已经存在**：注册表是**组装期构造的运行期值**（票 03），组装层知道 model profile——opencode 做的正是"在 registry 层按 model id 换掉 `edit`/`write` 工具集"。
- 将来真要接入"写 patch 更准"的模型（Codex 的 `apply_patch` 是**训进 GPT 系模型**的自由格式 DSL，而我们手上的 Kimi / DeepSeek 不是这类模型），改动是**两处局部**：`builtin()` 的构造处 + 能力表加一格。无接口变化、无迁移。
- **现在要保护的只有两件事**：`edit_file` 的线级契约稳定；**循环里禁止按工具名硬编码分支**——票 03 的 `effect()` / `WritePaths` 已经保证循环不需要知道"哪个工具是编辑器"。

### 4. 匹配全线失败即失效 read set（票 03 / 票 04 的交界）

- 本票对**票 03 第 4 节**的补充：read set 的唯一**写入**来源仍是 `ToolOutput.observed`；此处新增一个**移除**来源——**匹配全线失败（三层都没命中）时移除该路径的 read set 条目**，逼模型重新 `read_file` 才能再写。
- 理由：**读 → 写之间的间隙没有锁**（per-path 锁只在真正执行写的那一刻取）。全线失败只有两种成因：文件在读过之后被改过（别的 agent / 执行者动的手），或模型凭空编造了 `old_string`。两种都意味着"手上的内容已不可信"，而这是**不加内容哈希（票 03 明确后置）也能拿到同等安全性**的唯一地方，代价只是多一次重读。
- 匹配**成功**则不动 read set。

### 5. 明确不做

- **unified diff 解析**——证据型砍掉项（综述：全部调研对象里只有 aider 在用，且是为迁就特定模型的怪癖）。
- **AST / tree-sitter 编辑**——零个代表性实现把它当编辑路径。
- **多文件补丁 / `apply_patch` DSL**——v1 不做；将来若某模型需要，走第 3 节的挂点。
- **编辑后自动 lint/test 回灌**——那是 `PostToolUse` hook 的范畴（**票 05**），不是编辑工具内部的策略；综述把它标为 ROI 很高，别在这里实现。

**票 05 / 票 07 交接来的两件事（2026-09-13）**：

- **票 05**：`PostToolUse` 回灌已定为**一条追加的 `HookExecuted` 事件**（**不得改写工具结果**——事件永不修改），由投影合并进那一条 tool 消息。所以"编辑工具内部自己做回灌"这条路彻底关闭，与你的第 78 行一致。
- **票 07（对你的第 1 节匹配梯有一条硬约束）**：`/undo` 的唯一数据来源是编辑工具落盘的 **`<session-id>/outputs/<tool_call_id>.before`**（会话目录见票 07 第 1 节）。**它必须是"实际被替换区段的旧内容"，不能是 `old_string`**——因为你的降级梯（行尾空白无关 / 每行 trim）命中的文本可能与 `old_string` 不完全一致，用 `old_string` 反推会把文件写错。这意味着**匹配梯的返回值里要带上实际命中区间**（你的 `MatchOutcome::Unique(Range<usize>)` 已经够了），编辑工具据此切片落盘。
- 另：`/undo` 由**会话落盘**支撑，**不做"每次编辑自动 git commit"**（票 07 第 7 节）——所以编辑工具不需要与 git 交互，也不需要管用户的仓库状态。

**票 19 交接来的一条约束（2026-09-13）**：你第 1 节那个"**级名进工具结果 → 可观测**"的决定**被确认了，而且不需要升级成结构化字段**。

- **匹配梯的命中级别**（exact / 行尾空白无关 / 每行 trim）继续走**约定文本**（票 19 第 5 节），因为它本来就已经在流上了——缺的只是"可机器读"。
- **唯一要求**：**渲染级名的地方与解析它的地方共用一个格式常量**。理由是这个诊断的用途是**统计**（"这个模型是不是总在踩缩进"），而一个静默漂移的格式会让那个统计**悄悄变成 0**——它不报错，所以你永远不会发现。你第 1 节"降级是静默的"那句警告，在查询层同样成立。
- 票 19 会把它做成 `stats` 里的一个固定指标（**命中级别分布**），依据就是你这个约定文本。
- 另一条相关：`/undo` 需要的**实际命中区间**（你交接给票 07 的那个）与这里的**级名**是两件事——前者用于切片落盘，后者用于统计，别把两者混成一个字段。
