# 基于 bubblewrap 的沙箱：让 shell 的写边界由内核担保

`bash` 与动态工具从此跑在一个 **bubblewrap 沙箱**里：整台机器以只读挂进来，只有会话工作区、`/tmp` 与一小列工具缓存目录可写，越界由内核打回而不是由我们预判。这是 `README` 一直以来记着的那条「升级路径：只做 Linux 的 bubblewrap」的落地。

它改写了三处既有文字：README 的「这不是沙箱」那段（[ADR 0006](../../docs/adr/0006-sandbox-by-bubblewrap.md)）、`docs/bash.md` 的「`bash` 不包含什么」第一条、以及 README 里那节现在叫「**这一版不做**」的清单——最后这条不是本次的功能改动，而是那个名字读起来像永久判决、实际却只是范围边界，借这次一并纠正。

来源是 2026-10-01 的一轮 grilling（Q1–Q24），材料与一手引用在 [`.scratch/sandbox/research/`](research/)（五份调研 + 两份本机实测/图解）。

## Problem Statement

**shell 是权限门唯一管不住的工具。** 别的工具都有写集合：`write_file` 的路径被 [`SessionPaths`](../../src/tools/paths.rs) 限制在会话 cwd 内，`.env` 家族与 `.git` / `.ssh` 是任何规则都降不下去的拒绝地板。`bash` 没有写入集——它的 [`effect()` 恒为 `Exclusive`](../../src/tools/bash.rs)，意思是「它能写任何东西」，于是权限门只能决定**跑不跑**，决定不了**写哪里**。

后果是具体的：`auto` 档下 `bash -c 'echo x > .env'` 没有任何东西拦得住；`ask` 档下唯一的防线是**问用户**，而用户没法从一条命令字符串里可靠判断它会不会越界。README 把这条写在安全模型里：「**这不是沙箱。** v1 不做进程级隔离。」

**判据只能来自内核。** 调研的结论一致：同类产品里没有一家是「猜 argv」的（[03 §①](research/03-agent-sandbox-precedents.md)），因为 shell 的间接性让静态形状与真实行为脱钩（变量、`eval`、解释器、写脚本再执行）。可靠的做法只有一个：**让内核在写发生的那一刻说不行**。

## Solution

在**唯一那处 spawn** 上加一层包装。`src/tools/process.rs` 的 `run()` 是 `bash` 与动态工具共用的那一半（超时、进程组 kill、输出捕获都在那），沙箱在那里把 argv 包成一个 bubblewrap 调用：

```
bwrap
  --new-session --die-with-parent
  --ro-bind / /                       # 整机只读挂进来
  --bind <root> <root>                # 每个可写根一条
  --tmpfs /tmp
  --dev /dev --proc /proc
  --unshare-user --unshare-pid --unshare-ipc --unshare-uts
  --tmpfs <遮罩目录> --remount-ro <遮罩目录>   # 每个遮罩两条
  -- <原 argv>
```

内核给的拒绝信号是 **`EROFS`**（写只读挂载），不是我们算出来的。

**探测一次，在组装期**，而且必须真跑一条最小 profile——装了 bubblewrap 不等于它在这个环境里能用（Ubuntu 24.04 的 AppArmor 限制、容器里、WSL1 都会让它起不来）。探测失败就 **fail closed**：拒绝跑 shell，并给出可操作的出路；另有一个显式的 `[sandbox] mode = "off"` 让用户自己决定放弃这层。

## User Stories

1. 作为用户，我想让 `auto` 档下的 `bash` **写不出工作区**，这样我不用逐条读命令就能放心让它跑构建与测试。
2. 作为用户，我想让**工作区内的修改照常**——包括新建文件、删除文件、跑 `cargo test`——不必为沙箱额外做任何事。
3. 作为用户，我想让 shell **读不到我的 provider key**（`~/.config/fs-agent/config.toml`）与 `~/.ssh`，这样「模型把密钥发到网上」这条暴露路径少掉一半入口。
4. 作为用户，我想让工作区里的 `.git/config`、`.git/hooks` 与 `.env` 文件**用 shell 也改不动**，让现有的拒绝地板不再是文件工具专属——但 `git add` / `git commit` 必须照常能跑。
5. 作为用户，当这台机器上 bubblewrap 用不了时，我想让 `bash` **明确拒绝**并告诉我为什么、怎么办，而不是悄悄降级成一个看起来在保护的「问」。
6. 作为用户，我想有一个开关能**彻底关掉这层**，因为有些环境我宁愿自己承担。
7. 作为模型，我想在 `bash` 工具的描述里**读到自己在沙箱里**，这样我不会对着区外反复试。
8. 作为审计者，我想在事件流里看到**每次会话的沙箱状态**，以便回头核对某条命令当时到底有没有被关着。

## Implementation Decisions

### §1 形状：一条纯函数，不是一套抽象

- 新增 `src/tools/sandbox.rs`，核心是一个**纯函数** `wrap(argv, cwd, spec) -> Vec<String>`——输入输出都是值，不碰进程、不读环境。它的输出可以**逐字断言**，这是这一层最值钱的性质（不需要真有 bubblewrap 就能测）。
- **不做 provider 抽象**：没有平台候选链、没有后端 trait、没有 `enforcement` 上报。理由见 [05 §⑤](research/05-platform-portability-macos-windows-harmonyos.md)：真做过这件事的 `birdcage` 停更约 2.5 年、`cross-sandbox` 下载 67 次，而 DSH 那条最完整的链在 Windows 上也只能自报 `partial`。
- 将来加 macOS（Seatbelt 能表达同样语义）时，代价是**加一个分支**，不是重构——因为值钱的那条缝（`wrap()` 与 `run()` 的分界）已经在了。Windows 与鸿蒙不是「位置」问题：前者的三条路线都要求改动宿主 ACL，后者第一层就没有可用机制（[05 §③§④](research/05-platform-portability-macos-windows-harmonyos.md)）。

### §2 接缝：`process.rs` 一处，两个调用点跟着走

- `process::run` 现在直接 spawn `argv`；改成 spawn `wrap(...)` 的结果。超时、`process_group(0)`、`killpg`、输出捕获**一行都不用动**——bubblewrap 会 `exec` 目标命令并透传退出码，进程组语义不变。
- 两个调用点把沙箱传下去：[`bash.rs`](../../src/tools/bash.rs) 与 [`custom.rs`](../../src/tools/custom.rs)（`config.toml` 声明的动态工具）。两者共用 `process::run`，所以**执行者与讨论者自动同等生效**。
- `ToolContext` 加一个字段 `pub sandbox: &'a Sandbox`，与现有的 `bash: &'a BashLimits` 同一个形状（会话配置携带进来，工具永不伸手去够会话）。

### §3 探测与可用性

- **组装期探一次**，形态是跑一条最小 profile：`bwrap --ro-bind / / --dev /dev --die-with-parent -- /bin/true`，**以退出码为准**。不用 `bwrap --version`：装了不等于能用。
- 在 `PATH` 上找 `bwrap`，但**排除 cwd**（防有人往工作区里放一个假的）。
- 结果存进会话组装出来的注入值，每条命令不重探、不理会 PATH 中途变化。
- **不可用 → fail closed**：`bash` 与动态工具返回一个**工具错误**（不是「命令失败」），文本里给出两条出路（装 bubblewrap，或把 `[sandbox] mode` 设成 `"off"`）。
- `[sandbox] mode = "off"` 是显式放弃这层，此时 `wrap()` 退化成单位函数、探测根本不跑。
- **`/proc` 建不起来也算不可用**（无特权容器里的典型症状是 `Can't mount proc on /newroot/proc: Operation not permitted`）。这一版不做自动降级、不给 `--no-proc` 开关：那会把「沙箱没建起来」变成一次静默的重试。

### §4 边界：可写根、遮罩、保护路径

**可写根**（每条一个 `--bind <root> <root>`，顺序在 `--ro-bind / /` 之后）：

1. 会话 cwd；
2. `[sandbox] writable_roots` 列表里的每一项，默认 `~/.cargo`、`~/.rustup`、`~/.cache`——没有它们，`cargo build` 会因为写不了 `~/.cargo/.package-cache` 而失败。**这是可用性决定，不是安全决定**；不放 `~/.npm`、`~/.aws`、`~/.ssh`。
3. `/tmp` 用 `--tmpfs /tmp`（私有一次性临时区），不是 `--bind`。

**`--tmpfs /tmp` 有一条模型会撞上的后果**：每一次 `bash` 工具调用都是一次新的 spawn，也就是**一个新的空 `/tmp`**。同一条命令里写 `/tmp` 再用没问题，**跨两条命令就不通**——实测：一条命令在 `/tmp` 建目录并成功，下一条命令连那个路径都不存在。所以 `bash` 的工具描述必须把这条讲明白（见 §8），否则模型会以为 `/tmp` 是持久的。选它而不是 `--bind /tmp /tmp`，是因为 `/tmp` 在语义上属于**区外**：私有 tmpfs 让它既不可持久，也不把模型写的东西留在宿主上。

**遮罩**（每个目录两条：`--tmpfs` + `--remount-ro`）：

- `~/.config/fs-agent`（provider key 就在那儿）与 `~/.ssh`。
- 表现是「目录还在，但是空的、且只读」——**不是「不存在」**。这个区别实测过：不加 `--remount-ro` 时 tmpfs 是可写的，程序会把东西写进一个注定被丢弃的地方然后自己困惑。
- **目录不存在就整条跳过**：bwrap 对不存在的 `--tmpfs` 目标会直接报错退出，所以组装时要先判存在。

**保护子路径**（在可写根的 `--bind` **之后**追加 `--ro-bind`，顺序反了等于没保护）：

- 工作区里的 `.git/config` 与 `.git/hooks`——**不是整个 `.git`**；
- 工作区里**存在的** `.env` 家族文件（`.env`、`.env.local`…，但 `.example` / `.sample` / `.template` 那三个后缀除外，与 [`permissions.rs`](../../src/permissions.rs) 的 `ENV_TEMPLATE_SUFFIXES` 同一份口径）；
- shell rc 文件（`.bashrc` 等）不在工作区内，`--ro-bind / /` 已经管了，不额外处理。

**为什么不是整个 `.git`。** 最初照 Codex 的做法把 `.git` 整个压回只读（[03 §②](research/03-agent-sandbox-precedents.md)），实测下来 `git add` 与 `git commit` **全线失败**——它们写的第一样东西就是 `.git/index.lock`（`致命错误：无法创建 '.git/index.lock'：只读文件系统`，退出码 128）。但提交是正当操作，而权限门那条 `.git` 地板真正要防的是另外两样：**改 `.git/config`**（把 push 指向别处）与**改 `.git/hooks`**（下次 commit 执行任意代码）。这两个恰好是文件/目录级的，`--ro-bind` 能精确表达。改后实测：`git add` / `git commit` / `git log` 全部正常，而 `git config user.name hack` 与写 `.git/hooks/pre-commit` 都被内核拒掉。

这份清单与权限门的拒绝地板**对齐但不等同**：地板是「任何规则都降不下去」，这里只是「shell 也绕不过去」，而且**比地板窄**——地板保护整个 `.git` 目录，这里只保护它的两个入口。

### §5 网络：这一版不隔离，且如实说

- **不加 `--unshare-net`**。实测：加了之后连 `1.1.1.1:443` 都不通，`cargo build` / `npm install` 会一起断。
- 因此 **README「(d) 出网」那条暴露路径这一版完全没有改善**。`docs/credentials.md` 要照旧写着「如实说：什么都没有」，只补一句「文件沙箱不解决这一条」。

### §6 拒绝信号：只区分「沙箱坏了」与「命令失败了」

- 两者退出码都是非 0。唯一稳定的判据是 **bubblewrap 自己的诊断都带 `bwrap: ` 前缀**（实测原文：`bwrap: Can't create file /no-such-dir: Read-only file system`）。
- `bwrap: ` 开头 → 这是**沙箱没起来**，返回工具错误，文本要说明「命令没有跑」；
- 其余一律**原样返回给模型**。「命令被只读挡回」的消息来自内核或工具，**随 locale 变**（中文环境下是「只读文件系统」，`LC_ALL=C` 下是 `Permission denied`），所以不用它做判据——那会是一处会在中文环境里静默失效的逻辑。
- 模型读「只读文件系统」本来就懂。

### §7 配置：两个旋钮

```toml
[sandbox]
mode = "bwrap"                # 或 "off"
writable_roots = ["~/.cargo", "~/.rustup", "~/.cache"]
```

- 遮罩目录与保护路径**写死**，不给旋钮：它们是安全默认，不该被人为了顺手改松。
- `mode` 与权限模式三档是**两件不同的事**：前者决定「跑起来能碰到什么」，后者决定「跑不跑、要不要问」。

### §8 记录与可见性

- 沙箱状态**进事件流**（log-only 事件：模式 + 不可用时的原因），靠 replay 就能重算出某条命令当时有没有被关着。这与「事件流是唯一真相源」一致。
- **界面不显示**：TUI 左栏已经很挤，而沙箱在一个会话内基本不变。
- **模型侧写进 `bash` 工具的描述**（请求前缀的一部分，加一句是常量成本）：告诉它命令跑在沙箱里、工作区与缓存目录可写、区外只读；并且**明说 `/tmp` 每次调用都是新的**，跨命令不会保留。

### §9 文档与决定记录

- 新增 **`docs/sandbox.md`**（决策地图，形状同 `docs/bash.md`：形状表、边界、代码住哪）。
- 新增 **ADR 0006**，记默认行为与 fail-closed 的取舍。
- 改 **README**：《安全模型》里「这不是沙箱」那段重写；「明确不做」整节改名「**这一版不做**」（它混了「有证据支撑不做」与「这一版不做」两类，spec 的 `Out of Scope` 里本来就分着）。
- 改 **`docs/bash.md`**：`bash` 不包含什么——那条「进程级沙箱」删掉，指向 `docs/sandbox.md`。
- 改 **`docs/credentials.md`**：(d) 出网那一行补一句「进程级沙箱只管文件，不解决出网」。
- 改 **`CONTEXT.md`**：加词条「沙箱（Sandbox）」，写明它管文件、判据在内核（挂载表）、**不管网络**，与「权限模式」「断路器」是三件不同的事。

## Testing Decisions

三层，与仓库先例一致（`TestBackend` + pty 脚本 + 手工清单）：

1. **纯函数（主力）**——`wrap()` 的输出**逐字断言**：flag 顺序、每个可写根一条 `--bind`、遮罩目录的 `--tmpfs` + `--remount-ro` 成对、保护路径在 `--bind` **之后**、不存在的目录/文件被跳过、`mode = "off"` 时退化成单位函数。这一层不需要真有 bubblewrap，覆盖所有分支。
2. **集成**——沙箱不可用时是**工具错误**而不是命令结果；事件流里那条 log-only 事件写对了；`bwrap: ` 前缀被认出来、命令自己的非零退出不被误认。
3. **真机（手工清单 / pty）**——只有这一层能验「内核真的拦住了」：写工作区成功、写 `$HOME` 报只读、`cat ~/.config/fs-agent/config.toml` 读不到、`echo x > .env` 失败、`cargo test` 能跑。加进 `docs/tui-manual-checklist.md` 那一类清单。

## Out of Scope

**这一版不做**（不是永久决定，是范围边界）：

- **越界之后的一次性审批**（拒绝 → 原样重试 + 一句理由 → 一次批准 → `allowed-once`）。形状已经有了（[01 §⑦](research/01-dsh-workspace-permissions-and-shell.md)），但它要建立在「沙箱能稳定给出拒绝信号」之上，是下一个 effort。
- **网络隔离**：`--unshare-net`、域名白名单代理，都不做。这一版解决不了「把密钥发到网上」。
- **macOS / Windows / 鸿蒙**：只做 Linux。macOS 可行（Seatbelt 能表达同样语义）但要写 SBPL 生成；Windows 三条路线都要求改宿主 ACL；鸿蒙第一层就没有可用机制。
- **provider 抽象**：不做平台候选链、不做后端 trait、不做 `enforcement` 上报。
- **权限模式的改动**：`readonly` / `ask` / `auto` 三档不动。`workspace` 模式另开 effort（[`.scratch/workspace-mode/`](../workspace-mode/seed.md)）。
- **自动降级 / `--no-proc` 之类的退路**：不可用就是 fail closed。

## Further Notes

- **小节标题沿用现有 spec 的英文锚点**（`## Problem Statement` 这一批），因为把 `.scratch` 的小标题中文化是另一张票（`.scratch/language-migration/issues/04-tracker-headings-in-chinese.md`，状态 `ready-for-agent`）的范围，它有自己定好的译名表。正文散文一律中文。
- **这一版的真正价值不在安全，在失败点**：bubblewrap 自己的 `SECURITY.md` 写着它不是安全边界。它约束的是「程序别乱来」，给 agent 的误操作一个可靠的、内核给的失败点——这与仓库里「断路器存在是为了拦住事故，不是为了圈禁对手」是同一个立场。
- **五份调研**：[01](research/01-dsh-workspace-permissions-and-shell.md)（DSH）、[02](research/02-linux-sandbox-primitives-and-tools.md)（Linux 机制与工具）、[03](research/03-agent-sandbox-precedents.md)（同类产品）、[04](research/04-local-probe-bwrap-and-landlock.md)（本机实测与一条方法论警告）、[05](research/05-platform-portability-macos-windows-harmonyos.md)（平台可移植性）。
- **两份图解**：[eli5-sandbox.html](eli5-sandbox.html)、[eli5-bubblewrap.html](eli5-bubblewrap.html)（给人看的，不是一手材料）。

## Landing Notes (2026-10-01)

五张票同日落地，全部 `done`。落点：票 01 给了 `tools/sandbox.rs` 的 `wrap()` 纯函数与
`tests/sandbox.rs`；票 02 给了 `config.rs` 的 `[sandbox]` 节、探测、fail closed 与组装期注入；
票 03 把 `Sandbox` 接进 `process::run` 并区分「沙箱没起来」与「命令失败」；票 04 给了 `bash`
工具描述里那句说明与 `EventPayload::SandboxStatus`（log-only）；票 05 是真机清单
（[`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) ⑱）与这次收尾。

**与规格的四条偏差**（都是实现时才暴露的，逐条记理由）：

1. **探测 profile 多了 `--proc /proc`。** §3 写的最小 profile 里没有它，但同一条的第 4 点要求
   「`/proc` 建不起来也算不可用」；两条要一起满足，`--proc /proc` 就得进 profile。
2. **`--tmpfs /tmp` 挪到了所有 `--bind` 之前。** §1 的 flag 顺序把它排在 `--bind` 之后，真机
   一跑就露馅：挂载是叠上去的，一个落在 `/tmp` 下的会话工作区会被那条 tmpfs 整个盖掉，写自己
   的文件也报「只读文件系统」。顺序改对之后复验通过（⑱ 第 3 条）。
3. **失败文案没有进 `render/wording.rs`。** 票 04 第 4 条想收在那里，但那一层的模块声明写着
   「模型可见 / 进流的文本不在这里」—— 沙箱的工具错误与工具描述正是模型可见文本，所以它们与
   `bash` 的其它结果文本一样写在产生它们的模块（`tools/sandbox.rs`、`tools/bash.rs`），
   `wording.rs` 只收给人看的短语。
4. **探测的 `PATH` 经 `SessionConfig` 注入，不在 `lib` 里读环境。** 库不读进程环境的规矩照旧：
   `Config::resolve` 从它本来就拿到的那张环境表里取出 `PATH`，随 `[sandbox]` 配置进
   `SessionConfig`，组装期用它探一次（`OnceLock` 缓存，`mode = "off"` 或调用方已给结果时根本不
   探）。测试因此能从同一个口子注入假的 `PATH`。

**`git` 那条的实测结论**（⑱ 第 8 条，2026-10-01，临时仓库）：把 `.git/config` 与 `.git/hooks`
压回只读之后，`git status` / `git add` / `git commit` / `git log` 全部正常；`git config
user.name hack` 被拒，报的是 `无法写入配置文件 .git/config: 设备或资源忙`（EBUSY），而写
`.git/hooks/pre-commit` 报的是「只读文件系统」。两者都写在清单里，是因为**拒绝消息不是判据**
（§6）——判据只有 `bwrap: ` 前缀一条。

**收尾时补记的两条边界**（code-review 之后定的，都不是 bug，是取舍）：

5. **`SessionConfig::new()` / `Default` 的缺省是「这一层关着」**（`SandboxSettings::off()`）。
   `Config::resolve` 的缺省仍然是 `bwrap`（ADR 0006 的「默认开」），而 `cli` 的每一条组装路径
   都经 `Config::session_config` 拿到那一份 —— 于是生产路径上默认开、库内直接构造出来的会话
   （测试、以及别的调用方）不会凭空去跑一个探测。代价是：将来某条新的组装路径若忘了经
   `Config::session_config`，它会静默地没有沙箱。这是刻意换来的（否则每个直接构造
   `SessionConfig` 的测试都会去真探一次，而没有 `bwrap` 的 CI 上会 fail closed）。
6. **`.env` 家族只扫工作区顶层**（`tools/sandbox.rs` 的 `env_files`）。递归遍历一个可能很大的
   工作区要在每次 `bash` 调用前做一遍，不在预算里；所以 `sub/.env` 这样的嵌套文件 shell 写得
   进去。§4 说的「清单与地板对齐但不等同、而且比地板窄」在这里再窄一处，边界写在
   [`docs/sandbox.md`](../../docs/sandbox.md) 的「压回只读」那一段。

7. **沙箱状态在转录里也占一行**（code-review 之后先补到复盘视图，随后按用户的要求补齐到
   对话）。§8 的「界面不显示」说的是 TUI **左栏与状态行**——常驻 UI，不值当；但转录不是：
   `SandboxStatus` 现在与 `[上下文注入：…]` 同一档，`Block::Sandbox` 由 plain 与 TUI 两位
   画家各画一行暗色叙述（headless 也写一行到 stderr 诊断），`sessions show` 与 `--json` 里
   同样有它（`observe::Entry::Sandbox` + `wording::sandbox()`）。模型那一侧仍然没有
   （projection 是空分支）：它在沙箱里的信息一直来自 `bash` 工具描述那句常量。

**测试分布**：`tests/sandbox.rs` 29 条（纯函数逐字断言、配置、探测、`process::run` 的接缝、
经 `assemble` 的端到端，以及一条在真 `bwrap` 上跑的真机回归 —— 没有 `bwrap` 的机器上自动跳过）；
`tests/config_profiles.rs` 3 条配置解析；另外 `tests/cancellation.rs` 与
`tests/e2e_single_turn.rs` 里那两条「事件流恰好是这些」的断言按新骨架更新（多一条 `SandboxStatus`）。
