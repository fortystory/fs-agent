# 03: 工具循环与文件工具

**What to build:** agent 能真的读、写、改工作区里的文件。模型发出的工具调用被拼装、派发、产出结果，并且「每个 `tool_call` 恰好一条结果」这条不变量由循环**在一处**保证。

Blocked by: 01 · 骨架与唯一接缝

Status: done

**参考:** spec §7（工具与副作用）、§8（编辑匹配梯）、§10（单次结果截断）

- [x] `Tool` 是对象安全的 async trait：`spec()` / `effect(args) → ReadOnly | WritePaths | Exclusive` / `call()`；注册表是**运行期值**（无全局 static，随会话走），有运行期注册挂载点
- [x] 内建 `read_file` / `write_file` / `edit_file`；编辑线级契约是「绝对路径 + `old_string` + `new_string` + 可选 `replace_all`」
- [x] 匹配梯 = 有序列表 first-success-wins：**精确 → 行尾空白无关 → 每行 trim**；**命中级别必须进工具结果**（降级不静默）
- [x] 三样护栏：非唯一匹配拒绝、匹配区间过大拒绝、占位符拒绝（判据是**注释形状的短语**，不误伤 Rust 的 `..` / `..=`）
- [x] 匹配全线失败 ⇒ 失效该路径的 read set，模型必须重读
- [x] `read-before-edit` 与 per-path 写互斥都在 dispatch 集中强制；锁表是**组装期注入**、跨执行者共享
- [x] 调度器即使 v1 串行，也**已经**按 `effect` 分流（并行只读是接线而非重构）
- [x] 编辑把**实际被替换区段的旧内容**（不是 `old_string`）落到 `outputs/<tool_call_id>.before`
- [x] 正常路径下每个 `tool_call` 恰好一条 `ToolCallCompleted`
- [x] e2e：假 provider 发一次 `edit_file`，断言文件真的改了、事件流有起止两条、`.before` 内容 == 被替换的原文

## Comments

实现落点：`src/tools/`（`tool` / `paths` / `edit` / `file` / `registry`）、`src/session.rs`、`src/agent.rs`、`src/lib.rs`。

实现期收口的两处细节（spec 未改，只是把票面留白写实）：

1. **读集失效与读集记录都不发生在工具里。** dispatch 被拆成 `guardrails()`（纯读 `(tool, args, read set, cwd)`，产出 `GuardedCall`）与 `dispatch()`（拿已裁决的 `AllowedCall` 执行、持锁）。只有循环写事件流，所以对 read set 的写入也只能由循环在做完裁决后施加；否则 `Registry` 就得同时被可变借出两次。
2. **`replace_all` 只走精确层。** 降级层是给唯一字符串吸收格式偏差的；在 `replace_all` 下按模糊命中批量替换，正好是护栏要防的「改错一片」。`replace_all` 一律精确匹配，命中数为 0 报 `NoMatch`，护栏「区间过大」按首个到末个命中的包络测量。

两轴评审后又收口的四条（都以测试钉住）：

3. **`Tool` 除三件套外多一个 `read_paths(args)`。** 读集要由「工具自己声明读了哪个路径」来填，不能让 dispatcher 去猜参数名（`path` 还是 `file_path`）；这是三件套之外的第四个方法，属于实现期决定。
4. **路径接受绝对或相对，但一律限定在会话 cwd 子树内。** 票面写「绝对路径」，实现放宽为两种写法都收，权威约束换成更强的那条：越出 workspace 的模型路径直接拒。绝对路径全放在 cwd 之外时行为与票面一致。
5. **`read-before-edit` 只挡「已存在的目标」，且失败读不留授权。** 覆写一个已存在但没读过的文件被拒；新建文件无需先读（没有东西可覆盖）；`read_file` 失败不写读集，所以失败读换不来一次写。错误文案用 `read before write`（同一道闸也覆盖 `write_file` 的覆写）。
6. **`replace_all` 时 `.before` 是各命中旧文本按序拼接（纯文本、无偏移）。** 票面要的单次编辑场景是精确的；多区段的可还原性（带偏移）留给票 12 的 `/undo` 定，不在这里预做格式。另外 `.before` 在目标文件写成功之后才落盘——写失败就不该留下「可撤销」的假象。
7. **`.effect()` 的 `Exclusive` 有真语义：取「工作区独占锁」（先于 per-path 锁获取）。** v1 没有工具产出它（`bash` 是它的主人），所以它带 `#[allow(dead_code)]`；但语义已经接上，不是留给未来的空槽。
8. **v1 循环仍是串行 `for`，没有真的分流执行。** 按 `effect` 的判据（只读不取写锁、`WritePaths` 取锁、`Exclusive` 取工作区锁）已经在 `guardrails`/`dispatch` 里成立，所以将来把一批 `ReadOnly` 调用 `join!` 起来是接线，不用重构；票面要的是这个前提成立。
