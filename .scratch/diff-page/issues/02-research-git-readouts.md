# 02 — research：git 的三个读数与输出的稳定性

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

[01 号票](01-charting-decisions.md) 定了三件事：**「改了」= 相对 HEAD**（已暂存 + 未暂存 +
未跟踪）、列表那一行走 `git status` 那一套、点开时取 `git diff HEAD -- <path>`。这一票只查清
**取数的具体形状与它的稳定性**，不写实现、不做实现决定。

### 一、三个读数各跑什么，argv 逐字是什么

1. **列文件名**：`git status --porcelain=v2 -z` 是不是对的那一条？`-z` 之后路径怎么解
   （NUL 分隔、没有引号包裹、非 ASCII 原样？），不用 `-z` 时路径的引号与转义由哪个配置管
   （`core.quotepath`）？
2. **未跟踪的粒度**：默认把未跟踪的**整个目录**报成一行（`?? dir/`），`-uall` 才展开成文件。
   在一棵真的大仓库上（本仓库、以及一个几万文件量级的仓库各一次）实测两者各要多久、各有多少行。
   顺带查清：`status.showUntrackedFiles` 这个用户配置会不会把我们的默认改掉，要锁它吗。
3. **一个文件的正文**：`git diff HEAD -- <path>` 对**未跟踪**文件给什么（空输出？退出码？
   报错？）—— 若它不给，未跟踪文件那一档该怎么取（直接读盘？`git diff --no-index /dev/null <path>`？
   后者对退出码与路径头有什么影响）。
4. **二进制文件**：改了图片 / 二进制时 `git diff` 给什么（一句话？），`--numstat` 给什么。
5. **改名的形态**：`R` / `C` 在 porcelain v2 与 `git diff` 里各长什么样，要 `-M`/`-C` 还是读
   `status.renames` 配置？冲突（`U`）在两处的形态。

### 二、让输出不受用户配置影响：要锁哪些开关

逐条查清并给出**确切的命令行写法**（`--no-color` / `-c key=value` / 环境变量各用哪个）：

- `color.ui`（输出里的 ANSI 与 `--color` 的关系）；
- `diff.noprefix` / `diff.mnemonicPrefix` / `diff.srcPrefix` / `diff.dstPrefix`（`a/`、`b/` 那个前缀）；
- `diff.external` / `GIT_EXTERNAL_DIFF`（配了外部 diff 时 `git diff` 的输出会变成别的程序的输出！）；
- `core.pager` / `GIT_PAGER`（会不会去起 pager、要不要 `--no-pager`）；
- `diff.algorithm` / `diff.context`（hunk 的边界会不会变）；
- `status.relativePaths`；
- `safe.directory` 与 "dubious ownership"（**仓库属主与当前用户不一致时 git 会直接拒绝运行**）——
  这一条在什么条件下触发、退出码与 stderr 是什么、我们该不该自动加 `-c safe.directory=*`；
- `GIT_OPTIONAL_LOCKS` 与 index.lock：`git status` 会不会去写 index（refresh），要不要
  `--no-optional-locks`（**这一条重要：用户自己的终端里可能正跑着 git**）；
- `GIT_TERMINAL_PROMPT=0` / `GIT_ASKPASS`：哪些配置会让 git 停下来等人输入（那样我们会卡住）；
- 环境里要不要清掉继承来的 `GIT_DIR` / `GIT_WORK_TREE`（它们会让「当前目录是不是仓库」变成
  另一个答案）。

### 三、不可用态与失败的确切形状

- 不在 git 仓库里：退出码、stderr 原文。
- `git` 不在 `PATH`：`Err` 的 `ErrorKind` 是什么。
- 仓库存在但 HEAD 不存在（**还没有第一次提交的空仓库**）：`git diff HEAD` 会怎样、
  `git status --porcelain=v2` 会不会成功 —— 这个状态在新建仓库里很常见，要有确切答案。
- 超时该定几秒：给上面实测的耗时数字（大仓库那一次）作为依据。

### 产物

`.scratch/diff-page/research/02-git-readouts.md`：逐条回答，**每条给证据** —— 引 git 官方文档
（`git-status` / `git-diff` / `git-config` 的 `https://git-scm.com/docs/...` URL 与相关小节名），
本地实测的写清命令与输出（截断到关键几行）。查不到的明说「查不到」，不要推断。

**本票只查事实，不做「要不要 `-uall`」「字形怎么画」这类决定** —— 那些归 spec 与
[prototype：列表与弹窗在两档宽度下的排版](04-prototype-layout.md)。

## 作答

全文与逐条证据（文档小节名 + 本地实测输出）在
[`research/02-git-readouts.md`](../research/02-git-readouts.md)。

- **列文件名**：`git status --porcelain=v2 -z` 可用；`-z` 是 NUL 结尾、路径原样不加引号，`2`（改名）行内
  两路径之间也是 NUL；不用 `-z` 时引号与转义归 `core.quotePath`。
- **正文**：`git diff HEAD -- <path>` 对**未跟踪**文件给**空输出 + 退出码 0**；空仓库（无 HEAD）里它给
  128 / `fatal: bad revision 'HEAD'`，而 `git status --porcelain=v2` 照常成功。
- **锁法**（逐条实测有效）：`--no-color`；`--no-ext-diff`（压 `diff.external` 与 `GIT_EXTERNAL_DIFF`）；
  `--src-prefix=a/ --dst-prefix=b/`（压 `diff.noPrefix` / `mnemonicPrefix` / `srcPrefix` / `dstPrefix`）；
  `--no-pager`；`--unified=3`；命令行 `-unormal` / `-uall`；`git --no-optional-locks`；清掉继承来的
  `GIT_DIR` / `GIT_WORK_TREE`。
- **失败态**：非仓库里 `status`=128、`git diff HEAD`=129（后者还吐整页 usage）；`git` 不在 `PATH` 是
  `ErrorKind::NotFound`；dubious ownership 下 status=128、`-c safe.directory=<path>` 或 `*` 都解锁；
  **错误文案随 locale 本地化**（别靠英文短语判定）。
- **超时依据**：最坏实测是 10 万未跟踪文件 + `-uall` ≈ 60 ms，真仓库（10537 文件）23–25 ms，本仓库 4–6 ms
  → 1 秒已很宽，2–5 秒是不误杀的区间（由数据推出，不是决定）。
- **出乎意料**：`git status` **默认会写回 index**（要 `--no-optional-locks`）；文档里那个键写作
  `diff.noPrefix`（大写 P）；**`porcelain=v2` 会跟随用户配置**（`status.relativePaths`、
  `status.showUntrackedFiles`），v1 才有「不受用户配置影响」的明文保证；status 这一层几乎见不到 `C`。
- **没查到的两条**：`diff.algorithm` 改 hunk 边界（文档说有，本 fixture 测不出差异）；prompt 类配置
  的「停下来等人输入」（本地三个读数不联网，实测无差别；会提示的场景本机没有可控远端）。
