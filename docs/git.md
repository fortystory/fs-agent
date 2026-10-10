# `git`

`git(op, args?)` 是内建的**版本控制**入口：模型给一个 `op`、可选的 `args`，它在会话工作区里
跑一条 git 操作，返回它的退出码、标准输出与标准错误。来源是
[`.scratch/tool-coverage/spec.md`](../.scratch/tool-coverage/spec.md) §3。

## 为什么它不是一条 shell 命令

在它之前，模型要走 git 只有一条路：经 `bash` 拼一条 `git …`。那条路上的**权限代数**是错的
——`bash` 的 `effect()` 恒为 `Effect::Exclusive`（「一个 shell 什么都能写」），于是 `git status`
与 `git diff` 这种纯读取也一样要独占工作区、在 `readonly` 档下直接被拒。实测 `git` 在 bash 里
出现 1799 段，其中读侧约 78%（`diff` 436 / `status` 395 / `log` 285 / `show` 78）。

## 参数面

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `op` | 是 | 枚举：`status` / `diff` / `log` / `show` / `add` / `commit` / `stash` |
| `args` | 否 | 字符串数组，**原样透传**给 git（`--stat`、`--oneline`、`-n`、换目录的 `-C`） |

- **两条工具而不是一条**：模型不会为「这条 git 该走哪条路」多花一次判断，它会一律走 `bash`
  ——`grep` 的劝阻八天无效、`workdir` 上线两天 0.01% 采纳率，是同一族证据。
- **原样透传**：git 自己的开关不必重新学一套参数名，而验证它们能做到什么的正是 git 自己。
- **不给 `workdir`**：实测 `git -C` 出现 0 次，而 git 自己向上找仓库根。需要换目录就用 `args`
  透传 `-C`。
- **不在表里的那几条留给 `bash`**：`rev-parse` / `branch` / `ls-files`，以及搭测试仓库用的
  `config` / `init` / `user` / `safe`。表外的那些是一次性的搭建动作，不是日常查询。

## `effect()` 按 `op` 分档

| `op` | 效果 | 意味着 |
| --- | --- | --- |
| `status` / `diff` / `log` / `show` | `Effect::ReadOnly` | 四档放行、不取工作区锁 |
| `add` / `commit` / `stash` | `Effect::Exclusive` | 独占工作区；`readonly` 档拒 |

写 op 拿不到更精确的 `WritePaths`：`add` / `commit` 写的是 `.git/index`（以及 `.git/index.lock`、
对象库），**枚举不出**「恰好这些路径」。先例是 MCP 转发工具那处的「按参数返回不同 `Effect`」。

**不认识的 `op` 是参数错误，而且在分档之前就判出来**（`Tool::validate`，由
`Registry::facts()` 在权限门之前调）。否则用户会先为一次注定失败的调用点一次头，才拿到那句
「`op` 不认识」。

## 边界

- **模型侧必须过权限门与沙箱**：它走工具层那个带沙箱的进程件（`tools/process.rs`），与 `bash`
  同一条路。所以工作区可写、`.git/config` 与 `.git/hooks` 压回只读，而 `.git/index` 可写 ——
  把整个 `.git` 压只读会让提交全线失败。
- **与渲染层代码不共用**：渲染层那件宿主侧 git（改动页）的三条纪律是「不过权限门、不过沙箱、
  不在任何一帧里 await」，与模型侧的要求正相反。共用的只是「起一个子进程」这件事本身，
  不是一段代码，也不是 diff 的解析与上色。
- **输出原样**：stdout / stderr / 退出码各带一段，**不加** `cwd:` 首行 —— 站位固定是会话 cwd，
  没有歧义（`bash` 那条要加是因为站位可写）。溢出仍走既有的截断流水线。

## 与别的工具的分工

| 要知道什么 | 用哪个 |
| --- | --- |
| 仓库现在是什么样 | **`git(op: "status")`** |
| 某次改动的内容 | **`git(op: "diff")`**，或者改动页（人看的那一面） |
| 这个工作区里有哪些文件 | **[`glob`](glob.md)** |
| 某个名字出现在哪里 | **[`grep`](grep.md)** |
| 装一个测试仓库、改 remote | **`bash`**（不在 `op` 表里） |