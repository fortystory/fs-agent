# research：Linux 进程级文件沙箱的机制、工具与可用性前提

## 这份文件是什么

上一份（[01-dsh-workspace-permissions-and-shell.md](./01-dsh-workspace-permissions-and-shell.md)）清点了 **DSH 怎么用**这些机制：Linux 上 `bwrap` 优先、`landlock-run` 兜底，越界靠内核报错、模型带 `sandbox_permissions` 重试一次、失败关闭。**那份文件把机制当既成事实用，没有回答机制本身在 Linux 上到底能做什么、需要什么前提、在哪些发行版上会哑火。**

这份文件补那一层：**Linux（以及必要处的 macOS）做进程级文件沙箱，现成的机制与工具各是什么、真实能力边界在哪、无特权可用的前提是什么。** 读者是接下来要拍「要不要做进程级沙箱、做哪一条」的人。

规矩照 [01](./01-dsh-workspace-permissions-and-shell.md)：① 到 ⑧ 是事实清点（每条 claim 后面给一手来源），推论单独标注「**推论**」；**不写设计建议**；⑨ 写清楚这份文件回答不了什么。

## 来源与版本与观察时点

- **观察时点：2026-09-30**（前一日 2026-09-29 有 Ubuntu 26.04.1 的发布公告，见 [Ubuntu 官方博客](https://ubuntu.com/blog/upgrade-your-desktop-ubuntu-26-04-lts)）。版本相关的结论都钉版本号，不写「最新版」。
- **本机环境（用于「本机观测」类证据）**：Arch Linux，`uname -r` = `7.2.6-arch2-1`，bubblewrap **0.13.0**，systemd **262**，util-linux **2.42.4**，Docker **29.8.1**，Rust 工具链下的 `nix` 缓存为 **0.29.0**、`rustix` 为 **1.1.4**。本机 `/sys/kernel/security/lsm` = `capability,landlock,lockdown,yama,bpf`（**没有 `apparmor`**）。
- **本机是只读挂载环境**：`findmnt /` 显示 `/dev/nvme0n1p2[/@] btrfs ro,nosuid,nodev,...`。这一点很重要——**本机对 `/etc`、`/` 的写入失败（`只读文件系统`）可能来自挂载本身，不能归因于被考察的沙箱机制**；凡涉及本机实测的结论，正文会指出对照实验。
- **man page 是一手来源**，本机 `man-pages` 版本为 **6.19**（`man 7 landlock` 页脚 `Linux man-pages 6.19    2026-07-18`），行文里给 man7.org 的对应在线页。bwrap 的 man 用官方源 `bwrap.xml`（本机 man 与它同源，版本 0.13.0）。
- **内核文档与源码**引用 `torvalds/linux` 的 master（2026-09-30 抓取）与 docs.kernel.org。
- **引用格式**：`[标题或描述](URL)`；正文指出具体位置（flag 名、函数名、sysctl 名、章节名、常数名）。**凡是只能找到二手来源的，正文写明「二手来源，一手未验证」。**
- 复现本机实测的最小命令在 §②（bwrap）与 §③（Landlock）正文里给出。

---

## ① 结论先行

1. **「让 shell 在工作区里自动、区外要审批」在 Linux 上可达，而且有两条互不依赖的路**：一条是 **bubblewrap 造一个「整机只读挂进来、工作区 bind 成可写」的文件系统视图**（判据挂在 mount 上，写只读挂载返回 `EROFS`）；另一条是 **Landlock 给本进程及其后代挂一层 allow-list**（判据挂在 LSM 上，越界返回 `EACCES`）。两条都不需要 root，前提都是 **unprivileged user namespaces 可用**（bwrap 非 root 时必需）与 **no_new_privs**（Landlock 无特权时必需）。
2. **bubblewrap 不是安全边界，这是它自己的官方措辞，不是外人的评价。** `SECURITY.md` 原文：`bubblewrap is not a security boundary between the user and the OS, because anything bubblewrap could do, a malicious user could equally well do by writing their own tool equivalent to bubblewrap.`，以及 `bubblewrap is not a complete, ready-made sandbox with a specific security policy.`（[bubblewrap SECURITY.md](https://github.com/containers/bubblewrap/blob/main/SECURITY.md)）。它也不再支持 setuid：`Newer versions of bubblewrap refuse to operate if the binary has been made setuid.`（同处）。
3. **bwrap 限制的是「文件系统视图」，不是「原始宿主文件系统」。** 它总是新建 mount namespace，并把 root 放在一个宿主看不见的空 tmpfs 上，再用 `--ro-bind` / `--bind` / `--tmpfs` / `--dev` / `--proc` 把要看见的东西挂进去（`man bwrap(1)` DESCRIPTION 与各 OPTION；[bwrap.xml](https://github.com/containers/bubblewrap/blob/main/bwrap.xml)）。本机实测：`bwrap --die-with-parent -- /bin/sh` 连 `/bin/sh` 都找不到（空 root tmpfs），而 `bwrap --ro-bind / / ...` 之后能看到完整宿主根——**「看得到宿主」是 `--ro-bind / /` 这个参数造出来的，不是默认行为**。
4. **Landlock 的 ABI 版本是使用它的第一等约束，而且版本号跑得比 man page 快。** 本机 man-pages 6.19 的 ABI 表止于 **ABI 9 / Linux 7.1**（`LANDLOCK_ACCESS_FS_RESOLVE_UNIX`），但本机内核 7.2.6 实测 `landlock_create_ruleset(NULL,0,LANDLOCK_CREATE_RULESET_VERSION)` 返回 **10**；上游 `include/uapi/linux/landlock.h` 写明 ABI 10 加了 **UDP 网络限制** `LANDLOCK_ACCESS_NET_BIND_UDP` 与 `LANDLOCK_ACCESS_NET_CONNECT_SEND_UDP`（[landlock.h](https://github.com/torvalds/linux/blob/master/include/uapi/linux/landlock.h)，`Support added in Landlock ABI version 10`）。用法上必须**运行时问 ABI、再向下取子集**（best-effort），不能编译期假定。
5. **Ubuntu 23.10+ / 24.04 是 bwrap 最现实的一颗地雷，而且它是发行版补丁、不在上游。** Ubuntu 用 `kernel.apparmor_restrict_unprivileged_userns=1`（由 apparmor 包的 `/usr/lib/sysctl.d/10-apparmor.conf` 启用）把「创建 unprivileged user namespace」限制成「只有带 `userns,` 规则的 AppArmor profile 才准」（[Ubuntu 官方 spec SE045](https://discourse.ubuntu.com/t/spec-unprivileged-user-namespace-restrictions-via-apparmor-in-ubuntu-23-10/37626)、[Canonical 博客](https://ubuntu.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces)）。这个 sysctl **不在上游内核里**——`security/apparmor/lsm.c` 的 sysctl 表只有 `unprivileged_userns_apparmor_policy`、`apparmor_display_secid_mode`、`apparmor_restrict_unprivileged_unconfined`（[lsm.c](https://github.com/torvalds/linux/blob/master/security/apparmor/lsm.c)）。Ubuntu 24.04（noble）的 bubblewrap 包**不带** `/etc/apparmor.d/bwrap`（[包文件列表](https://packages.ubuntu.com/noble/amd64/bubblewrap/filelist)），于是 apt 装的 bwrap 会以 `bwrap: setting up uid map: Permission denied` 失败（[Launchpad #2144531](https://bugs.launchpad.net/ubuntu/+source/bubblewrap/+bug/2144531)）。**Debian 12 没有找到同款 AppArmor 限制的一手证据**（见 §⑨）：Debian 的机制是另一个 sysctl `kernel.unprivileged_userns_clone`，而 Debian 的 bubblewrap 包已经不再覆盖它、理由是「这个默认值已经好几年就是这样了」（Debian 条目见 Ubuntu 镜像的 changelog：`bubblewrap (0.11.1-1)`）。
6. **seccomp 单独做不了「路径前缀」判定，理由是结构性的。** 内核文档原话：滤波器只能看 `system call number and the system call arguments`，而 `BPF programs may not dereference pointers which constrains all filters to solely evaluating the system call arguments directly.`，并且明确写 `System call filtering isn't a sandbox.`（[seccomp_filter.rst](https://docs.kernel.org/userspace-api/seccomp_filter.html)）。Rust 封装的同一句话：`They don't allow you to filter by path name in open calls, or indeed any syscall arguments that are pointers`（[extrasafe README](https://github.com/boustrophedon/extrasafe)）。它在文件沙箱里的角色是**补漏**：拦住 `open_by_handle_at`（Docker 默认 profile 里它只在 `CAP_DAC_READ_SEARCH` 下才放行）、`mount`/`setns`/`unshare`（Docker 默认只在 `CAP_SYS_ADMIN` 下放行）这类绕过路径判定的口子（[moby 默认 seccomp profile](https://github.com/moby/profiles/blob/main/seccomp/default.json)）。
7. **namespaces 是被 bwrap 与「现成容器」共同使用的地基，而 `chroot` 明确不是安全边界。** `chroot(2)` 的官方措辞：`it is not intended to be used for any kind of security purpose, neither to fully sandbox a process nor to restrict filesystem system calls.`（[chroot(2)](https://man7.org/linux/man-pages/man2/chroot.2.html)）。无特权创建 user namespace 自 Linux 3.8 起不需要特权（[namespaces(7)](https://man7.org/linux/man-pages/man7/namespaces.7.html)），其余 namespace 的创建一般要 `CAP_SYS_ADMIN`。
8. **现成工具大致分三层，对「shell 在可写工作区里跑、区外只读」的贴合度递减**：**进程级**（bwrap、Landlock、nsjail、firejail、`systemd-run --user` + systemd 的 `ReadOnlyPaths=`/`ReadWritePaths=`、rootless 容器）、**容器级**（rootless Podman/Docker、gVisor 的 `runsc`）、**虚机/WASM 级**（Firecracker、Kata、wasmtime）。后两层的代价不是「更安全」而是「更重」：虚机级要 KVM 与 guest 内核，WASM 级要求目标是**编译成 wasm 的程序**而不是宿主上的 `bash`。
9. **Rust 侧最贴合这个需求的是 `landlock` crate（纯 Rust、无需外部 C 库、`pre_exec` 友好），跨平台的 `birdcage` 存在但要么受限、要么已停更**：`birdcage` 0.8.1 最后发布于 **2024-04-19**（[crates.io API](https://crates.io/api/v1/crates/birdcage)），仓库已从 `phoenixkahlo` 转到 `phylum-dev`；它 **Linux 走 namespaces、macOS 走 `sandbox_init()`（Seatbelt）**，自陈 `It is not a complete sandbox preventing all side-effects or permanent damage ... Applications can still execute most system calls, which is especially dangerous when execution is performed as root.`（[birdcage README](https://github.com/phylum-dev/birdcage)）。

---

## ② bubblewrap（`bwrap`）

**官方仓库**：[containers/bubblewrap](https://github.com/containers/bubblewrap)（原属 projectatomic，Debian 的 changelog 里记录了 URL 迁移，见 `bubblewrap (0.4.1-1)` 条目：`Update various URLs from https://github.com/projectatomic/bubblewrap to https://github.com/containers/bubblewrap`）。

### 定位：无特权的「低层沙箱启动器 / 容器搭建工具」

`man bwrap(1)` 的第一句（[bwrap.xml](https://github.com/containers/bubblewrap/blob/main/bwrap.xml)）：

> `bwrap is an unprivileged low-level sandboxing tool. You are unlikely to use it directly from the commandline, although that is possible.`

README 的 `User namespaces` 一节：`There is a feature in the Linux kernel called user namespaces which allows unprivileged users to use container features. Bubblewrap uses these to build the sandbox, allowing any user to use the tool.`，并紧接一句 `Historically, bubblewrap also supported a setuid mode for systems where unprivileged user namespaces were not supported. However, this has been removed.`（[README](https://github.com/containers/bubblewrap/blob/main/README.md)）。

### 它限制的是「文件系统视图」，不是「原始宿主文件系统」

`man bwrap(1)` DESCRIPTION：它 `works by creating a new, completely empty, filesystem namespace where the root is on a tmpfs that is invisible from the host`，然后由命令行选项构造 root 与进程环境（同上）。也就是说「宿主文件系统」没有被改动，被改动的是**新 mount namespace 里的挂载表**。

**本机观测（Arch，bwrap 0.13.0，2026-09-30）**，用来把这句话钉实：

```sh
# 1) 什么都不 bind：root 是空 tmpfs，连 /bin/sh 都不存在
$ bwrap --die-with-parent -- /bin/sh
bwrap: execvp /bin/sh: No such file or directory

# 2) --ro-bind / / 之后看到的是完整宿主视图（只读）
$ bwrap --ro-bind / / --die-with-parent -- /bin/sh -c 'ls / | head -3; cat /etc/os-release | head -1'
bin
boot
dev
NAME="Arch Linux"

# 3) 写宿主根下的任何位置：只读挂载把它打回 EROFS
$ bwrap --ro-bind / / --die-with-parent -- /bin/sh -c 'touch /etc/PROBE'
touch: 无法 touch '/etc/PROBE': 只读文件系统

# 4) 换成 --tmpfs /tmp：/tmp 变成可写的私有 tmpfs
$ bwrap --ro-bind / / --dev /dev --proc /proc --tmpfs /tmp --die-with-parent --unshare-pid -- /bin/sh -c 'echo HELLO > /tmp/x && cat /tmp/x'
HELLO
```

**结论（事实）**：文件系统隔离的载体是 **mount namespace**，因此「区外只读」在 shell 里表现为 `read-only file system`（`EROFS`），这与 [01](./01-dsh-workspace-permissions-and-shell.md) §③④里记的 DSH bwrap 拒绝方言 `read-only file system` 是同一个来源。

### 典型 flag 组合

以下全部来自 `man bwrap(1)`（[bwrap.xml](https://github.com/containers/bubblewrap/blob/main/bwrap.xml)），括号里是该 man 页的选项名：

- 文件系统视图：`--ro-bind SRC DEST`（`Bind mount the host path SRC on DEST readonly`）、`--bind SRC DEST`、`--dev-bind`（允许设备访问）、`--ro-bind-try`/`--bind-try`（SRC 不存在则忽略）、`--remount-ro DEST`、`--tmpfs DEST`、`--proc DEST`、`--dev DEST`、`--bind-data FD DEST`/`--ro-bind-data FD DEST`。
- overlay 挂载（较新）：`--overlay-src`、`--overlay RWSRC WORKDIR DEST`、`--tmp-overlay DEST`（写落到 tmpfs，不持久）、`--ro-overlay DEST`；`--ro-overlay` 或多于一个 `--overlay-src` 需要较新内核（man 里给出内核版本脚注）。
- namespace：`--unshare-user`、`--unshare-user-try`、`--unshare-ipc`、`--unshare-pid`、`--unshare-net`、`--unshare-uts`、`--unshare-cgroup`、`--unshare-cgroup-try`、`--unshare-all`（`Currently equivalent with: --unshare-user-try --unshare-ipc --unshare-pid --unshare-net --unshare-uts --unshare-cgroup-try`）、`--share-net`、`--userns FD`、`--userns2 FD`。
- **`--disable-userns`**：`Prevent the process in the sandbox from creating further user namespaces, so that it cannot rearrange the filesystem namespace or do other more complex namespace modification. This is currently implemented by setting the user.max_user_namespaces sysctl to 1, and then entering a nested user namespace which is unable to raise that limit in the outer namespace. This option requires --unshare-user.`（同 man 页；`user.max_user_namespaces` 的官方定义见 [sysctl/user.rst](https://docs.kernel.org/admin-guide/sysctl/user.html)）
- 生命周期与会话：`--die-with-parent`（父死则沙箱死，见下）、`--new-session`（`man` 写明它防 `TIOCSTI` 一类「向沙箱外的终端注入命令」，与 CVE-2017-5226 相关）、`--chdir DIR`、`--clearenv`、`--setenv`、`--uid`/`--gid`、`--hostname`。
- capability 与 seccomp：`--cap-add CAP`、`--cap-drop CAP`、`--seccomp FD`（`man` 只说可以传 seccomp filter，不负责生成）。

`--die-with-parent` 的意义：man 页写它 `Ensures child process (COMMAND) dies when bwrap's parent dies. Kills all bwrap's descendants.`——**推论**：这类 flag 处理的是「沙箱进程的生命周期」，不是文件系统权限；它不改变区外只读这件事。

### 依赖什么内核能力

- **mount namespace：总是创建。** `man bwrap(1)`：`By default, bwrap creates a new mount namespace for the sandbox.`（README 同：`bubblewrap always creates a new mount namespace`）。
- **user namespace：非 root 时必需。** 同一句 man：`Optionally it also sets up new user, ipc, pid, network and uts namespaces (but note the user namespace is required if bwrap is not run as root).`；README 的 `Sandboxing` 一节把 `CLONE_NEWUSER` / `CLONE_NEWIPC` / `CLONE_NEWPID` / `CLONE_NEWNET` / `CLONE_NEWUTS` 各自的好处列了一遍（[README](https://github.com/containers/bubblewrap/blob/main/README.md)）。
- 其余 namespace 的创建在 Linux 上一般要 `CAP_SYS_ADMIN`（[namespaces(7)](https://man7.org/linux/man-pages/man7/namespaces.7.html)：`Creation of new namespaces using clone(2) and unshare(2) in most cases requires the CAP_SYS_ADMIN capability ... User namespaces are the exception: since Linux 3.8, no privilege is required to create a user namespace.`）。在 user namespace 内部，进程持有该 namespace 的完整 capability 集，因此可以在其中 mount（[user_namespaces(7)](https://man7.org/linux/man-pages/man7/user_namespaces.7.html)，`Effect of capabilities within a user namespace`）。

**推论**：bwrap 的可移植性等价于「这台机器允不允许无特权 user namespace」，而不是「装没装 bwrap」。

### 在哪些常见发行版/环境上会不可用

**(a) Ubuntu 23.10+ / 24.04：AppArmor 的 userns 限制。** 这是**发行版补丁**，分四层证据：

1. **官方 spec**（Canonical 安全工程师 Alex Murray，SE045，2023-08-07；[discourse](https://discourse.ubuntu.com/t/spec-unprivileged-user-namespace-restrictions-via-apparmor-in-ubuntu-23-10/37626)）：`These patches introduce a new sysctl named kernel.apparmor_restrict_unprivileged_userns which is used to enable / disable AppArmor enforcement of user namespace restrictions at runtime.`；启用方式是在 apparmor 包里放 `/usr/lib/sysctl.d/10-apparmor.conf`，内容含 `kernel.apparmor_restrict_unprivileged_userns = 1`。同文指出「LSM 对 user namespace 的 mediation 在 6.1 进了上游，但让 AppArmor 用上它所需的改动**还没有上游**（`are not yet upstream`），因此 Ubuntu 内核需要 SAUCE patches」。Canonical 博客补了时间线：23.10 发布当日该特性是 opt-in（`On release day, the feature will be opt-in and you will be able to turn it on using the command line.`），随后通过 SRU 改成默认开启（`we will then turn it on by default, on 23.10, using the SRU process.`）（[Canonical 博客](https://ubuntu.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces)）。
2. **上游内核里确实没有这个 sysctl**：`security/apparmor/lsm.c` 的 `apparmor_sysctl_table[]` 只有 `unprivileged_userns_apparmor_policy`、`apparmor_display_secid_mode`、`apparmor_restrict_unprivileged_unconfined`（[lsm.c](https://github.com/torvalds/linux/blob/master/security/apparmor/lsm.c)，master，2026-09-30）；`Documentation/admin-guide/sysctl/kernel.rst` 里 grep 不到 `apparmor_restrict_unprivileged_userns`。上游有的是 `userns_create` LSM hook（同文件 `LSM_HOOK_INIT(userns_create, apparmor_userns_create)`）。
3. **游客侧的现象**：Launchpad #2144531（2026-03-16 报，Ubuntu 24.04，bubblewrap `0.9.0-1ubuntu0.1`）标题即 `bubblewrap fails to set uid_map under kernel.apparmor_restrict_unprivileged_userns=1 (Missing AppArmor profile)`，报错逐字为 `bwrap: setting up uid map: Permission denied`；报告人给的出路是给 bwrap 加 `profile bwrap /usr/bin/bwrap flags=(unconfined) { userns, ... }`（[bug #2144531](https://bugs.launchpad.net/ubuntu/+source/bubblewrap/+bug/2144531)，已被 #2069526 标为重复）。
4. **Ubuntu 24.04 的 bubblewrap 包不带这个 profile**：noble 的 `bubblewrap` 文件列表只有 `/usr/bin/bwrap`、`/usr/lib/sysctl.d/50-bubblewrap.conf`、补全脚本、文档与 man page，**没有 `/etc/apparmor.d/bwrap`**（[packages.ubuntu.com/noble/amd64/bubblewrap/filelist](https://packages.ubuntu.com/noble/amd64/bubblewrap/filelist)）。而 noble 的 `apparmor` 包**有** `/usr/lib/sysctl.d/10-apparmor.conf`（[packages.ubuntu.com/noble/amd64/apparmor/filelist](https://packages.ubuntu.com/noble/amd64/apparmor/filelist)）——即默认开着限制、却没给 bwrap 开例外。

Chromium 的项目文档把这个现象描述得很清楚（**这是 Chromium 项目的一手文档，不是 Ubuntu 官方**）：`Our primary sandbox no longer works on developer builds on some Linux distributions, namely Ubuntu, due to a security feature that restricts access to a powerful kernel feature, user namespaces.`，并给出两种绕过：全局 `echo 0 > /proc/sys/kernel/apparmor_restrict_unprivileged_userns`，或按路径写 `flags=(unconfined) { userns, }` 的 AppArmor profile（[Chromium: AppArmor User Namespace Restrictions](https://chromium.googlesource.com/chromium/src/+/main/docs/security/apparmor-userns-restrictions.md)）。AppArmor 上游的规则语法 `userns`（权限 `create`）在 `apparmor.d(5)` 的语法定义里，见 `USERNS RULE = [ QUALIFIERS ] 'userns' [ USERNS ACCESS PERMISSIONS ]` 与示例 `userns,` / `userns create,`（[apparmor.d.pod](https://gitlab.com/apparmor/apparmor/-/blob/master/parser/apparmor.d.pod)）。

**(b) 任何把 `kernel.unprivileged_userns_clone` 设为 0 的机器。** 这个 sysctl 同样不是上游的，Canonical 的 spec 明确列出「若干发行版内核（Ubuntu、[Debian](https://salsa.debian.org/kernel-team/linux/-/blob/master/debian/patches/debian/add-sysctl-to-disallow-unprivileged-CLONE_NEWUSER-by-default.patch)、[Arch](https://github.com/archlinux/linux/commit/d73ae82da61377a3f89ec5eded369fd186cbd165)）带了一个未被上游接受的补丁，允许用 sysctl 关掉无特权 user namespace」。**推论**：这类机器上 bwrap 会以「无法创建 user namespace」失败，表现与 (a) 不同但结果相同。

**(c) Debian 12：没有找到 AppArmor 那种限制的一手证据，而 sysctl 的默认值是「允许」。** 找到的 Debian 事实是：
- Debian 的 `apparmor` 在 bookworm 是 **3.0.8-3**（[Debian 官方 changelog](https://metadata.ftp-master.debian.org/changelogs/main/a/apparmor/apparmor_3.0.8-3_changelog)，`apparmor (3.0.8-3) unstable; urgency=medium  * Cherry-pick a few small, targeted fixes from upstream 3.0 branch`），该 changelog 里 grep 不到 `userns` / `user namespace`；而 `userns` 规则出现在 AppArmor 的 master（4.x）语法里（见 (a) 的 apparmor.d.pod）。
- Debian 的 bubblewrap 包在 `0.11.1-1` 停止覆盖 `kernel.unprivileged_userns_clone`，理由逐字：`Stop overriding kernel.unprivileged_userns_clone sysctl. The setting we use has been the default for several years.`（Debian changelog 条目，出现在 Ubuntu 的 changelog 文件里：`bubblewrap (0.11.1-1) unstable; urgency=medium`，[changelogs.ubuntu.com](https://changelogs.ubuntu.com/changelogs/pool/main/b/bubblewrap/bubblewrap_0.11.1-1ubuntu0.3/changelog)）。
- **推论（不是文档原话）**：Debian 12 上 bwrap 的 userns 默认可用；任务里说的「Debian 12 的 AppArmor 限制」在我能拿到的来源里没有对应物。这条留到 §⑨。

**(d) 默认的 Docker 容器里。** Docker/moby 的默认 seccomp profile（`defaultAction: SCMP_ACT_ERRNO`）里，`unshare`、`setns`、`mount`、`clone3`、`bpf` 等**只在进程带 `CAP_SYS_ADMIN` 时**才 `SCMP_ACT_ALLOW`；`clone` 另有一条「无 CAP_SYS_ADMIN」的规则，它要求 `args[0]` 满足掩码 `value: 2114060288`（`0x7E020000`，即 `SCMP_CMP_MASKED_EQ`）（[moby/profiles/seccomp/default.json](https://github.com/moby/profiles/blob/main/seccomp/default.json)）。**推论（从上面这条事实推出，不是文档原话）**：默认容器（无 `CAP_SYS_ADMIN`）里 bwrap 拿不到 `unshare`/`clone` 的自由，需要 `--cap-add SYS_ADMIN`、自定义 seccomp profile 或 `--privileged` 之类才能跑。

**(e) 本机 Arch（对照）**：`bwrap --version` = 0.13.0，`--ro-bind / /`、`--tmpfs /tmp`、`--unshare-all`、`--die-with-parent` 均正常工作（见上面的实测），LSM 列表里没有 apparmor。

**(f) RHEL/Fedora 未查**——写进 §⑨。

### setuid 还是无特权

- 现状：**无特权**，并且**拒绝以 setuid 运行**。`SECURITY.md` 原文：`Older versions of bubblewrap were optionally setuid root. This is a system security risk. ... Newer versions of bubblewrap refuse to operate if the binary has been made setuid.`（[SECURITY.md](https://github.com/containers/bubblewrap/blob/main/SECURITY.md)，并链接到 [v0.11.2 的历史说明](https://github.com/containers/bubblewrap/blob/v0.11.2/SECURITY.md#system-security)）。
- README 说 setuid 模式已移除（见上）。Debian changelog 里还留着历史教训：`Fixes a root privilege escalation vulnerability introduced in 0.4.0, in cases where the kernel allows creation of user namespaces by unprivileged users and bwrap is (unnecessarily) setuid root.`（`bubblewrap (0.4.1-1)` 条目，CVE-2020-5291 / GHSA-j2qp-rvxj-43vj，同 changelog URL）。

### 自陈的逃逸面与「不是安全边界」

原样引用（[SECURITY.md](https://github.com/containers/bubblewrap/blob/main/SECURITY.md)）：

> `bubblewrap is not a security boundary between the user and the OS, because anything bubblewrap could do, a malicious user could equally well do by writing their own tool equivalent to bubblewrap.`

> `bubblewrap is a toolkit for constructing sandbox environments. bubblewrap is not a complete, ready-made sandbox with a specific security policy.`

> `As a result, the level of protection between the sandboxed processes and the host system is entirely determined by the arguments passed to bubblewrap.`（并明确把安全模型的责任推给调用者：`Whatever program constructs the command-line arguments for bubblewrap ... is responsible for defining its own security model`）

README 的 `Limitations` 一节列了需要特别小心的点（[README](https://github.com/containers/bubblewrap/blob/main/README.md)）：

- `If you are not filtering out TIOCSTI commands using seccomp filters, argument --new-session is needed to protect against out-of-sandbox command execution (see CVE-2017-5226).`
- `Everything mounted into the sandbox can potentially be used to escalate privileges. For example, if you bind a D-Bus socket into the sandbox, it can be used to execute commands via systemd.`（建议用 xdg-dbus-proxy 过滤 D-Bus）
- 挂进沙箱的 seccomp 约束可能反过来限制应用自己的沙箱：`if you limit the syscalls and don't allow the seccomp syscall, a browser cannot apply these restrictions`。

它还把 Flatpak 类型的 CVE 归给框架而不归给自己（`CVE-2017-5226 ... is considered to be a Flatpak vulnerability, not a bubblewrap vulnerability`），并直接对比 Firejail：`Firejail is similar to Flatpak before bubblewrap was split out in that it combines a setuid tool with a lot of desktop-specific sandboxing features.`，同段引用 `@cgwalters` 认为按路径白名单是坏主意：`the myriad ways users have to manipulate paths, and the myriad ways in which system administrators may configure a system`（同 README）。

---

## ③ Landlock LSM

**一手来源**：内核文档 [`Documentation/userspace-api/landlock.rst`](https://docs.kernel.org/userspace-api/landlock.html)（master，2026-09-30 抓取）、man pages [`landlock(7)`](https://man7.org/linux/man-pages/man7/landlock.7.html) / [`landlock_create_ruleset(2)`](https://man7.org/linux/man-pages/man2/landlock_create_ruleset.2.html) / [`landlock_add_rule(2)`](https://man7.org/linux/man-pages/man2/landlock_add_rule.2.html) / [`landlock_restrict_self(2)`](https://man7.org/linux/man-pages/man2/landlock_restrict_self.2.html)（本机 man-pages 6.19）、上游头文件 [`include/uapi/linux/landlock.h`](https://github.com/torvalds/linux/blob/master/include/uapi/linux/landlock.h)、官网 [landlock.io](https://landlock.io/)（作者 Mickaël Salaün）。

### 它是什么、限制什么

`landlock(7)` NAME 一行就是答案：`Landlock - unprivileged access-control`。DESCRIPTION：`Landlock is an access-control system that enables any processes to securely restrict themselves and their future children. Because Landlock is a stackable Linux Security Module (LSM), ...`，策略是 `a set of access rights (e.g., open a file in read-only, make a directory, etc.) tied to a file hierarchy`。三个 syscall：`landlock_create_ruleset(2)`、`landlock_add_rule(2)`、`landlock_restrict_self(2)`（同 man）。

landlock.rst 的目标表述与之一致，并强调 `Landlock empowers any process, including unprivileged ones, to securely restrict themselves.`（[landlock.rst](https://docs.kernel.org/userspace-api/landlock.html)）。

规则种类（man `landlock(7)`，`Landlock rules` 一节）：
- **Filesystem rules**：对象是文件层级，权限是文件系统访问权（见下）。
- **Network rules（since ABI v4）**：对象是 TCP 端口，权限是网络访问权。

### 文件系统访问权限集合（逐条）

man `landlock(7)` 的 `Filesystem actions` 一节（括号里是引入的 ABI）：

只作用于文件：
- `LANDLOCK_ACCESS_FS_EXECUTE`（ABI 1）：执行文件。
- `LANDLOCK_ACCESS_FS_WRITE_FILE`（ABI 1）：以写打开文件（man 提醒通常还要 `TRUNCATE`，因为 `creat(2)` 之类会顺手截断）。
- `LANDLOCK_ACCESS_FS_READ_FILE`（ABI 1）：以读打开文件。
- `LANDLOCK_ACCESS_FS_TRUNCATE`（**ABI 3**）：`truncate(2)`/`ftruncate(2)`/`creat(2)`/`open(O_TRUNC)`。
- `LANDLOCK_ACCESS_FS_IOCTL_DEV`（**ABI 5**）：对**已打开的字符/块设备**调 `ioctl(2)`；man 列出不受它管的一批常见 ioctl。
- `LANDLOCK_ACCESS_FS_RESOLVE_UNIX`（**ABI 9**）：查找 pathname UNIX domain socket；管 `connect(2)` 与带显式收件地址的 `sendmsg(2)`，拒绝时返回 `EACCES`。

作用于目录自身及之下：
- `LANDLOCK_ACCESS_FS_READ_DIR`（ABI 1）：打开目录或列目录。

只作用于目录内容（不管目录本身）：
- `LANDLOCK_ACCESS_FS_REMOVE_DIR`、`REMOVE_FILE`、`MAKE_CHAR`、`MAKE_DIR`、`MAKE_REG`、`MAKE_SOCK`、`MAKE_FIFO`、`MAKE_BLOCK`、`MAKE_SYM`（均 ABI 1）。
- `LANDLOCK_ACCESS_FS_REFER`（**ABI 2**）：跨目录 link/rename。**它是唯一一个「即使没有在 `handled_access_fs` 里声明，也总是默认被拒」的权限**（man `landlock_create_ruleset(2)`：`For historical reasons, the LANDLOCK_ACCESS_FS_REFER right is always denied by default, even when its bit is not set in handled_access_fs.`）；在 ABI v1 上「跨目录 reparent 永远被拒」。

网络权限：`LANDLOCK_ACCESS_NET_BIND_TCP` / `LANDLOCK_ACCESS_NET_CONNECT_TCP`（ABI 4）；`LANDLOCK_ACCESS_NET_BIND_UDP` / `LANDLOCK_ACCESS_NET_CONNECT_SEND_UDP`（**ABI 10**，[landlock.h](https://github.com/torvalds/linux/blob/master/include/uapi/linux/landlock.h)：`Support added in Landlock ABI version 10.`）。

IPC scope flags（相对「权限」是另一类）：`LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET`、`LANDLOCK_SCOPE_SIGNAL`（**ABI 6**）；它们**不支持通过 `landlock_add_rule(2)` 开例外**（man `landlock(7)`：`IPC scoping does not support exceptions via landlock_add_rule(2).`）。

### ABI 版本演进（准确的表）

man `landlock(7)` 的 `VERSIONS` 一节给出表（本机 man-pages 6.19，`Linux man-pages 6.19   2026-07-18`；在线同页 [landlock(7)](https://man7.org/linux/man-pages/man7/landlock.7.html)）：

| ABI | 内核 | 新增 |
| --- | --- | --- |
| 1 | 5.13 | `LANDLOCK_ACCESS_FS_EXECUTE`、`WRITE_FILE`、`READ_FILE`、`READ_DIR`、`REMOVE_DIR`、`REMOVE_FILE`、`MAKE_CHAR`、`MAKE_DIR`、`MAKE_REG`、`MAKE_SOCK`、`MAKE_FIFO`、`MAKE_BLOCK`、`MAKE_SYM` |
| 2 | 5.19 | `LANDLOCK_ACCESS_FS_REFER` |
| 3 | 6.2 | `LANDLOCK_ACCESS_FS_TRUNCATE` |
| 4 | 6.7 | `LANDLOCK_ACCESS_NET_BIND_TCP`、`LANDLOCK_ACCESS_NET_CONNECT_TCP` |
| 5 | 6.10 | `LANDLOCK_ACCESS_FS_IOCTL_DEV` |
| 6 | 6.12 | `LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET`、`LANDLOCK_SCOPE_SIGNAL` |
| 7 | 6.15 | `LANDLOCK_RESTRICT_SELF_LOG_SAME_EXEC_OFF`、`..._LOG_NEW_EXEC_ON`、`..._LOG_SUBDOMAINS_OFF`（日志行为，不是新权限） |
| 8 | 7.0 | `LANDLOCK_RESTRICT_SELF_TSYNC` |
| 9 | 7.1 | `LANDLOCK_ACCESS_FS_RESOLVE_UNIX` |
| **10** | **（man 未列）** | `LANDLOCK_ACCESS_NET_BIND_UDP`、`LANDLOCK_ACCESS_NET_CONNECT_SEND_UDP`（[landlock.h](https://github.com/torvalds/linux/blob/master/include/uapi/linux/landlock.h)） |

man 明确说：`Users should use the Landlock ABI version rather than the kernel version to determine which features are available.`，并提醒发行版内核可能 backport、版本号对不上（同页 VERSIONS）。`landlock.rst` 给的做法是 best-effort：**运行时读 ABI，再对 `handled_access_fs` 取该 ABI 认识的最大子集**，例子里的表写到 `(LANDLOCK_ACCESS_FS_RESOLVE_UNIX << 1) - 1  // v9: add "resolve_unix"`（[landlock.rst](https://docs.kernel.org/userspace-api/landlock.html)，`Defining and enforcing a security policy` 之后的兼容段）。

**本机观测**：`landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)` 在 `7.2.6-arch2-1` 上返回 **10**（本机编译的 C 探针，2026-09-30）。这是 **ABI 10** 在实际内核上出现的第一手证据，也说明 man-pages 6.19 的表落后于内核。

### 无特权使用的前提

`landlock_restrict_self(2)`：`In order to enforce a ruleset, either the caller must have the CAP_SYS_ADMIN capability in its user namespace, or the thread must already have the no_new_privs bit set. As for seccomp(2), this avoids scenarios where unprivileged processes can affect the behavior of privileged children (e.g., because of set-user-ID binaries). If that bit was not already set by an ancestor of this thread, the thread must make the following call: prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);`（[landlock_restrict_self(2)](https://man7.org/linux/man-pages/man2/landlock_restrict_self.2.html)）。

**ABI 10 起有免 prctl 的新路**：`LANDLOCK_RESTRICT_SELF_NO_NEW_PRIVS`（`LANDLOCK_RESTRICT_SELF_NO_NEW_PRIVS (1U << 4)`）的注释写：`Sets the no_new_privs attribute of the calling thread only once the enforcement of the ruleset succeeded: no_new_privs is set if and only if sys_landlock_restrict_self() succeeds. This removes the need for a prior prctl(2) PR_SET_NO_NEW_PRIVS call (or CAP_SYS_ADMIN use).`（[landlock.h](https://github.com/torvalds/linux/blob/master/include/uapi/linux/landlock.h)）。`landlock.rst` 的代码示例也把它写成 ABI>10 的分支（`If the ABI > 10, we can tie setting no_new_privs with successful ruleset enforcement and skip the manual prctl(...) call.`，[landlock.rst](https://docs.kernel.org/userspace-api/landlock.html)）——**注意文档里这个 `> 10` 与头文件把它算在 ABI 10 的写法有出入，属于上游文档内部的不一致，写在这里备查。**

### 它不能限制什么

- **`CAVEATS`（man `landlock(7)`）**：`It is currently not possible to restrict some file-related actions accessible through these system call families: chdir(2), stat(2), flock(2), chmod(2), chown(2), setxattr(2), utime(2), fcntl(2), access(2).`——即**元数据与存在性探测不在管辖内**。
- **已打开的 fd 不受限**：`Files or directories opened before the sandboxing are not subject to these restrictions.`（man `landlock(7)`，`Filesystem actions` 开头）；`Truncating files` 一节进一步说截断权限**绑定到 fd 上**，可以在进程之间传递、即使接收方没有 Landlock ruleset 也保留。**本机实测**：先 `open("/etc/hostname", O_RDONLY)`、再施加只允许读的 ruleset，随后对该 fd 的 `pread` 仍成功（探针输出 `已打开的 /etc/hostname fd 继续可读 -> OK`）。
- **不能改文件系统拓扑**：`Threads sandboxed with filesystem restrictions cannot modify filesystem topology, whether via mount(2) or pivot_root(2). However, chroot(2) calls are not denied.`（landlock.rst `Current limitations` → `Filesystem topology modification`）。
- **特殊文件系统**：`files that do not come from a user-visible filesystem (e.g. pipe, socket), but can still be accessed through /proc/<pid>/fd/*, cannot currently be explicitly restricted. Likewise, some special kernel filesystems such as nsfs, which can be accessed through /proc/<pid>/ns/*, cannot currently be explicitly restricted.`——不过 `thanks to the ptrace restrictions, access to such sensitive /proc files are automatically restricted according to domain hierarchies`（landlock.rst 同节）。
- **ptrace 被额外约束**：被沙箱化的进程要 ptrace 目标，必须持有目标 ruleset 的子集（`the tracee must be in a sub-domain of the tracer`；man `landlock(7)` `Ptrace restrictions` 与 landlock.rst 同节）。
- **层数上限 16**：`There is a limit of 16 layers of stacked rulesets. ... Once this limit is reached, sys_landlock_restrict_self() returns E2BIG.`（landlock.rst `Ruleset layers`）。
- **IOCTL 只对新打开的 fd 生效**：`The LANDLOCK_ACCESS_FS_IOCTL_DEV right restricts the use of ioctl(2), but it only applies to newly opened device files.`（landlock.rst `Current limitations` → `IOCTL support`）。
- **`/proc`、信号、同一 uid 的其他进程**：Landlock 的机制是「进程给自己上约束」，作用域是**施加者线程及其后代**（man `landlock(7)` `Inheritance`：`Every new thread resulting from a clone(2) inherits Landlock domain restrictions from its parent. ... one process's thread may apply Landlock rules to itself, but they will not be automatically applied to other sibling threads`；landlock.rst 同）。**推论（从这条事实推出）**：**同一个 uid 下、与沙箱进程没有 fork/clone 关系的其它进程完全不受影响**——Landlock 不是系统级策略，管不了「用户的其他终端」。对 `/proc/<pid>/*` 的读、对其它进程发信号这类事，要挡住只能靠 ABI 6 的 `LANDLOCK_SCOPE_SIGNAL`（信号）与 ptrace restrictions（`/proc/<pid>/mem` 之类），而抽象 Unix socket 之外的 `/proc` 访问本身不在文件权限管辖内（见上面的 special filesystems 一节）。**信号限制本身也有明确边界**：`LANDLOCK_SCOPE_SIGNAL` 只允许向同域或嵌套域内的进程发信号（man `landlock(7)` `IPC scoping`）。

### 一旦施加能否撤销

不能。man `landlock(7)` 的 EXAMPLE 结尾：`Once a thread is landlocked, there is no way to remove its security policy; only adding more restrictions is allowed.`；`landlock_restrict_self(2)` 的 `A domain can only be updated in such a way that the constraints of each past and future composed rulesets will restrict the thread and its future children for their entire life.`（[landlock_restrict_self(2)](https://man7.org/linux/man-pages/man2/landlock_restrict_self.2.html)）。

**推论**：这决定了一个进程一旦给自己上了 Landlock，就**不能在同一进程内「临时放宽」**；要放宽只能换一个新进程（这正是 [01](./01-dsh-workspace-permissions-and-shell.md) 里 DSH 用独立 launcher 可执行文件 `landlock-run` 的原因）。

### 本机实测：allow-list 让「区外写」失败

本机（Arch，内核 7.2.6，ABI 10）编译的探针：ruleset `handled_access_fs` 取到 ABI 10 的全部位；给 `/` 只授 `EXECUTE|READ_FILE|READ_DIR`，给 `/tmp` 授全部；`PR_SET_NO_NEW_PRIVS` + `landlock_restrict_self` 之后：

```
write    /tmp/ll-ok                                      -> OK
write    /home/forty/code/fortystory/fs-agent/ll-probe   -> Permission denied
read     /etc/hostname                                   -> OK
read 前的 fd（限制前打开）继续可读                          -> OK
```

对照实验（同一进程、不加 Landlock）：写 workspace 成功。因此上面那行 `Permission denied`（`EACCES`）**是 Landlock 造成的**，不是挂载造成的。至于写 `/etc` 在本机同样报 `只读文件系统`，那是本机 `/` 本身以 `bwrap`/`ro` 挂载，**不能拿它当 Landlock 的证据**（见 §来源与版本里的说明）。

这与 [01](./01-dsh-workspace-permissions-and-shell.md) §③④里 DSH 的 Landlock 拒绝方言 `permission denied` 一致。

---

## ④ seccomp / seccomp-bpf

**一手来源**：内核文档 [`Documentation/userspace-api/seccomp_filter.rst`](https://docs.kernel.org/userspace-api/seccomp_filter.html)（master）、[`seccomp(2)`](https://man7.org/linux/man-pages/man2/seccomp.2.html)（本机 man-pages 6.19）、以及 Docker/moby 的默认 profile（[default.json](https://github.com/moby/profiles/blob/main/seccomp/default.json)）。

### 它管什么：syscall 号与参数，不解引用指针

seccomp_filter.rst 把机制说死了（原样引用）：

> `The filter is expressed as a Berkeley Packet Filter (BPF) program, as with socket filters, except that the data operated on is related to the system call being made: system call number and the system call arguments.`

> `Additionally, BPF makes it impossible for users of seccomp to fall prey to time-of-check-time-of-use (TOCTOU) attacks that are common in system call interposition frameworks. BPF programs may not dereference pointers which constrains all filters to solely evaluating the system call arguments directly.`

> `System call filtering isn't a sandbox. It provides a clearly defined mechanism for minimizing the exposed kernel surface. It is meant to be a tool for sandbox developers to use.`（该节标题就叫 `What it isn't`）

`seccomp(2)` 的 `SECCOMP_SET_MODE_FILTER`：`The system calls allowed are defined by a pointer to a Berkeley Packet Filter (BPF) passed via args.`，并要求 `either the calling thread must have the CAP_SYS_ADMIN capability in its user namespace, or the thread must already have the no_new_privs bit set`（[seccomp(2)](https://man7.org/linux/man-pages/man2/seccomp.2.html)）；过滤器随 `fork`/`clone` 继承、跨 `execve` 保留（同处）。

### 为什么它不能单独做「路径前缀」判定

- **指针参数看不见内容**：上面第一句就是答案。`openat(2)` 的路径是 `args[0]` 指向的用户内存，BPF 无法读它，只能拿到那个**指针数值**——而指针数值每次调用都不同。Rust 生态里 [extrasafe](https://github.com/boustrophedon/extrasafe) 的 README 用同一理由解释它为什么需要 Landlock：`Seccomp filters are a somewhat blunt tool. ... They don't allow you to filter by path name in open calls, or indeed any syscall arguments that are pointers (but we now support Landlock!)`。
- **一旦试图用「解释器/包装器」来补，就会引入 TOCTOU 与解析不完备**：seccomp 的卖点恰恰是**没有** TOCTOU（见上）；把判定搬到用户态（`SECCOMP_RET_TRACE`/`SECCOMP_RET_USER_NOTIF` 的 supervisor）就把这个性质换掉了。seccomp 自身的历史坑也写在这里：`on older kernels, seccomp-based sandboxes must not allow use of ptrace(2)——even of other sandboxed processes——without extreme care; ptracers can use this mechanism to escape from the seccomp sandbox.`（`seccomp(2)` 的 `SECCOMP_RET_TRACE`，[seccomp(2)](https://man7.org/linux/man-pages/man2/seccomp.2.html)）。
- **deny-list 结构性危险**：`It is strongly recommended to use an allow-list approach whenever possible because such an approach is more robust and simple. A deny-list will have to be updated whenever a potentially dangerous system call is added ... it is often possible to alter the representation of a value without altering its meaning, leading to a deny-list bypass.`，并特别点名 x86-64/x32 的 `__X32_SYSCALL_BIT` 绕过（`seccomp(2)` 的 NOTES，[seccomp(2)](https://man7.org/linux/man-pages/man2/seccomp.2.html)）。

### 它在文件沙箱里通常补什么

**拦住那些「绕过路径判定」的 syscall**。最清楚的一手例子是 Docker 的默认 profile：它把 `open_by_handle_at` 放在 `includes: { caps: ["CAP_DAC_READ_SEARCH"] }` 下（也就是默认拒绝，只有给了这个 capability 才放行），并把 `mount`、`setns`、`unshare`、`clone3`、`bpf`、`open_tree`、`fsopen`、`pivot_root` 类操作放在 `includes: { caps: ["CAP_SYS_ADMIN"] }` 下；`ptrace` 则在 `CAP_SYS_PTRACE` 下放行（[moby default.json](https://github.com/moby/profiles/blob/main/seccomp/default.json)，逐条 JSON 可见）。`open_by_handle_at` 之所以是经典绕过，是因为它按 **file handle** 打开文件、不经过路径，因此任何「按路径前缀」的判定都拦不住它；`seccomp(2)` 里 `open_by_handle_at(2)` 自己要求 `CAP_DAC_READ_SEARCH`（[open_by_handle_at(2)](https://man7.org/linux/man-pages/man2/open_by_handle_at.2.html) 逐字：`The caller must have the CAP_DAC_READ_SEARCH capability to invoke open_by_handle_at().`；本机 man-pages 6.19 的 `man 2 open_by_handle_at` 同）。

**推论（从上面两条推出，不是文档原话）**：在「工作区可写、区外只读」的设计里，seccomp 的位置是**第二道闸**——把 Landlock/bwrap 管不到或难以枚举的口子（按 handle 打开、挂载、`setns`、`ptrace`）关掉；它无法承担「判定这个路径在不在工作区」这件事本身。

---

## ⑤ namespaces（user / mount / pid / net / ipc / uts / cgroup）与 `pivot_root` / `chroot`

**一手来源**：man pages [`namespaces(7)`](https://man7.org/linux/man-pages/man7/namespaces.7.html)、[`user_namespaces(7)`](https://man7.org/linux/man-pages/man7/user_namespaces.7.html)、[`mount_namespaces(7)`](https://man7.org/linux/man-pages/man7/mount_namespaces.7.html)、[`pid_namespaces(7)`](https://man7.org/linux/man-pages/man7/pid_namespaces.7.html)、[`network_namespaces(7)`](https://man7.org/linux/man-pages/man7/network_namespaces.7.html)、[`pivot_root(2)`](https://man7.org/linux/man-pages/man2/pivot_root.2.html)、[`chroot(2)`](https://man7.org/linux/man-pages/man2/chroot.2.html)、[`unshare(2)`](https://man7.org/linux/man-pages/man2/unshare.2.html)、[`clone(2)`](https://man7.org/linux/man-pages/man2/clone.2.html)（本机 man-pages 6.19）、内核文档 [`sysctl/user.rst`](https://docs.kernel.org/admin-guide/sysctl/user.html)。

### 各自带来什么隔离

`namespaces(7)` 的表（Namespace / Flag / Isolates）逐行：

| Namespace | Flag | Isolates |
| --- | --- | --- |
| Cgroup | `CLONE_NEWCGROUP` | cgroup root directory |
| IPC | `CLONE_NEWIPC` | System V IPC、POSIX message queues |
| Network | `CLONE_NEWNET` | 网络设备、栈、端口等 |
| Mount | `CLONE_NEWNS` | mount points |
| PID | `CLONE_NEWPID` | process IDs |
| Time | `CLONE_NEWTIME` | boot/monotonic clocks |
| User | `CLONE_NEWUSER` | user/group IDs |
| UTS | `CLONE_NEWUTS` | hostname、NIS domain name |

API 是 `clone(2)`（子进程进新 namespace）、`unshare(2)`（自己进新 namespace）、`setns(2)`（加入已有 namespace，用一个 `/proc/pid/ns/*` fd 指定）（同 man `The namespaces API` 一节）。

**文件沙箱只用到其中两个**（mount + user），其余是顺带：**推论**——`pid`/`net`/`ipc`/`uts`/`cgroup` 不改变文件系统的可写范围（`net` 会改变网络可见性，但它不防止写宿主文件）。

### 无特权创建 user namespace 需要什么

- `namespaces(7)`：`Creation of new namespaces using clone(2) and unshare(2) in most cases requires the CAP_SYS_ADMIN capability ... User namespaces are the exception: since Linux 3.8, no privilege is required to create a user namespace.`
- `user_namespaces(7)`：`A call to clone(2) or unshare(2) with the CLONE_NEWUSER flag makes the new child process (for clone(2)) or the caller (for unshare(2)) a member of the new user namespace created by the call.`；新 namespace 里的进程拿到**该 namespace 的完整 capability 集**，在父 namespace 里没有 capability（`Capabilities` 一节）。mount 能力的具体范围写在同一页 `Effect of capabilities within a user namespace`：`Holding CAP_SYS_ADMIN within the user namespace that owns a process's mount namespace allows that process to create bind mounts and mount the following types of filesystems: /proc (since Linux 3.8), /sys (since Linux 3.8), devpts (since Linux 3.9), tmpfs(5) (since Linux 3.9), ramfs (since Linux 3.9), mqueue (since Linux 3.9) ...`。
- 嵌套上限：`The kernel imposes (since Linux 3.11) a limit of 32 nested levels of user namespaces.`（同 man）。
- 数量上限由 `/proc/sys/user/max_user_namespaces` 这类 sysctl 管（[sysctl/user.rst](https://docs.kernel.org/admin-guide/sysctl/user.html)：`max_user_namespaces ... The maximum number of user namespaces that any user in the current user namespace may create.`）；本机 `cat /proc/sys/user/max_user_namespaces` = **2147483647**，`/proc/sys/kernel/unprivileged_userns_clone` = **1**（本机观测；后者按 Canonical spec 的说法属于「没有被上游接受、由若干发行版内核携带」的补丁——同文原话 `a kernel patch (that has not been accepted by the upstream kernel developers)`，[Ubuntu spec](https://discourse.ubuntu.com/t/spec-unprivileged-user-namespace-restrictions-via-apparmor-in-ubuntu-23-10/37626)）。
- **发行版可以关掉它**：Canonical spec 列出的 `kernel.unprivileged_userns_clone` 补丁（Debian/Arch/Ubuntu 都有过），以及 Ubuntu 24.04 的 `kernel.apparmor_restrict_unprivileged_userns`（见 §②(a)）。

### mount namespace

`mount_namespaces(7)`：`Mount namespaces provide isolation of the list of mounts seen by the processes in each namespace instance.`；新建时是父的挂载表**副本**（`clone` 取父的、`unshare` 取调用者此前的），之后的 `mount(2)`/`umount(2)` 默认不影响另一边（同 man DESCRIPTION）。另有 `SHARED SUBTREES` 一节讲挂载事件在 namespace 之间的传播（`MS_SHARED` 等四种 propagation type）——**推论**：把宿主挂载点 bind 进沙箱时，传播类型决定了 `mount` 会不会漏回宿主；bwrap 的 README 也提到它会把用户指定的目录默认挂成 `nodev`（`Any such directories you specify mounted nodev by default, and can be made readonly.`）。

### `pivot_root` 与 `chroot` 的定位

- `pivot_root(2)`：`changes the root mount in the mount namespace of the calling process`，`The calling process must have the CAP_SYS_ADMIN capability in the user namespace that owns the caller's mount namespace.`；限制包括 `new_root and put_old must not be on the same mount as the current root`、`new_root must be a path to a mount point, but can't be "/"`、父挂载传播类型不得是 `MS_SHARED` 等（[pivot_root(2)](https://man7.org/linux/man-pages/man2/pivot_root.2.html)）。**它是「换根」而不是「限制权限」**：换完根之后进程仍然拥有原 uid/capability，只是路径解析从新根开始。
- `chroot(2)`：`Only a privileged process (Linux: one with the CAP_SYS_CHROOT capability in its user namespace) may call chroot().`，并且官方明确否定它的安全用途：`This call changes an ingredient in the pathname resolution process and does nothing else. In particular, it is not intended to be used for any kind of security purpose, neither to fully sandbox a process nor to restrict filesystem system calls.`，还给了逃逸方式（`chdir(2)` 到将被移出 chroot 的目录，等它被移出，再 `open("../../../etc/passwd")`）（[chroot(2)](https://man7.org/linux/man-pages/man2/chroot.2.html)）。
- **推论**：`pivot_root` + 挂载表构造 = bwrap 那条路；单独的 `chroot` 不是那条路。nsjail 的 README 把 `chroot()` 与 `pivot_root()` 并列列为「Filesystem constraints」手段（[nsjail README](https://github.com/google/nsjail)）。

---

## ⑥ 现成工具对比

每一节只写：**要什么权限、是进程级/容器级/虚机级、对「shell 在一个可写工作区里跑、区外只读」是否合用（机制层面的事实）**。

### nsjail（Google）

- 定位：`Linux process isolation tool using namespaces, resource limits, and seccomp-bpf syscall filters.`；隔离维度列出 `UTS, MOUNT, PID, IPC, NET, USER, CGROUPS, TIME`；文件系统手段 `chroot()`, `pivot_root()`, read-only mounts, custom `/proc` and `tmpfs`；syscall 过滤用 [Kafel](https://github.com/google/kafel/) 的 seccomp-bpf 策略；还有 cgroup 集成（v1/v2）（[nsjail README](https://github.com/google/nsjail)）。
- 权限前提：README 的 Troubleshooting 第一条写 `CLONE_NEWUSER required: Run with --disable_clone_newuser (requires root) or ensure user namespaces are enabled: sysctl kernel.unprivileged_userns_clone  # Should be 1`；它的 Docker 示例直接 `docker run --privileged`（同 README）。**事实**：要么无特权 userns 可用（默认走 `clone_newuser: true`，配置文件里可见 `clone_newuser: true`），要么 root。
- 层级：**进程级**（它 fork/exec 目标进程并施加 namespace+seccomp+资源限制）。
- 对「可写工作区、区外只读」：README 把 `read-only mounts` 列为文件系统能力（可选 flag/配置），机制上同 bwrap（mount namespace）。**推论**：它比 bwrap 更重（自带 supervisor、cgroup、网络），但两者在「造文件系统视图」上是同类手段。

### firejail

- 官方定位：`Firejail is a lightweight security tool intended to protect a Linux system by setting up a restricted environment for running (potentially untrusted) applications.`，并自称 `it is an SUID sandbox program that reduces the risk of security breaches by using Linux namespaces, seccomp-bpf and Linux capabilities.`（[firejail README](https://github.com/netblue30/firejail)）。它支持 profile（`~/.config/firejail/`、`/etc/firejail/`、找不到就用 default profile，且 `The default profile is quite restrictive.`），默认 profile 会把系统目录挂只读：`These directories are /etc, /var, /usr, /bin, /sbin, /lib, /lib32, /libx32 and /lib64. Only /home and /tmp are writable.`（[firejail man](https://github.com/netblue30/firejail/blob/master/src/man/firejail.1.in)）。
- 路径级选项：`--blacklist=dirname_or_filename`（`This makes a file or directory completely inaccessible.`，注释里讲 symlink 也会被一起拉黑）、`--read-only=<dir_or_file>`（例：`firejail --read-only=~/.mozilla /usr/bin/firefox`）（同 man）。man 里还能看到 `--allow-debuggers`（`whitelisting system calls ptrace and process_vm_readv`）与 `--build`（`builds a whitelisted profile`）这两个含「whitelist」的条目；**我没有从 man 里逐字取到 `--whitelist=` 这个选项的条目，因此不在这里断言它**。
- 权限与层级：**SUID + 进程级**。它的 README 自称 `it is an SUID sandbox program ...`（同 README）；bwrap 的作者在 README 里把 firejail 作为对照：`Firejail is similar to Flatpak before bubblewrap was split out in that it combines a setuid tool with a lot of desktop-specific sandboxing features.`（[bubblewrap README](https://github.com/containers/bubblewrap/blob/main/README.md)）——**注意这是 bubblewrap 的立场，不是 firejail 自己的措辞**。firejail 自己的 `SECURITY.md` 只讲支持版本策略（`we only support the latest released version (and the current development version)`），没有找到「firejail 不是安全边界」这类自陈原话（[firejail SECURITY.md](https://github.com/netblue30/firejail/blob/master/SECURITY.md)）。
- 对「可写工作区、区外只读」：**机制上直接对应**（default profile 就是「系统目录 ro、/home 与 /tmp rw」，可以定制），代价是它要 setuid 安装。

### `systemd-run --user` 的 sandboxing

`systemd.exec(5)` 的 `SANDBOXING` 一节开头（本机 systemd 262 的 man；在线同页 [systemd.exec](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html)）：

- `ReadWritePaths=, ReadOnlyPaths=, InaccessiblePaths=, ExecPaths=, NoExecPaths=`：`Sets up a new file system namespace for executed processes. ... Paths listed in ReadWritePaths= are accessible from within the namespace with the same access modes as from outside of it. Paths listed in ReadOnlyPaths= are accessible for reading only, writing will be refused even if the usual file access controls would permit this. Nest ReadWritePaths= inside of ReadOnlyPaths= in order to provide writable subdirectories within read-only directories.`；`InaccessiblePaths=` 下面不能嵌套前两者；另有 `-`/`+` 前缀语义（前者不存在则忽略、后者相对 `RootDirectory=`）。
- `ProtectSystem=`：`true` 挂只读 `/usr/` 与 bootloader 目录，`full` 连 `/etc/`，`strict` 把整个层级挂只读（除 API 挂载点），可用 `ReadWritePaths=` 开例外（同页）。
- `ProtectHome=`：`true` 让 `/home/`、`/root`、`/run/user` 不可访问且为空；`read-only` 改成只读；`tmpfs` 在某上挂只读 tmpfs（同页）。man 还说明它与 `InaccessiblePaths=` / `ReadOnlyPaths=` / `TemporaryFileSystem=:ro` 大致等价。
- `PrivateTmp=`：`a new file system namespace will be set up ... /tmp/ and /var/tmp/ directories inside it are not shared with processes outside of the namespace`（同页）。
- `NoNewPrivileges=`：`ensures that the service process and all its children can never gain new privileges through execve() (e.g. via setuid or setgid bits, or filesystem capabilities). This is the simplest and most effective way to ensure that a process and its children can never elevate privileges again.`（同页）。
- `PrivateUsers=`：`sets up a new user namespace for the executed processes and configures a user and group mapping`，取值 `self`/`identity`/`full`/`managed`（同页）。
- `RestrictNamespaces=`：限制 `unshare(2)`/`clone(2)`/`setns(2)` 到给定的 namespace 类型列表，取 `cgroup, ipc, net, mnt, pid, user, uts, time`（同页）。
- **最关键的一条前提**（同一 `SANDBOXING` 节）：`some sandboxing functionality is generally not available in user services (i.e. services run by the per-user service manager). Specifically, the various settings requiring file system namespacing support (such as ProtectSystem=) are not available, as the underlying kernel functionality is only accessible to privileged processes. However, most namespacing settings, that will not work on their own in user services, will work when used in conjunction with PrivateUsers=true.`（[systemd.exec](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html)；本机 systemd 262 的 man 逐字相同）
- 同节还提醒这些只读选项**不挡 AF_UNIX 通信**：`the various options that turn directories read-only ... do not affect the ability for programs to connect to and communicate with AF_UNIX sockets in these directories. These options cannot be used to lock down access to IPC services hence.`

**推论**：`systemd-run --user --property=...` 要在无特权 user manager 下真正生效，`PrivateUsers=true`（或 `PrivateUsers=` 的某个值）几乎是必须一起给的，因为单纯的文件系统 namespace 选项在 user service 里会被「gracefully turned off」（同节原话是 `many of these sandboxing features are gracefully turned off on systems where the underlying security mechanism is not available`）。**要留意这个「优雅关闭」语义：它不是报错，是静默不生效。**

### rootless Podman / Docker

- **Podman（rootless）**：官方 tutorial 说 `Rootless Podman requires the user running it to have a range of UIDs listed in the files /etc/subuid and /etc/subgid.`，并解释了这是通过 `newuidmap`/`newgidmap` 把一段宿主 uid 映射进 user namespace（[podman rootless tutorial](https://github.com/containers/podman/blob/main/docs/tutorials/rootless_tutorial.md)：`Podman makes use of a user namespace to shift the UIDs and GIDs of a block of users it is given access to on the host (via the newuidmap and newgidmap executables)`，并明确 `Rootless Podman is not, and will never be, root; it's not a setuid binary, and gains no privileges when it runs.`）。另需要用户态网络工具（unprivileged network namespace 的 `pasta`/slirp4netns 类）。
- **挂载可见性**：`podman run -v host:container:ro` / `--read-only`（`Mount the container's root filesystem as read-only.`）——`--volume` 支持 `ro=true` 之类选项，`--read-only-tmpfs` 默认在 `/dev`、`/dev/shm`、`/run`、`/tmp`、`/var/tmp` 上挂 rw tmpfs（[podman-run(1)](https://docs.podman.io/en/latest/markdown/podman-run.1.html)，`--read-only`/`--read-only-tmpfs` 条目）。**注意容器与宿主是不同挂载命名空间**：要「让 shell 在宿主的工作区里跑」，只能把工作区 bind 进去，容器内的可写视图**不是宿主路径本身**。
- **Docker（rootless）**：官方文档要求 `newuidmap`/`newgidmap`，`/etc/subuid` 与 `/etc/subgid` 里至少 65,536 个 subordinate id，且 `Rootless mode does not require root privileges even during the installation of the Docker daemon, as long as the prerequisites are met.`（[Docker Rootless mode](https://docs.docker.com/engine/security/rootless/)）。
- 层级：**容器级**（OCI 运行时 + 镜像/rootfs）。
- 对「shell 在工作区里跑、区外只读」：机制上能表达（bind 工作区 rw、`--read-only`、只读 bind），但要额外维护镜像/运行时/存储与 subuid 配置，而且 `.scratch` 这种宿主路径必须显式 bind。**这是「事实描述」，不是取舍建议。**

### gVisor（`runsc`）

- 定位与机制：应用对宿主 System API 的调用由 **Sentry** 拦截并**重新实现**（`No system call is passed through directly to the host. Every supported call has an independent implementation in the Sentry`），文件系统由 **Gofer** 进程管理、按请求提供文件描述符（`The Gofer process manages the container's filesystem and provides file descriptors to the sandbox upon request.`；沙箱自身在空 mount namespace 里，可选 directfs 让其完全不开文件）（[gVisor Security Model](https://gvisor.dev/docs/architecture_guide/security/)）。
- 平台（syscall 拦截实现）：`KVM`（用 KVM 让 Sentry 同时当 guest OS 与 VMM）、`systrap`（用 `SECCOMP_RET_TRAP` + `SIGSYS` 拦截）、`ptrace`（`PTRACE_SYSEMU`）；`systrap replaced ptrace as the default gVisor platform in mid-2023.`（[gVisor Platform Guide](https://gvisor.dev/docs/architecture_guide/platforms/)）。
- 层级：**容器级**（`runsc` 是 OCI runtime，跑一个容器）。它自己就是「用户态内核」的路子，而不是给单个 shell 加约束。
- 对「shell 在宿主工作区里跑、区外只读」：机制上是「把工作负载放进一个被虚拟化的 System API 里」，宿主路径要通过 gofer/挂载配置暴露；**不适用于「在宿主原地给一个 shell 加文件边界」**（**推论**，依据是上面的机制：宿主文件访问要经过 gofer 而不是直接路径解析）。

### microVM：Firecracker / Kata Containers

- **Firecracker**：`an open source virtualization technology ... Firecracker runs workloads in lightweight virtual machines, called microVMs`，核心是 `a virtual machine monitor (VMM) that uses the Linux Kernel Virtual Machine (KVM) to create and run microVMs`（[Firecracker README](https://github.com/firecracker-microvm/firecracker)）。
- **Kata Containers**：`standard implementation of lightweight Virtual Machines (VMs) that feel and perform like containers, but provide the workload isolation and security advantages of VMs`；它的 runtime 是 containerd shim，hypervisor 走硬件虚拟化（README 的架构表里列出 `x86_64, amd64 | Intel VT-x, AMD SVM`）（[Kata Containers README](https://github.com/kata-containers/kata-containers)）。
- 层级：**虚机级**。宿主工作区要通过 virtiofs/block/共享目录显式给 guest。**推论**：对「让一个 `bash` 在宿主工作区里跑」这条需求，虚机级意味着 guest 里要有一份完整的 rootfs（或 rootfs image）与 guest 内核；工作区是「挂进去的」，不是「原地被限制的」。

### WASM 沙箱：wasmtime / WASI

- wasmtime 是 `A standalone runtime for WebAssembly`（[wasmtime README](https://github.com/bytecodealliance/wasmtime)）。**它执行的是 WebAssembly 模块，不是宿主上的原生二进制**（同 README 的定位）。文件系统的能力模型是**显式预开目录**：`--dir` 把宿主目录暴露给模块，且 wasmtime 自己的选项必须写在 `.wasm` 之前——官方文档专门用 `wasmtime foo.wasm --dir .` 作反例（这样会把 `--dir .` 传给程序本身），正确的是 `wasmtime --dir . foo.wasm`（`All Wasmtime options must come before the WebAssembly file provided.`，[wasmtime CLI options](https://docs.wasmtime.dev/cli-options.html)）。
- 层级：**运行时的模块级沙箱**（capability-based；不给 preopen 就拿不到文件系统）。
- 对「shell 在工作区里跑、区外只读」：**机制上不直接可用**——要拿 `bash` 当沙箱对象，得先把 `bash` 以及它要 exec 的一切编成 wasm（**推论**：依据是上面「跑的是 wasm 模块」这一事实）；WASI 的 preopen 也只能表达「哪些目录可用」，表达不了「宿主上任意命令的写边界」。

---

## ⑦ Rust 生态

crates.io 的数字都取自官方 API `https://crates.io/api/v1/crates/<name>`（本报告 2026-09-30 抓取）；「最后更新」用 API 的 `updated_at`。

### `landlock`（landlock-lsm/rust-landlock）

- **最新版本 0.4.7，`updated_at` = 2026-07-27，累计下载 17,061,772**（[crates.io API](https://crates.io/api/v1/crates/landlock)）；仓库 [`landlock-lsm/rust-landlock`](https://github.com/landlock-lsm/rust-landlock)。
- **封装机制**：Landlock 三个 syscall 的安全抽象。README：`Landlock is a security feature available since Linux 5.13. ... This Rust crate provides a safe abstraction for the Landlock system calls along with some helpers.`；`Landlock empowers any process, including unprivileged ones, to securely restrict themselves.`（[README](https://github.com/landlock-lsm/rust-landlock)）。
- **API 形状**（来自官方示例 [`examples/sandboxer.rs`](https://github.com/landlock-lsm/rust-landlock/blob/main/examples/sandboxer.rs) 的 `use` 列表，函数名逐字）：`landlock::{path_beneath_rules, Access, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, LandlockStatus, NetPort, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope, ABI}`；示例里用 `path_beneath_rules(paths, access)` 生成规则、用 `Ruleset`/`RulesetAttr`/`RulesetCreatedAttr` 三段式建规则集。**它显式建模了 ABI 协商**（`ABI`、`CompatLevel`、`RulesetStatus`、`LandlockStatus`）。
- **能不能在 `pre_exec` 里施加**：**本报告没有找到该 crate 关于 `pre_exec`/fork 的专门文档段落**，因此不写断言。可确认的相邻事实是：Linux 侧 `pre_exec` 的约束是「`pre_exec` 闭包必须在只有调用线程存活时跑」（`CommandExt::pre_exec` 的文档要求调用方保证 async-signal-safe），而 Landlock 自身要求 `restrict_self` 作用于**调用线程**、并被子线程/子进程继承（[landlock_restrict_self(2)](https://man7.org/linux/man-pages/man2/landlock_restrict_self.2.html)、man `landlock(7)` `Inheritance`）。**这是从内核侧事实推出的，不是 crate 文档原话。**
- **ABI 10 的 UDP 权限是否已暴露：未确认。** 官方 sandboxer 示例里的网络环境变量只有 `LL_TCP_BIND` / `LL_TCP_CONNECT`，`AccessNet` 的用法也只出现 TCP（[sandboxer.rs](https://github.com/landlock-lsm/rust-landlock/blob/main/examples/sandboxer.rs)）；**推论**：0.4.7 的示例尚未覆盖 ABI 10 的 `LANDLOCK_ACCESS_NET_BIND_UDP` / `LANDLOCK_ACCESS_NET_CONNECT_SEND_UDP`，但我没有逐版核对 CHANGELOG。
- **平台**：Linux only（机制本身是 Linux LSM）。
- **维护状态**：活跃（2026-07 仍有发布，且 crate 在 crates.io 的下载量千万级）。

### `seccompiler`（rust-vmm）

- **最新 0.5.0，`updated_at` = 2025-03-07，下载 21,768,779**（[crates.io API](https://crates.io/api/v1/crates/seccompiler)）。
- **封装机制**：把 seccomp-bpf 策略编译成 BPF 并安装。README：`Provides easy-to-use Linux seccomp-bpf jailing.`；`Due to the fact that seccomp is a Linux-specific feature, this crate is supported only on Linux systems.`，支持架构 `Little-endian x86_64`、`aarch64`、`riscv64`（[README](https://github.com/rust-vmm/seccompiler)）。
- **API 形状**（源码 `src/lib.rs` 的 `pub fn`）：`apply_filter(bpf_filter: BpfProgramRef)`、`apply_filter_all_threads(bpf_filter: BpfProgramRef)`、`compile_from_json<R: Read>(reader: R, arch: TargetArch) -> Result<BpfMap>`（[lib.rs](https://github.com/rust-vmm/seccompiler/blob/main/src/lib.rs)）。
- 平台：Linux only；**没有路径感知能力**（见 §④）。

### `libseccomp`（Rust binding）

- **最新 0.4.0，`updated_at` = 2025-04-05，下载 2,016,829**（[crates.io API](https://crates.io/api/v1/crates/libseccomp)）；仓库 [`libseccomp-rs/libseccomp-rs`](https://github.com/libseccomp-rs/libseccomp-rs)。
- **封装机制**：绑定 C 的 libseccomp，因此运行/编译环境要有系统 libseccomp（仓库名与 crate 名同源；**该前提在本报告里只从 crate 名与仓库定位推断，未逐字取到 README 原话**）。相对 `seccompiler`（纯 Rust 编译 BPF）是两条实现路线。

### `nix`

- **最新 0.31.3，`updated_at` = 2026-05-11，下载 831,811,037**（[crates.io API](https://crates.io/api/v1/crates/nix)）；仓库 [`nix-rust/nix`](https://github.com/nix-rust/nix)。
- **它提供与沙箱直接相关的 syscall 封装**（本机 cargo 缓存里的 **nix 0.29.0** 源码逐字）：`nix::sched::unshare(flags: CloneFlags)`（`src/sched.rs:139`）、`nix::unistd::pivot_root(...)`（`src/unistd.rs:2885`）、`nix::sys::prctl::set_no_new_privs()`（`src/sys/prctl.rs:199`）。这些 API 在 feature flag 后面（同一 crate 的 `Cargo.toml`：`sched = ["process"]`、`mount = ["uio"]`、`process = []` 等）。**版本差异提醒**：上面行号来自本机缓存的 0.29.0，crates.io 上当前是 0.31.3。
- 平台：主要是 Unix/Linux（各自有 cfg 门），沙箱相关的三件套是 Linux 语义。它**不是**沙箱框架，只是 syscall 的 safe wrapper。

### `caps`

- **最新 0.5.6，`updated_at` = 2025-10-17，下载 24,730,540**（[crates.io API](https://crates.io/api/v1/crates/caps)）；仓库 [`lucab/caps-rs`](https://github.com/lucab/caps-rs)。
- 定位：`A pure-Rust library to work with Linux capabilities.`，支持 POSIX 三集与 Linux 的 Ambient/Bounding 集，`See capabilities(7) for more details.`；目标是 `be usable in static targets, without requiring an external C library`（[README](https://github.com/lucab/caps-rs)）。
- **它不管路径**：capability 是进程属性，不是文件路径边界（**推论**，依据是 `capabilities(7)` 的语义与上面的定位）。在文件沙箱里它通常用来 drop 掉 `CAP_DAC_READ_SEARCH`/`CAP_SYS_ADMIN` 这类会绕过路径判定的能力。

### `birdcage`（重点：跨平台一个 API）

- **最新 0.8.1，`updated_at` = 2024-04-19，下载 144,495**（[crates.io API](https://crates.io/api/v1/crates/birdcage)）。**仓库已从 `phoenixkahlo/birdcage` 转到 [`phylum-dev/birdcage`](https://github.com/phylum-dev/birdcage)**（crates.io API 的 `repository` 字段即后者）——原作者的仓库地址不是当前维护地址。
- **定位**：`Birdcage is a cross-platform embeddable sandboxing library allowing restrictions to Filesystem and Network operations using native operating system APIs.`，最初为 Phylum CLI 防恶意依赖而做（[README](https://github.com/phylum-dev/birdcage)）。
- **支持平台与底层机制**：README 的 `Supported Platforms` 只有两行——`Linux via namespaces`、`macOS via sandbox_init() (aka Seatbelt)`。**注意：Linux 侧用的是 namespaces，不是 Landlock**（这是它与我最初预期不同的一点，逐字依据即上面两行）。
- **自陈的成熟度/局限（原样引用）**：`Birdcage focuses **only** on Filesystem and Network operations. It **is not** a complete sandbox preventing all side-effects or permanent damage. Applications can still execute most system calls, which is especially dangerous when execution is performed as root. Birdcage should be combined with other security mechanisms, especially if you are executing known-malicious code.`（同 README）。
- **API 形状**（源码 `src/lib.rs` 逐字）：trait `Sandbox`，方法 `fn new() -> Self`、`fn add_exception(&mut self, exception: Exception)`、`fn spawn(self, sandboxee: Command) -> Result<Child>`；平台别名 `#[cfg(target_os = "linux")] pub type Birdcage = LinuxSandbox;`、`#[cfg(target_os = "macos")] pub type Birdcage = MacSandbox;`；例外枚举 `Exception::{ Read(PathBuf), WriteAndRead(PathBuf), ExecuteAndRead(PathBuf), Environment(String), FullEnvironment, Networking }`（[lib.rs](https://github.com/phylum-dev/birdcage/blob/main/src/lib.rs)）。
- **施加方式的重要约束**（`spawn` 的文档注释逐字）：`This will setup the sandbox in the **CURRENT** process, before launching the sandboxee. Since most of the restrictions will also be applied to the calling process, it is recommended to create a separate process before calling this method. The calling process is **NOT** fully sandboxed.`，以及 `Sandboxing will fail if the calling process is not single-threaded.` 和 `After failure, the calling process might still be affected by partial sandboxing restrictions.`（同 lib.rs）。**推论**：它的 API 形状与「`pre_exec` 里给自己上约束」相容，但**要求调用线程是进程里唯一线程**——这与 `Command::pre_exec` 的常见用法（fork 之后、exec 之前）是同一类约束。
- **成熟度**：最后发布 2024-04-19（crates.io `updated_at`），**约 2.5 年无新版本**（相对 2026-09-30）；下载量 144k，比 `landlock`/`seccompiler` 低两个数量级。

### `extrasafe`

- **最新 0.5.1，`updated_at` = 2024-04-16，下载 36,109**（[crates.io API](https://crates.io/api/v1/crates/extrasafe)）；仓库 [`boustrophedon/extrasafe`](https://github.com/boustrophedon/extrasafe)。
- 定位（README 逐字）：`extrasafe is an easy-to-use wrapper around various Linux security tools, including seccomp filters ... the Landlock Linux Security Module ... and user namespaces for broader isolation.`；seccomp 部分给 `Deny-by-default with pre-selected sets of syscalls to enable`；Landlock 部分 `We also support using Landlock to allow specific, targeted access to the filesystem`；并明确写了 seccomp 的路径盲区（见 §④ 引用）与 `you should continue to use Linux Security Modules like AppArmor and SELinux!`（同 README）。
- 其它已查：`syscallz` 0.17.0（`updated_at` 2023-09-28）、`process_control` 5.2.0（2025-09-06）、`gaol` 0.2.1（2019-10-16）；crates.io 上**不存在**名为 `minijail` 的 crate（HTTP 404）。

### 汇总表（crates.io API，2026-09-30）

| crate | max_stable | 最后更新 | 机制 | 平台 |
| --- | --- | --- | --- | --- |
| `landlock` | 0.4.7 | 2026-07-27 | Landlock syscall 安全封装 + ABI 协商 | Linux |
| `seccompiler` | 0.5.0 | 2025-03-07 | 编译/安装 seccomp-BPF（纯 Rust） | Linux |
| `libseccomp` | 0.4.0 | 2025-04-05 | 绑定 C libseccomp | Linux |
| `nix` | 0.31.3 | 2026-05-11 | syscall wrapper（`unshare`/`pivot_root`/`prctl`） | Unix/Linux |
| `caps` | 0.5.6 | 2025-10-17 | capabilities（非路径） | Linux |
| `birdcage` | 0.8.1 | 2024-04-19 | Linux: namespaces；macOS: Seatbelt | Linux + macOS |
| `extrasafe` | 0.5.1 | 2024-04-16 | seccomp + Landlock + userns 包装 | Linux |
| `syscallz` | 0.17.0 | 2023-09-28 | seccomp | Linux |
| `process_control` | 5.2.0 | 2025-09-06 | 子进程控制（非沙箱） | Linux/Windows |

**关于「像 birdcage 这种跨平台一个 API」的观察（事实）**：目前只有 `birdcage` 明确宣称跨 Linux/macOS 且只给一个 `Sandbox` trait；它的 Linux 后端是 namespaces（不用 Landlock），最后发布在 2024-04。**推论**：想同时用上 Landlock 的细粒度 allow-list 与 macOS Seatbelt，就得自己搭两个后端（[01](./01-dsh-workspace-permissions-and-shell.md) 记的 DSH 正是这样：Linux 用 bwrap/Landlock、macOS 用 `sandbox-exec -p <SBPL>`）。

---

## ⑧ 无特权可用性的现实约束

**事实**列，「常见发行版上的坑」里的推论会标注。

| 机制 | 需要的内核版本 | 需要的权限 | 常见发行版/环境上的坑 |
| --- | --- | --- | --- |
| user namespaces（bwrap、nsjail、rootless 容器、`PrivateUsers=` 的共同前提） | Linux 3.8 起无特权创建（[namespaces(7)](https://man7.org/linux/man-pages/man7/namespaces.7.html)）；上限 `/proc/sys/user/max_user_namespaces`（[sysctl/user.rst](https://docs.kernel.org/admin-guide/sysctl/user.html)） | 无（3.8+） | 发行版可用 `kernel.unprivileged_userns_clone=0` 关掉（Debian/Arch/Ubuntu 都带过这个补丁，见 [Ubuntu spec](https://discourse.ubuntu.com/t/spec-unprivileged-user-namespace-restrictions-via-apparmor-in-ubuntu-23-10/37626) 的链接）；Ubuntu 23.10+ 默认用 AppArmor 限制（`kernel.apparmor_restrict_unprivileged_userns=1`，[spec](https://discourse.ubuntu.com/t/spec-unprivileged-user-namespace-restrictions-via-apparmor-in-ubuntu-23-10/37626)）；Docker 默认 seccomp profile 里 `unshare`/`setns`/`mount` 只在 `CAP_SYS_ADMIN` 下放行（[moby default.json](https://github.com/moby/profiles/blob/main/seccomp/default.json)）；**推论**：默认容器里 bwrap/nsjail 需要额外 capability 或自定义 seccomp。本机 Arch 默认可用（`unprivileged_userns_clone=1`，实测）。 |
| bwrap 的 mount namespace 操作 | 无特定版本（mount namespace 很老） | 非 root 时依赖 userns 的 `CAP_SYS_ADMIN`（[user_namespaces(7)](https://man7.org/linux/man-pages/man7/user_namespaces.7.html)） | 见上一行的所有坑；额外：`--unshare-cgroup`/`--unshare-cgroup-try` 与内核对 cgroup namespace 的支持相关（cgroup namespace 自 Linux 4.6，[namespaces(7)](https://man7.org/linux/man-pages/man7/namespaces.7.html) 的 `/proc/pid/ns/cgroup (since Linux 4.6)`） |
| Landlock | **ABI 1 = 5.13**；具体权限的引入见 §③ 表；本机内核 7.2.6 报 **ABI 10** | 无特权时 `PR_SET_NO_NEW_PRIVS`（[landlock_restrict_self(2)](https://man7.org/linux/man-pages/man2/landlock_restrict_self.2.html)）；ABI 10 起可用 `LANDLOCK_RESTRICT_SELF_NO_NEW_PRIVS` 免 prctl（[landlock.h](https://github.com/torvalds/linux/blob/master/include/uapi/linux/landlock.h)） | 需要 `CONFIG_SECURITY_LANDLOCK` 且 `lsm=` 列表含 landlock（man `landlock(7)` NOTES：`It must contain the string landlock to enable Landlock.`）；低于 ABI 1 的内核直接不可用；RHEL/Fedora 的现状本报告未查（见 §⑨）；**推论**：ABI 越低，能表达的文件权限越少（例如 ABI<3 时 `creat`/`O_TRUNC` 不受 `TRUNCATE` 管、ABI<2 时跨目录 rename 永远被拒） |
| seccomp-bpf | `CONFIG_SECCOMP_FILTER`（[seccomp(2)](https://man7.org/linux/man-pages/man2/seccomp.2.html)） | 无特权时 `no_new_privs`（同 man） | Docker 默认已自带一层 seccomp（[moby default.json](https://github.com/moby/profiles/blob/main/seccomp/default.json)），嵌套叠加时要留意层数与语义；x32/`__X32_SYSCALL_BIT` 的架构陷阱（[seccomp(2)](https://man7.org/linux/man-pages/man2/seccomp.2.html)） |
| `systemd-run --user` 的文件沙箱 | 视选项而定（`ProtectSystem=` Added in version 214 等，见 man） | user manager 里文件系统 namespace 选项需要 `PrivateUsers=` 配合才生效（[systemd.exec](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html) SANDBOXING 节） | `many of these sandboxing features are gracefully turned off`——**静默不生效**，不报错（同节）；只读目录选项不挡 AF_UNIX（同节） |
| rootless Podman / Docker | 需要内核 userns + subordinate id 机制；Docker 文档要求 `newuidmap`/`newgidmap` 与 `/etc/subuid`、`/etc/subgid`（[Docker Rootless](https://docs.docker.com/engine/security/rootless/)） | 无 root，但**管理员要先配 `/etc/subuid`/`/etc/subgid`**（[podman tutorial](https://github.com/containers/podman/blob/main/docs/tutorials/rootless_tutorial.md)） | 存储驱动/网络（pasta、slirp4netns）依赖发行版打包；容器内看到的不是宿主路径本身（不同 mount namespace） |
| nsjail | 与上面 userns 相同；cgroup v1/v2 支持（[README](https://github.com/google/nsjail)） | 无特权 userns，或 root（`--disable_clone_newuser`） | 需要系统有 Kafel 编译出的策略；README 的 Docker 示例用 `--privileged` |
| firejail | README 称 `runs on any Linux computer with a 3.x kernel version or newer`（[README](https://github.com/netblue30/firejail)） | **SUID**（README 自称 `an SUID sandbox program`；具体安装/`setuid` 位怎么设，本报告未逐字核实） | setuid 二进制本身是审计面；与 bwrap 的「拒绝 setuid 运行」取向相反（[bubblewrap SECURITY.md](https://github.com/containers/bubblewrap/blob/main/SECURITY.md)） |
| gVisor `runsc` | KVM 平台需硬件虚拟化；`systrap` 需 `SECCOMP_RET_TRAP`；`ptrace` 需 `PTRACE_SYSEMU`（[Platform Guide](https://gvisor.dev/docs/architecture_guide/platforms/)） | 无特权模式存在（官方有 [Rootless](https://gvisor.dev/docs/user_guide/rootless/) 文档页），本报告未细读 | 容器级；宿主路径要经 gofer/挂载暴露 |
| microVM（Firecracker / Kata） | KVM + 硬件虚拟化（[Firecracker](https://github.com/firecracker-microvm/firecracker)、[Kata](https://github.com/kata-containers/kata-containers)） | 通常需要访问 `/dev/kvm` 与相应权限 | 虚机级；guest rootfs/内核是额外构件 |
| wasmtime + WASI | 不依赖内核沙箱特性；依赖运行 wasm 模块 | 无特权（进程内运行时） | 目标必须是 wasm；文件系统靠 `--dir` 预开目录（[CLI options](https://docs.wasmtime.dev/cli-options.html)） |

---

## ⑨ 边界：这份文件回答不了什么

- **Debian 12 是否存在 AppArmor 的 userns 限制：未找到一手证据。** 我找到的 Debian 事实是 `apparmor` 包在 bookworm 为 **3.0.8-3**（[Debian changelog](https://metadata.ftp-master.debian.org/changelogs/main/a/apparmor/apparmor_3.0.8-3_changelog)，其中没有 `userns` 字样），以及 Debian 的 `kernel.unprivileged_userns_clone` 补丁存在（[Canonical spec 引用的 Debian patch](https://salsa.debian.org/kernel-team/linux/-/blob/master/debian/patches/debian/add-sysctl-to-disallow-unprivileged-CLONE_NEWUSER-by-default.patch)）。**我没有拿到「Debian 12 默认拒绝无特权 userns」或「Debian 12 有 `apparmor_restrict_unprivileged_userns`」的一手依据**；`packages.debian.org` 的 bookworm filelist 页面我抓到的内容无法确认（返回的正文里没有 `/usr/lib/sysctl.d/` 条目，且 `sources.debian.org` 被防爬页面挡住）。这里**不下结论**。
- **RHEL / Fedora / openSUSE 的现状未查**：`user.max_user_namespaces` 的发行版默认值、SELinux 是否参与 userns 限制，都不在这份文件里。
- **Ubuntu 24.04 之后（26.04/26.10）的 bubblewrap 包是否补上了 AppArmor profile 未查**：我只确认了 noble 的 filelist 没有 `/etc/apparmor.d/bwrap`；Launchpad 页显示 `bubblewrap (Ubuntu)` 的 latest release 是 `0.11.1-1ubuntu0.3`（2026-09-17 上传，可能属于更新的发行版），但**对应发行版的 filelist 未取**。
- **`landlock` crate 的 `pre_exec` 支持**：没有找到 crate 文档里关于 `pre_exec`/多线程的具体段落，所以只给了内核侧的约束事实，没写它「能不能直接放进 `pre_exec`」。另外 `birdcage` 的 Linux 后端具体怎么用 namespaces（是否也设 seccomp、是否 drop capability）我只读到 `lib.rs` 的公开 API 与 README，**没有逐行读 `src/linux.rs`**。
- **macOS 只做了最小覆盖**：本文件没有系统调研 Seatbelt/SBPL 的当前状态（`sandbox-exec` 是否仍随系统提供、`sandbox_init()` 的可用性），只在 §⑦ 记了 birdcage 自陈「macOS via `sandbox_init()`」。DSH 侧在 [01](./01-dsh-workspace-permissions-and-shell.md) §④ 有 `sandbox-exec -p <SBPL>` 的记录。
- **没有实测「bwrap 在 Ubuntu 24.04 上失败」**：这是引用 Launchpad bug 与包文件列表得出的，不是我在 Ubuntu 上跑出来的。**同理，Debian 与本机之外的发行版行为都是引用，不是实测。**
- **本机实测的适用范围有限**：所有「本机观测」都来自 Arch + 内核 7.2.6 + bwrap 0.13.0 + Landlock ABI 10，且这台机器的 `/` 是只读挂载（见「来源与版本」），因此**关于 `/etc` 的 `EROFS` 不能当作沙箱效果的证据**；只有工作区写从「可写」变 `EACCES` 那一组是对照干净的。
- **没有做性能测量**：bwrap 每次启动的挂载构造代价、Landlock 的判定开销，这份文件只字未提。
- **没有讨论「谁来定义允许写的工作区集合」**：那是设计问题（哪些根可写、`/tmp` 算不算、symlink/`..` 怎么规范化），[01](./01-dsh-workspace-permissions-and-shell.md) §③ 记了 DSH 的答案（canonicalize 后比对），本文件不重复也不评判。
