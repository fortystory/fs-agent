# research 02：git 的三个读数与输出的稳定性（findings）

来源票：[research：git 的三个读数与输出的稳定性](../issues/02-research-git-readouts.md)。
本文件只记**事实**，不做实现决定（要不要 `-uall`、字形怎么画归 spec 与 prototype 票）。

## 实测环境与证据约定

- 机器：`git version 2.56.0`，Linux，nvme 上的 `/home` 与 tmpfs 上的 `/tmp` 各测了一次。
- 默认 locale 是中文（`zh_CN`），所以 stderr 文案默认中文；凡必须给原文的地方我同时跑 `LC_ALL=C`
  拿英文。**这条本身就是一条结论：git 的错误文案随 locale 变。**
- 文档引用统一用官网 URL；我抽验过 `https://git-scm.com/docs/git-status`（HTTP 200）与同版本
  本机 man 页内容一致，其余条目的小节名取自本机 man 页（git 2.56.0 自带，与官网同源）。
- 下面每条结论后面都跟「命令 → 关键输出」；输出截断到关键几行。

---

## 一、三个读数各跑什么

### 1.1 列文件名：`git status --porcelain=v2 -z`

**文档把这一族描述为给脚本/机器解析用的形态**（v1 那节的原话是「an alternate `-z` format
recommended for machine parsing」）；以下是逐条事实，**选不选它归 spec**。

1. **行类型**：`1` = 普通改动（`1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>`）、`2` = 改名/复制
   （末尾多 `<X><score> <path><sep><origPath>`）、`u` = 未合并、`?` = 未跟踪、`!` = 忽略（要
   `--ignored` 才出现）。文档：[git-status(1)](https://git-scm.com/docs/git-status) 的
   `OUTPUT` → `Porcelain Format Version 2` 一节（`Changed Tracked Entries` / `Unmerged entries` /
   `Other Items`）。
2. **`-z` 之后路径原样**：NUL 结尾、不加引号、不做反斜杠转义，非 ASCII 的 UTF-8 字节原样。
   同一行内、状态字段与第一个路径之间仍是**一个空格**；`2` 行内两个路径之间是 **NUL**。

   ```
   $ printf 'a\n' > 中文文件名.txt
   $ git status --porcelain=v2 -z | head -c 160 | xxd
   00000000: 3f20 e4b8 ade6 9687 e696 87e4 bbb6 ...   ← "? \xe4\xb8\xad..."（原样 UTF-8）
   ```
   文档：同页 `Pathname Format Notes and -z`：「When the `-z` option is given, pathnames are printed
   as is and without any quoting and lines are terminated with a *NUL* (ASCII 0x00) byte.」
3. **不用 `-z` 时**由 `core.quotePath`（默认 true）决定引号与转义：含「不寻常」字节的路径被包在
   双引号里、字节转成八进制。引号把 `a/` 前缀一起包住。

   ```
   $ git status --porcelain=v2 | grep 中文
   1 .M N... 100644 100644 100644 7898192... 7898192... "\344\270\255\346\226\207.txt"
   $ git -c core.quotepath=false status --porcelain=v2 | grep 中文
   1 .M N... 100644 100644 100644 7898192... 7898192... 中文.txt
   ```
   文档：[git-config(1)](https://git-scm.com/docs/git-config) 的 `core.quotePath`
   （「bytes with values larger than 0x80 ... octal」；`-z` 是「completely verbatim」的那条出路）。
4. **含空格的路径**（未 `-z` 时）不被引号，字段与路径之间还是空格 —— 所以「按空格切分」在未 `-z`
   时会歧义：

   ```
   $ git status --porcelain=v2 | grep 'sp ace'
   1 .M N... 100644 100644 100644 6178079... 6178079... sp ace.txt
   ```
5. **未跟踪目录**在两种形态下都只是 `? <dir>/`（尾斜杠），展开成文件要靠 `-uall`（见 1.2）。
6. **`porcelain=v2` 并不是「与用户配置完全无关」**：`status.relativePaths` 与
   `status.showUntrackedFiles` 实测都会改变 v2 输出（见 2.x）。文档对 **v1** 才明文保证
   「guaranteed not to change ... based on user configuration」（同页 `Porcelain Format Version 1`，
   且列了两条例外），**v2 那一节没有这句保证**。这是本票最反直觉的一条之一。

### 1.2 未跟踪的粒度：默认 normal vs `-uall`，以及耗时

**默认是 normal（整个未跟踪目录报成一行）；`-uall` 才展开成文件。**
注意易混点：`-u` 后面**不带值**时等于 `all`；而**完全不用 `-u`** 时等于 `normal`。文档：
[git-status(1)](https://git-scm.com/docs/git-status) `OPTIONS` 的 `-u[<mode>], --untracked-files[=<mode>]`
（「It is optional: it defaults to all」讲的是选项的值；「When `-u` option is not used, untracked files
and directories are shown (i.e. the same as specifying normal)」讲的是不写选项）。

实测（每格 3 次取范围；`行数` = `--porcelain=v2 | wc -l`）：

| 仓库 | 被跟踪文件 | 默认行数 | 默认耗时 | `-uall` 行数 | `-uall` 耗时 |
| --- | --- | --- | --- | --- | --- |
| 本仓库 heng（干净时） | 802 | 2 | 4–6 ms | 8 | 5–6 ms |
| `dhb/finance-manage-api.dianhua.cn`（工作区 2201 个目录，无未跟踪文件） | 10537 | 6 | 22–25 ms | 6 | 23–24 ms |
| 合成：200 个目录 × 100 文件 = **20000** 个未跟踪文件（nvme） | 0 | 3 | 5 ms | 20008 | 20–22 ms |
| 合成：20000 个未跟踪文件（tmpfs） | 0 | 1 | 2 ms | 20000 | 13–14 ms |
| 合成：400 个目录 × 250 文件 = **100000** 个未跟踪文件（tmpfs） | 0 | 1 | 1–2 ms | 100000（输出 1.5 MB） | 58–62 ms |

读法：**耗时几乎全是「未跟踪文件有多少」驱动，不是「被跟踪文件有多少」驱动**。两个真仓库都很干净，
所以它们的数字看起来一样；代价只在「工作区里堆着一大棵未跟踪树」时才出现。10 万个未跟踪文件、
最坏的 tmpfs 条件下，`-uall` 也只到 ~60 ms。

用户配置会把默认改掉，实测：

```
$ git -c status.showUntrackedFiles=all  status --porcelain=v2 | grep '?'
? d/a.txt                      ← 展开
$ git -c status.showUntrackedFiles=all  status --porcelain=v2 -unormal | grep '?'
? d/                           ← 命令行选项压住配置
$ git -c status.showUntrackedFiles=no   status --porcelain=v2 -unormal | grep '?'
? d/
```

文档：[git-config(1)](https://git-scm.com/docs/git-config) 的 `status.showUntrackedFiles`（`no` /
`normal` / `all`）；[git-status(1)](https://git-scm.com/docs/git-status) 的
`UNTRACKED FILES AND PERFORMANCE` 一节还提到 `core.untrackedCache` / `core.fsmonitor` 会让后续
`git status` 变快 —— **实测提醒**：同一命令连着跑时后面几次可能因为 index 里的缓存而更快，
上面的数字都是热缓存下的。

### 1.3 一个文件的正文：`git diff HEAD -- <path>`

**对未跟踪文件：空输出 + 退出码 0。既不报错，也不给内容。**

```
$ printf 'brand\nnew\n' > brand-new.txt
$ git status --porcelain=v2 | grep brand
? brand-new.txt
$ git diff HEAD -- brand-new.txt; echo "exit=$?"
exit=0                                    ← 一行输出都没有
$ git diff HEAD -- newdir; echo "exit=$?"  ← 未跟踪目录
exit=0
```

同理：**改名**时限定单个路径会破坏 rename 检测，`git diff HEAD -- <新路径>` 变成「新增文件」、
`-- <旧路径>` 变成「删除文件」：

```
$ git mv a.txt renamed.txt
$ git diff HEAD                      → diff --git a/a.txt b/renamed.txt / similarity index 100% / rename from / rename to
$ git diff HEAD -- renamed.txt | head -3
diff --git a/renamed.txt b/renamed.txt
new file mode 100644
--- /dev/null
```

**空仓库（还没有第一次提交，HEAD 不存在）**：`git diff HEAD` 直接死，`git status` 一切正常。

```
$ git diff HEAD -- f.txt; echo "exit=$?"
致命错误：bad revision 'HEAD'
exit=128
$ git diff HEAD > /dev/null; echo "exit=$?"      ← 不带 pathspec 时是另一句
致命错误：有歧义的参数 'HEAD'：未知的版本或路径不存在于工作区中。
exit=128
$ git diff; echo "$? / 行数=$(git diff | wc -l)"  ← 不带 HEAD 的形式在空仓库里是合法的
0 / 行数=0
$ git status --porcelain=v2; echo "exit=$?"
? f.txt
exit=0
```

**二进制**：

```
$ git diff HEAD                       → diff --git a/logo.bin b/logo.bin
                                        index e57fd5b..df5a3cc 100644
                                        Binary files a/logo.bin and b/logo.bin differ
$ git diff HEAD --numstat             → -	-	logo.bin        （制表符分隔；文档说二进制输出两个 "-"）
```

文档：[git-diff(1)](https://git-scm.com/docs/git-diff) 的 `--numstat`（「For binary files, outputs two
`-` instead of saying `0 0`」）。

**未跟踪文件的另一条取法 `git diff --no-index /dev/null <path>`**（我实测了，不做取舍）：

```
$ git diff --no-index /dev/null brand-new.txt; echo "exit=$?"
diff --git a/brand-new.txt b/brand-new.txt     ← 路径头是 a/<path> b/<path>，不是 a//dev/null
new file mode 100644
index 0000000..5786b13
--- /dev/null
+++ b/brand-new.txt
@@ -0,0 +1,2 @@
+brand
+new
exit=1                                          ← 注意：有差异时退出码是 1
```

- 退出码 1 是**文档规定的**：这一形式隐含 `--exit-code`。文档：[git-diff(1)](https://git-scm.com/docs/git-diff)
  的 `DESCRIPTION` → `git diff [<options>] --no-index [--] <path> <path>`（「This form implies
  `--exit-code`」）。无差异时实测退出码 0。
- 二进制时它给 `Binary files /dev/null and b/new.bin differ`（同样 exit 1）。
- 空仓库里也能用（同上，exit 1）。

### 1.4 改名（`R`）/复制（`C`）/冲突（`U`）的形态

**改名（已暂存）在 porcelain v2 里是 `2` 行**，`<X><score>` 是相似度，路径是「新 TAB/ NUL 旧」：

```
$ git mv a.txt b.txt && git status --porcelain=v2
2 R. N... 100644 100644 100644 58b8997... 58b8997... R100 b.txt	a.txt      ← 中间是 TAB
$ git status --porcelain=v2 -z | xxd | tail -2
00000070: 2052 3130 3020 622e 7478 7400 612e 7478   R100 b.txt.a.tx
00000080: 7400                                              ← "b.txt\0a.txt\0"
```

同一场景 `git diff HEAD` 给的是 patch 头里的三行：

```
diff --git a/a.txt b/b.txt
similarity index 100%
rename from a.txt
rename to b.txt
```

对照 **v1**（供比较，别拿错格式）：`RM a.txt -> b.txt`，`-z` 时顺序反转成 `RM b.txt\0a.txt\0` ——
文档 [git-status(1)](https://git-scm.com/docs/git-status) `Porcelain Format Version 1` 写明 v1 的 `-z`
「the *->* is omitted ... and the field order is reversed」；v2 的 `2` 行**本来就是**「先 path 后
origPath」。

**要不要 `-M` / 读配置**：实测 `git status` 默认就报 `R`（`2 R.`），无需 `-M`；`status.renames`
默认跟随 `diff.renames`（默认 true）。关掉就拆成 `D` + `A`：

```
$ git -c status.renames=false status --porcelain=v2
1 D. N... 100644 000000 000000 1c99002... 0000... a.txt
1 A. N... 000000 100644 100644 0000... 1c99002... renamed.txt
$ git status --porcelain=v2 --no-renames        ← 等价写法（命令行选项）
```

文档：[git-config(1)](https://git-scm.com/docs/git-config) 的 `status.renames`
（「Defaults to the value of `diff.renames`」）；[git-status(1)](https://git-scm.com/docs/git-status)
`OPTIONS` 的 `--renames, --no-renames`。`status.renames=copies` 文档说是「Git will detect copies, as
well」—— **实测没测出 `C`**：造了「新文件是某已跟踪文件的完整副本」的场景，`git status
--porcelain=v2` 与 `-c status.renames=copies` 都报 `1 A.`；只有直接问 `git diff -C
--find-copies-harder --name-status` 才得到 `C100 a.txt copy.txt`。也就是说 **status 这一层几乎
看不到 `C`**，字形集里 `C` 基本是空档。（我试了两种副本场景都没触发，不排除还有别的触发条件；
这一点**我没有查到决定性的文档说明**。）

**冲突**：v2 用 `u` 行，七个 hash/模式字段是 stage1/2/3 加工作区：

```
$ git status --porcelain=v2          ← 冲突中
u UU N... 100644 100644 100644 100644 f0f2307... b66dd7b... bd7a938... c.txt
$ git diff HEAD -- c.txt | head -8    ← 给的是「HEAD ↔ 工作区」，**含冲突标记**
@@ -1,3 +1,7 @@
 l1
+<<<<<<< HEAD
 MASTER
+=======
+SIDE
+>>>>>>> side
$ git diff | head -4                  ← 不带 HEAD 时是 combined diff，另一种形状
diff --cc c.txt
index b66dd7b,bd7a938..0000000
@@@ -1,3 -1,3 +1,7 @@@
```

`--numstat` 在冲突状态下也分叉：`git diff HEAD --numstat` 给 `4	0	c.txt`（HEAD↔工作区），
`git diff --numstat` 给 `0	0	c.txt`（combined）。文档：[git-status(1)](https://git-scm.com/docs/git-status)
`Short Format`（`U` = 「updated but unmerged」，以及那张 unmerged 组合表）+ `Unmerged entries` 的
字段表；[git-diff(1)](https://git-scm.com/docs/git-diff) 的 combined diff 形式在 `DESCRIPTION` 里
（`git diff --cached`/merge 相关段落）。

---

## 二、让输出不受用户配置影响：逐条查清了什么

| 配置 | 实测影响 | 压住它的确切写法（实测有效） |
| --- | --- | --- |
| `color.ui` | `always` 时输出带 ANSI（`^[[1m`） | `--no-color`（等价 `--color=never`，文档明说可覆盖配置）；`status --porcelain=v2` 本来就无色 |
| `core.pager` / `GIT_PAGER` / `PAGER` | stdout 是管道时**不起 pager**；真 tty 下会起 | `git --no-pager …`（`-P`）；或 `GIT_PAGER=cat` / 空串 |
| `diff.noPrefix`（文档里的写法，键名大小写不敏感） | 头行变成 `diff --git f.txt f.txt` | `--src-prefix=a/ --dst-prefix=b/`；或 `--default-prefix` |
| `diff.mnemonicPrefix` | 头行变成 `c/f.txt w/f.txt`（`git diff HEAD` 时） | 同上（实测 `--src-prefix/--dst-prefix` 赢）；`--no-prefix` 是去掉前缀 |
| `diff.srcPrefix` / `diff.dstPrefix` | 头行前缀变成你设的值 | `--src-prefix=a/ --dst-prefix=b/` |
| `diff.external` / `GIT_EXTERNAL_DIFF` | **输出整个换成外部程序的输出**（不是 patch） | `--no-ext-diff`（文档：「Disallow external diff drivers」） |
| `diff.algorithm` | 文档说会改 hunk 边界；**本次 fixture 里没测出可见差异** | `--diff-algorithm=myers`（命令行选项覆盖配置） |
| `diff.context`（默认 3） | hunk 头与内容范围明显变（`--context=0` → `@@ -2 +2 @@`） | `--unified=3`（文档：「This value is overridden by the -U option」） |
| `status.relativePaths`（默认 true） | **porcelain v2 也受影响**：子目录里给 `../top.txt` | `-c status.relativePaths=false`，或者干脆 `git -C <repo-root>`（实测最干净） |
| `status.showUntrackedFiles` | **porcelain v2 也受影响** | `-unormal` / `-uall` / `-uno`（命令行选项压住配置） |
| `core.quotePath`（默认 true） | 未 `-z` 时路径被引号+八进制；**`git diff` 的头行也一样** | 用 `-z`（status）；`diff` 没有对应的「原样」开关，只能 `-c core.quotePath=false` |
| `safe.directory` | 见第三节；属主不一致时**连仓库都不认** | `-c safe.directory=<path>` 或 `-c safe.directory=*`（实测两者都能解锁） |
| `GIT_OPTIONAL_LOCKS=0` / `--no-optional-locks` | `git status` **默认会写回 index** | `git --no-optional-locks status` 或 `GIT_OPTIONAL_LOCKS=0` |
| `GIT_DIR` / `GIT_WORK_TREE` | 会**顶掉**「当前目录是不是仓库」的答案 | 从环境里清掉（`env_remove`）；或显式 `--git-dir` / `--work-tree` |
| `GIT_TERMINAL_PROMPT` / `GIT_ASKPASS` | 本地 `status`/`diff` 实测无可见影响（不联网、不取凭据） | —（只有 `fetch`/`push` 那类才需要，见下） |

逐条的实测原文：

**color**（文档：[git-diff(1)](https://git-scm.com/docs/git-diff) `OPTIONS` → `--no-color`；
[git-config(1)](https://git-scm.com/docs/git-config) `color.ui`）

```
$ git -c color.ui=always diff HEAD | cat -v | head -2
^[[1mdiff --git a/f.txt b/f.txt^[[m
^[[1mindex 0ff3bbb..461b272 100644^[[m
$ git -c color.ui=always diff --no-color HEAD | head -2
diff --git a/f.txt b/f.txt            ← --no-color 压住
$ git -c color.ui=always status --porcelain=v2 | cat -v | head -1
1 .M N... 100644 ... f.txt            ← porcelain 本来就无色
```

**pager**（文档：[git(1)](https://git-scm.com/docs/git) `OPTIONS` 的 `-P, --no-pager` 与
`ENVIRONMENT VARIABLES` 的 `GIT_PAGER`；[git-config(1)](https://git-scm.com/docs/git-config) `core.pager`）

```
$ git -c core.pager='sh -c "echo PAGER-RAN >&2; cat"' diff HEAD > /dev/null
（无 PAGER-RAN）                       ← stdout 不是 tty，不起 pager
$ script -qec "git -c core.pager='echo PAGER-RAN; cat' diff HEAD" /dev/null | head -3
PAGER-RAN                              ← 真 tty 下起了 pager
^[[1mdiff --git a/f.txt b/f.txt^[[m     ← 而且 tty 下默认上色
$ script -qec "git --no-pager -c core.pager='echo PAGER-RAN; cat' diff HEAD" /dev/null | head -2
（无 PAGER-RAN）
$ script -qec "git -c core.pager='echo PAGER-RAN; cat' status --porcelain=v2" /dev/null | head -2
1 .M N... ... f.txt                    ← status --porcelain 不起 pager
```

**前缀**（文档：[git-config(1)](https://git-scm.com/docs/git-config) 的 `diff.noPrefix`、
`diff.mnemonicPrefix`、`diff.srcPrefix`、`diff.dstPrefix`）

```
$ git -c diff.noprefix=true    diff HEAD | head -1   → diff --git f.txt f.txt
$ git -c diff.mnemonicPrefix=true diff HEAD | head -1 → diff --git c/f.txt w/f.txt
$ git -c diff.srcPrefix=SRC/ -c diff.dstPrefix=DST/ diff HEAD | head -1 → diff --git SRC/f.txt DST/f.txt
$ git -c diff.mnemonicPrefix=true diff --src-prefix=a/ --dst-prefix=b/ HEAD | head -1 → diff --git a/f.txt b/f.txt
$ git -c diff.noprefix=true diff --src-prefix=a/ --dst-prefix=b/ HEAD | head -1        → diff --git a/f.txt b/f.txt
$ git -c diff.mnemonicPrefix=true diff --no-prefix HEAD | head -1                      → diff --git f.txt f.txt
$ git -c diff.mnemonicPrefix=true diff --default-prefix HEAD | head -1                 → diff --git a/f.txt b/f.txt
```

注意文档条目的名字是 `diff.noPrefix`（大写 P，空格缩进下在 `diff.mnemonicPrefix` 之后），
配置键名大小写不敏感，所以 `diff.noprefix` 一样生效。

**外部 diff**（文档：[git-config(1)](https://git-scm.com/docs/git-config) 的 `diff.external`；
[git(1)](https://git-scm.com/docs/git) `ENVIRONMENT VARIABLES` 的 `GIT_EXTERNAL_DIFF`；
[git-diff(1)](https://git-scm.com/docs/git-diff) `OPTIONS` 的 `--no-ext-diff`）

```
$ git -c diff.external=/tmp/x/ext.sh diff HEAD
EXTERNAL-DIFF-RAN                       ← 输出完全变成外部程序的
$ git -c diff.external=/tmp/x/ext.sh diff --no-ext-diff HEAD | head -2
diff --git a/f.txt b/f.txt              ← 压回内置
$ GIT_EXTERNAL_DIFF=/tmp/x/ext.sh git diff --no-ext-diff HEAD | head -1
diff --git a/f.txt b/f.txt              ← 环境变量那一路也被压住
```

外部程序被调用时收 **7 个参数**（`path old-file old-hex old-mode new-file new-hex new-mode`）——
实测 `args=7`，文档同 `GIT_EXTERNAL_DIFF` 条目。附带事实：`diff.trustExitCode` / 
`GIT_EXTERNAL_DIFF_TRUST_EXIT_CODE` 为默认 false 时外部程序必须返回 0，返回别的码会让 git
报 fatal（我实测时因管道 `head` 触发 SIGPIPE，看到过 `致命错误：外部 diff 退出，停止在 f.txt` + 128）。
还有一对相关开关：`--ext-diff` / `--no-ext-diff`、`--textconv` / `--no-textconv`（后者管
`.gitattributes` 里的 textconv 过滤器，本次没测）。

**hunk 边界**（文档：[git-config(1)](https://git-scm.com/docs/git-config) 的 `diff.context`、
`diff.algorithm`；[git-diff(1)](https://git-scm.com/docs/git-diff) 的 `-U, --unified[=<n>]`）

```
$ git diff HEAD | grep '^@@'                 → @@ -1,5 +1,5 @@   /   @@ -7,7 +7,7 @@ l6
$ git -c diff.context=0  diff HEAD | grep '^@@' → @@ -2 +2 @@ l1 / @@ -10 +10 @@ l9
$ git -c diff.context=10 diff HEAD | grep '^@@' → @@ -1,20 +1,20 @@
$ git -c diff.context=10 diff --unified=0 HEAD | grep '^@@' → @@ -2 +2 @@ l1 / @@ -10 +10 @@ l9
```

`diff.algorithm=minimal|histogram` 在我这个 20 行两处改动的 fixture 里**没有产生可见差别**；
文档（`diff.algorithm`）说它选算法、`git-diff(1)` 有 `--diff-algorithm` 覆盖 —— 「会改 hunk 边界」
这点我没能用实测证实，**只能引文档，实测为否**。

**相对路径**（文档：[git-status(1)](https://git-scm.com/docs/git-status) `CONFIGURATION` 的
`status.relativePaths`）

```
$ cd sub && git status --porcelain=v2 | head -2
1 .M N... ... ../f.txt
? ../d/
$ cd sub && git -C /tmp/x/r status --porcelain=v2 | head -2
1 .M N... ... f.txt                    ← -C 到仓库根之后路径天然是根相对
? d/
```

顺带一条同族的：`git diff` 也有一整套 `--relative`（文档 `git-diff(1)` 的
`--[no-]relative[=<prefix>]`）与配置 `diff.relative`，本次没测。

**`--no-optional-locks`：`git status` 默认真的会写 index**（文档：[git-status(1)](https://git-scm.com/docs/git-status)
`BACKGROUND REFRESH`；[git(1)](https://git-scm.com/docs/git) `OPTIONS` 的 `--no-optional-locks` 与
`ENVIRONMENT VARIABLES` 的 `GIT_OPTIONAL_LOCKS`）

```
$ touch -d '2020-01-01' .git/index; touch -d '2021-01-01' f.txt
$ git status --porcelain=v2 >/dev/null; stat -c %y .git/index
2026-10-09 00:19:01.416326266 +0800     ← index 被写回了
$ touch -d '2020-01-01' .git/index; touch -d '2021-01-01' f.txt
$ git --no-optional-locks status --porcelain=v2 >/dev/null; stat -c %y .git/index
2020-01-01 00:00:00.000000000 +0800     ← 没写
$ GIT_OPTIONAL_LOCKS=0 git status --porcelain=v2 >/dev/null; stat -c %y .git/index
2020-01-01 00:00:00.000000000 +0800     ← 也没写
```

**index.lock 存在时**（陈旧锁、且工作区 stat 已失效）:`git status --porcelain=v2` 仍然 **exit 0、
输出正常**，锁文件原样留在那里 —— 它只是跳过了写回，**不会失败**。文档说的是相反方向的危害：
后台跑 status 时它持有的锁会让**别的**进程失败。

**`GIT_DIR` / `GIT_WORK_TREE`**（文档：[git(1)](https://git-scm.com/docs/git) `ENVIRONMENT VARIABLES`
的 `GIT_DIR`、`GIT_WORK_TREE`）

```
$ cd /tmp/g/elsewhere（非仓库） && git status --porcelain=v2
fatal: not a git repository (or any parent up to mount point /)
exit=128
$ GIT_DIR=/tmp/g/r/.git git status --porcelain=v2
1 .D N... ... f.txt        ← 认了那个 .git，而工作树是当前目录（所以 f.txt 显示被删）
exit=0
$ GIT_DIR=/tmp/g/r/.git GIT_WORK_TREE=/tmp/g/r git status --porcelain=v2
1 .M N... ... f.txt
exit=0
$ GIT_DIR=/tmp/g/elsewhere git status --porcelain=v2   ← 在真仓库子目录里
fatal: not a git repository: '/tmp/g/elsewhere'
exit=128
```

同族的还有 **`GIT_INDEX_FILE`**（票面没问，但同样会改读数）：`GIT_INDEX_FILE=/tmp/g/nope.index
git status --porcelain=v2` 给的是「另一套 index」的世界（`1 D. … f.txt` + `? f.txt`）。

**`GIT_TERMINAL_PROMPT` / `GIT_ASKPASS`**（文档：[git(1)](https://git-scm.com/docs/git)
`ENVIRONMENT VARIABLES`：`GIT_ASKPASS`「commands which need to acquire passwords or passphrases
(e.g. for HTTP or IMAP authentication)」、`GIT_TERMINAL_PROMPT`「git will not prompt on the terminal
(e.g., when asking for HTTP authentication)」）

结论是**与我们的三个读数无关**：`status` / `diff HEAD` 都是纯本地操作，不联网、不读凭据，所以不会
停在等人输入。实测把 stdin 关掉、再设 `GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=/nonexistent`，输出与
不设时**逐字节相同**：

```
$ git status --porcelain=v2 < /dev/null | head -1
1 .M N... ... f.txt
$ GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=/nonexistent git status --porcelain=v2 < /dev/null | head -1
1 .M N... ... f.txt
```

「会等人输入」的场景（`fetch`/`push`/`ls-remote` 之类需要 HTTP 认证的）**我没有实测** —— 本机
没有可控的、会要求认证的远端。这条只能引文档。

---

## 三、不可用态与失败的确切形状

### 3.1 不在 git 仓库里

```
$ cd /tmp/lk && git status --porcelain=v2; echo "exit=$?"
致命错误：不是 Git 仓库（或者直至挂载点 / 的任何父目录）
停止在文件系统边界（未设置 GIT_DISCOVERY_ACROSS_FILESYSTEM）。
exit=128
$ LC_ALL=C git status --porcelain=v2; echo "exit=$?"
fatal: not a git repository (or any parent up to mount point /)
Stopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set).
exit=128
```

**`git diff HEAD` 在非仓库里是另一条路，退出码不同、stderr 长得多**：

```
$ LC_ALL=C git diff HEAD; echo "exit=$?"
fatal: not a git repository (or any parent up to mount point /)
Stopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set).
warning: Not a git repository. Use --no-index to compare two paths outside a working tree
usage: git diff --no-index [<options>] <path> <path> [<pathspec>...]
…（之后是整页 usage）
exit=129
```

**要点：128 vs 129 不是「两类错误」，而是两个命令各自的失败路径；stderr 也完全不同（一个短句、
一个带整页 usage）。要判定「这不是仓库」只能靠退出码 + 是否出现 `not a git repository` 这类
特征串（而它还随 locale 变）。** 见第三节末尾的 locale 说明。

### 3.2 `git` 不在 `PATH`

用 rustc 编了个 6 行程序实测（`Command::new(...).output()`）：

```
缺名 -> kind=NotFound raw_os_error=Some(2) display=No such file or directory (os error 2)
git ok: "git version 2.56.0"
PATH 空 -> kind=NotFound raw_os_error=Some(2) display=No such file or directory (os error 2)
```

即 `std::io::ErrorKind::NotFound`（`raw_os_error() == Some(2)`，`ENOENT`）。
注意第二种写法（`Command::new("git").env("PATH", "/nonexistent")`）才真正模拟出「PATH 里找不到」；
`env_clear()` 不管用，因为 `execvp` 在 `PATH` 缺失时会退回 confstr 的默认值（`/bin:/usr/bin`），
而 `git` 就在 `/usr/bin/git`。

### 3.3 仓库存在但没有第一次提交（HEAD 不存在）

- `git status --porcelain=v2`：**完全正常**，exit 0，未跟踪文件照列（`? f.txt`；`-z` 时 `? f.txt\0`）。
- `git diff HEAD`：**exit 128**，`fatal: bad revision 'HEAD'`（带 `-- <path>` 时）或
  `fatal: ambiguous argument 'HEAD'`（不带 pathspec 时）—— 见 1.3 的原文。
- `git diff`（不带 HEAD）：exit 0、空输出（这是「index ↔ 工作区」，不需要 HEAD）。
- `git diff --no-index /dev/null f.txt`：可用，exit 1。

文档里有一条相关的旁证：[git-diff(1)](https://git-scm.com/docs/git-diff) 在
`git diff --cached` 形式的说明里写「If HEAD does not exist (e.g. unborn branches) and `<commit>` is
not given, it shows all staged changes」—— 也就是说 `--cached` 那一支对 unborn HEAD 有专门处理，
而 `HEAD` 作为显式 revision 的那一支没有。

### 3.4 `safe.directory` / "dubious ownership"

**真实触发条件**（文档：[git-config(1)](https://git-scm.com/docs/git-config) 的 `safe.directory`
条目，本机 man 页同）：仓库属于别人时，「By default, Git will refuse to even parse a Git config of a
repository owned by someone else」，用 `safe.directory` 列例外。

**本机无法造出真实的属主不一致**（没有 root；`newuidmap`/`bwrap` 都受 `no_new_privs` 限制，
`bwrap --uid X` 会把内部 uid 一起映射过去，`fakeroot` 的 `chown` 被内核拒）。我改用 git 那个
`GIT_TEST_*` 前缀的测试开关（`GIT_TEST_ASSUME_DIFFERENT_OWNER=1`）拿到了这条失败分支的真实输出。
**声明**：这个变量**不是文档化的用户接口**（我没能在 git 官方文档里查到它，出处是 git 源码/
测试套件；本机 git 2.56.0 实测它生效并走同一条分支），真实世界里触发这条分支的条件就是上面那条
`safe.directory` 文档描述。

```
$ GIT_TEST_ASSUME_DIFFERENT_OWNER=1 LC_ALL=C git status --porcelain=v2; echo "exit=$?"
fatal: detected dubious ownership in repository at '/tmp/so/r'
To add an exception for this directory, call:

	git config --global --add safe.directory /tmp/so/r
exit=128

$ GIT_TEST_ASSUME_DIFFERENT_OWNER=1 git status --porcelain=v2       ← 默认 locale
致命错误：在 '/tmp/so/r' 检测到可疑的仓库所有权
要为本仓库创建特例，请运行：

	git config --global --add safe.directory /tmp/so/r
exit=128

$ GIT_TEST_ASSUME_DIFFERENT_OWNER=1 LC_ALL=C git -c safe.directory=/tmp/so/r status --porcelain=v2
1 .M N... ... f.txt
exit=0
$ GIT_TEST_ASSUME_DIFFERENT_OWNER=1 LC_ALL=C git -c 'safe.directory=*' status --porcelain=v2
1 .M N... ... f.txt
exit=0
$ GIT_TEST_ASSUME_DIFFERENT_OWNER=1 LC_ALL=C git diff HEAD; echo "exit=$?"
…usage…
exit=129                              ← diff 又是 129 + usage 那条路
```

要点：**`status` 128、`diff` 129**；`-c safe.directory=<绝对路径>` 与 `-c safe.directory=*` 都能解锁；
`status` 与 `diff` 都在「连仓库都不认」这一层失败，所以 stderr 里第一句就是 dubious ownership。
（用测试钩子代替真实条件这一点要说在明处：**触发条件是真实的，但本机的确没能实测真实属主不一致
的那次运行**。）

### 3.5 locale：错误文案会本地化

同一条件、只换 `LC_ALL`：

```
$ git status --porcelain=v2        → 致命错误：不是 Git 仓库（或者直至挂载点 / 的任何父目录）
$ LC_ALL=C git status --porcelain=v2 → fatal: not a git repository (or any parent up to mount point /)
```

**所以「靠 stderr 里出现某个英文短语来判断失败类型」不可靠**，判断应当落在这个进程自己的
退出码/信号上（或者把子进程的 `LC_ALL=C` 固定住）。

### 3.6 超时该定几秒（依据）

上表里最坏的一次是「10 万个未跟踪文件 + `-uall` + tmpfs」= **约 60 ms**；真仓库（10537 个
被跟踪文件、2201 个目录）是 **23–25 ms**；本仓库 4–6 ms。把这些当上界，再考虑真实磁盘、冷页缓存、以及别的东西
在抢 IO（真实世界可能慢一个数量级，也就是几百毫秒），**1 秒已经是很宽的余量，2–5 秒是「绝不会误杀，
但卡住时用户能忍」的区间**。这是由上面数字推出的建议，**不是本票的决定**；另外要留意
`UNTRACKED FILES AND PERFORMANCE` 里那条「同一个命令第二次跑会更快」的缓存效应 —— 冷热差距也在
这个量级里。

---

## 与本票相关的、但票面没问的几条（记下来，别丢）

1. **`git status` 默认写 index**（可选锁）—— 用户终端里可能正跑着 git，这一条是票面特意点名的，
   实测确凿（2.x 那张表）。
2. **porcelain v2 会跟随用户配置**（`status.relativePaths`、`status.showUntrackedFiles`），
   v1 才有白纸黑字的「regardless of user configuration」保证。
3. **`GIT_INDEX_FILE`** 也属于「继承来的环境会改读数」那一类。
4. **`--no-index /dev/null <path>` 的退出码是 1**（有差异时），且路径头是 `a/<path> b/<path>` ——
   拿它当正文时，这一条会同时影响「成功判定」和「头行长什么样」。
5. **`git diff HEAD -- <path>` 对改名文件只给一侧时，会退化成新增/删除**；列表与弹窗若各自用
   「路径 → 内容」这条路，名字变化会以两份不相干的 diff 出现。
6. **状态行里 `C`（复制）在 status 这一层几乎见不到**（实测两轮都没触发，只有 `git diff -C
   --find-copies-harder` 能给）。
7. 无 tty 时 `git diff` 不上色也不起 pager；**真 tty 下默认上色**（`color.ui=auto` 的默认值是
   `auto`）—— 只有在我们把子进程接到用户终端时才会遇到，但值得记着（`git --no-pager` +
   `--no-color` 都压得住）。
