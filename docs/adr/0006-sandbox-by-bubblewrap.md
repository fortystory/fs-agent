# 让 shell 的写边界由内核担保：默认开启的 bubblewrap 沙箱

`bash` 与动态工具从此跑在 **bubblewrap** 里：`--ro-bind / /` 把整台机器只读挂进来，只有会话工作区、`/tmp` 与一列可配置的工具缓存目录可写，越界由内核以 `EROFS` 打回。它在 `src/tools/process.rs` 那唯一一处 spawn 上包一层 argv，抽出的是纯函数 `wrap()`。

这**改写 README《安全模型》里那段「这不是沙箱」**，也把 `docs/bash.md` 的「`bash` 不包含什么」第一条删掉。来源是 2026-10-01 的一轮 grilling（Q1–Q24），完整决定在 [`.scratch/sandbox/spec.md`](../../.scratch/sandbox/spec.md)，一手材料在 [`.scratch/sandbox/research/`](../../.scratch/sandbox/research/)（五份调研、含本机实测）。

## 为什么判据只能来自内核

`bash` 是权限门唯一管不住的工具：别的工具有写集合，`write_file` 的路径被限制在会话 cwd 内，`.env` 家族与 `.git` / `.ssh` 是降不下去的拒绝地板；而 shell 的 `effect()` 恒为 `Exclusive`——「它能写任何东西」。于是门只能决定跑不跑，决定不了写哪里。`auto` 档下 `echo x > .env` 没有任何东西拦得住；`ask` 档下唯一的防线是问用户，而用户没法从一条命令字符串里可靠判断它会不会越界。

替代方案是**猜 argv**，调研里没有一家这么做（[03 §①](../../.scratch/sandbox/research/03-agent-sandbox-precedents.md)），理由也是结构性的：变量展开、`eval`、解释器、写个脚本再执行，都能让静态形状与真实行为脱钩。猜出来的东西既不是拒绝也不是允许，还会给人一个「已检查」的假观感。

所以这一层的价值与「断路器」不同：断路器是拦住事故的启发式，**沙箱是内核给的判定**。两者的共同点是都不假装自己是对手级别的边界——bubblewrap 自己的 `SECURITY.md` 就写着它不是安全边界。它约束的是程序别乱来，给误操作一个可靠的失败点。

## 为什么默认开，而且 fail closed

这是本 ADR 里最可能被后来者质疑的一条，所以把取舍写明：

- **默认开**，是因为它只在「有可用的 bubblewrap」时才生效，而它保护的是 `auto` 档下**已经不问用户**的那些命令。默认关等于把这条防线只留给会读文档的人。
- **探测不到就 fail closed**（拒绝跑 shell，返回工具错误并给两条出路：装 bubblewrap，或显式把 `mode` 设成 `"off"`），因为另外两个选项都更差：**静默放行**会让「沙箱」这个词变成谎话；**降级为「一律问」**看起来在保护，实际上「区外」这个判据已经消失，用户面对的是比现在更多的打断而没有更少的风险。
- 代价是明确的：**Ubuntu 24.04 用户开箱即用不了 `bash`**。那里 `kernel.apparmor_restrict_unprivileged_userns=1` 由发行版补丁默认打开，而 bubblewrap 需要能带 capability 的 user namespace（[02 §②](../../.scratch/sandbox/research/02-linux-sandbox-primitives-and-tools.md)）。出路是他们在 profile 里放行 `bwrap`，或者把 `mode` 设成 `"off"`——两条都是显式动作，这正是 fail closed 想要的效果。
- **探测在组装期做一次，且要真跑一条最小 profile**：装了 bubblewrap 不等于它在这个环境里能用。

## 为什么网络不在这一层

bubblewrap 的 `--unshare-net` 能隔离网络，但加上之后 `cargo build` / `npm install` 会一起断——实测连 `1.1.1.1:443` 都不通。文件与网络是两套判据、两种代价，混在一版里会让两边都不可控，所以网络隔离单独排期。

**因此要如实说：README 里「(d) 出网：什么都没有」这一版没有改善。** 沙箱只管文件，`curl` 带着 key 出去仍然分不出来。不要因为「我们做了沙箱」就以为这条被堵上了——`docs/credentials.md` 照旧写着那句话，只补一句说明。

## 后果

- **`bash` 的行为变了，而且是在默认档位下。** `auto` 档下写工作区外的命令从此失败（内核 `EROFS`），包括一些以前能跑的：写 `~/.npm`、写 `~/.aws`、`git push`（`~/.ssh` 被遮住）、往 `~/.config` 里写配置。可写根可配，遮罩与保护路径不可配。
- **`git` 仍然能提交，但改不了 hooks 与 remote。** 一开始照 Codex 的做法把整个 `.git` 压回只读，实测下来 `git add` / `git commit` 全线失败——它们写的第一样东西就是 `.git/index.lock`；改成只压回 `.git/config` 与 `.git/hooks` 之后，提交正常，而这两个真正危险的入口仍然进不去。
- **`/tmp` 每次调用都是新的。** 同一条命令内可用，跨命令不保留，也不会写到宿主上——模型需要知道这一点，所以这句话进了 `bash` 的工具描述。
- **`~/.config/fs-agent/config.toml` 与 `~/.ssh` 在 shell 里变成「存在但是空的、且只读」**。这堵掉了一部分「模型把 key 读出来」的路径，但不堵出网。
- **多了一处会拒绝工作的开关。** 沙箱不可用时 `bash` 与动态工具是**工具错误**，不是命令失败；判据只有 `bwrap: ` 前缀一条（被沙箱拒绝的命令，其消息随 locale 变，不能拿来做判据）。
- **模型知道自己在沙箱里**（写在 `bash` 工具描述里），所以它不会对着区外反复试；沙箱状态进事件流（log-only），replay 能重算出某条命令当时有没有被关着。
- **为 macOS 留了位置，但只留了一条缝。** 抽出的 `wrap()` 纯函数让将来加 Seatbelt 是「加一个分支」而不是重构；不预做 provider 抽象，因为真做过这件事的项目一个停更、一个几乎没人用（[05 §⑤](../../.scratch/sandbox/research/05-platform-portability-macos-windows-harmonyos.md)）。Windows 与鸿蒙不是「位置」问题：前者三条路线都要求改动宿主 ACL，后者第一层就没有可用机制。
- **`README`「明确不做」那一节同时改名为「这一版不做」。** 它把 spec 里分着的两类（有证据支撑不做 / 这一版不做）混在一起，读起来像永久判决——这次的沙箱本来就在那份清单上，正是这个措辞问题暴露出来的。
