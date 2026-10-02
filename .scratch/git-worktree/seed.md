# 种子材料：git worktree

> **这不是 spec，也不是票。** 它是 2026-10-02 在一轮 `/ask-matt` 里记下的一句话意向：
> 「支持 git 的 worktree」。
> 还没被访谈、也没有票；想推进时走 `/grill-with-docs` 把它折成 `spec.md`，再 `/to-tickets` 拆票。
> **写就于 2026-10-02**；下面《现状》一节核实于同一天。

## 它要什么

让人（或者它自己）在**一个 git worktree 里**干活，而不是所有活都挤在同一个工作目录。常见的
三种读法，代价差得很远：

1. **会话跑在 worktree 里**：起会话时指定一个 worktree（或者凭空开一个），那一整场都在那儿。
2. **执行者各拿一个 worktree**：`task` 派出去的执行者互不踩文件 —— 这是「工作区隔离」的另一条
   路（今天靠 `PathLocks` 串行化）。
3. **`/worktree` 命令**：会话中途新建或切换到一个 worktree。Codex 的 `/worktree` 就是
   「start or continue a conversation in a new worktree」。

## 现状（2026-10-02 核实）

- **零命中**：`grep -rn worktree` 在 `src/`、`tests/`、`.scratch/`、`.gitignore` 与配置里一处都
  没有 —— 这是全新的意向，没有半成品。
- **一手材料**在 [`docs/research/notes/codex-gemini.md`](../../docs/research/notes/codex-gemini.md)：
  Codex 把它当作**隔离**手段（`codex-rs/worktree` crate、`--worktree` 旗标、`/worktree` 斜杠
  命令），并链到官方文档 `learn.chatgpt.com/docs/environments/git-worktrees.md`。
- 仓库里**与它相邻**的既有面（推进时要跟它们对齐，别另起一套）：
  - **会话桶按 cwd 分**（[`CONTEXT.md`](../../CONTEXT.md) 的「会话存储」/「会话桶」）：不带 id 的
    `--continue` 只扫本桶，而 `-c <id>` 命中别的工作区时会切到那场会话自己的工作目录
    （`continue-by-id`）。worktree 会把「**工作区到底是什么**」顶到台面上：worktree 是同一个
    工作区的另一个检出，还是另一个工作区？
  - **权限模式 `workspace` 档**（`workspace-mode`）与**沙箱**（`sandbox`）：两者都以「会话工作区」
    为界 —— 区外写要问一次、内核只允许工作区可写。换了目录之后那条界线画在哪。
  - **执行者与写锁**（`multi-agent-architecture`、`task` 工具、`PathLocks`）：并行执行者今天共享
    一个工作区，靠 per-path 写互斥防撞；worktree 是「用隔离换成本」的另一条路。

## 待谈的分叉

1. **会话级**（整场在一个 worktree 里）还是**执行者级**（每个 `task` 一个）？后者的生命周期与
   清理规则完全是另一套。
2. worktree **从哪来**：直接调 `git worktree add`（那就要定分支名、基点、失败怎么报），还是要求
   人先建好、会话只认路径。
3. **工作区身份**：会话桶、`-c`、权限档、沙箱都以 cwd 为界 —— 这条定了，上面三处才有一致的
   答案（「同一工作区」还是「另一个工作区」）。
4. **生命周期**：会话结束时自动 `git worktree remove` 吗？有未提交改动时怎么办（删还是留）？
5. **与 `--continue` / `-c` 的交互**：重放一场在 worktree 里的会话时，那个 worktree 可能已经
   被删了；退出的那行回执（`fs-agent -c <id>`）要不要把路径也带上。
6. **权限与沙箱**：默认的 worktree 落在仓库之外（或 `.git/worktrees/` 那一带），于是 `workspace`
   档的「区外写要问一次」会不会把它挡在外面 —— 要不要自动算作区内；遮罩目录与 `.git/config`
   那几条写死边界在 worktree 里怎么算（`.git` 在 worktree 里是一个文件，不是目录）。
7. **界面与命令**：状态行 / 终端标题 / 左栏要不要显示 worktree 名，以及有没有 `/worktree`。
