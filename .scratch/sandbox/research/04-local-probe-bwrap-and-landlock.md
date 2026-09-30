# 实测：bwrap 与 Landlock 在这台开发机上真的做了什么

## 这份文件是什么

不是文献调研，是**在这台机器上真跑出来的结果**。姊妹篇 [`02`](02-linux-sandbox-primitives-and-tools.md) 讲机制与可用性前提、[`03`](03-agent-sandbox-precedents.md) 讲同类产品的先例；这份只回答一个问题：**那两条路在这台机器上跑得起来吗，拦得住什么。**

观察时点 2026-09-30，机器 Arch Linux、内核 `7.2.6-arch2-1`、x86_64。全部结论来自当场执行的命令，命令都写在下面。

## 先说方法论：第一版实验是无效的

**这个会话自己就跑在一个 `workspace-write` 的内核沙箱里。** `/proc/mounts` 里读得出来：

```
/dev/nvme0n1p2 /                                      btrfs ro,...
tmpfs          /tmp                                   tmpfs ro,...
tmpfs          /tmp                                   tmpfs rw,...
/dev/nvme0n1p2 /home                                  btrfs ro,...
/dev/nvme0n1p2 /home/forty/code/fortystory/fs-agent   btrfs rw,...
```

即「整机只读 + `/tmp` 可写 + 工作区单独 rw bind」——**宿主本身就是沙箱**。

后果很直接：我第一版实验里，「在 bwrap / Landlock 里写家目录被拒」看起来像是沙箱在拦，但**不套沙箱也一样被拒**：

```sh
$ touch ~/host-write-probe
touch: 无法 touch '/home/forty/host-write-probe': 只读文件系统
```

那是挂载层拦的，不是沙箱。**要证明沙箱拦住了什么，必须挑一个宿主自己能写的目标来做对照** —— 这台机器上只有工作区和 `/tmp` 两个。下面的结论都经过这层对照；上面那条无效归因留着当反面教材。

## ① bubblewrap

```sh
$ bwrap --version
bubblewrap 0.13.0

$ bwrap --ro-bind / / --dev /dev --proc /proc --unshare-pid --die-with-parent /bin/echo bwrap-ok
bwrap-ok
```

**干净对照**（宿主写 `/tmp` 成功，同一个 `/tmp` 在沙箱里被拒）：

```sh
$ touch /tmp/host-can-write && echo "宿主写 /tmp: OK"
宿主写 /tmp: OK

$ bwrap --ro-bind / / --bind "$PWD" "$PWD" --dev /dev --proc /proc --die-with-parent \
      bash -c 'touch /tmp/probe'
touch: 无法 touch '/tmp/probe': 只读文件系统          # EROFS
```

结论：**能起来，也确实拦住了宿主可写的位置**。「整机只读 + 工作区可写」这个形状在这台机器上成立。

**网络默认不隔离**（这条容易漏）：

| 形状 | `1.1.1.1:443` |
| --- | --- |
| 不写 `--unshare-net` | **通** |
| 写 `--unshare-net` | 不通（`OSError`） |

另外 `--ro-bind / /` 是**造出来的视图**，不是默认：什么都不 bind 时 root 是空 tmpfs。这一点 [`02`](02-linux-sandbox-primitives-and-tools.md) §② 有更细的实测。

## ② Landlock

```sh
landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION) → ABI = 10
```

内核 `7.2.6` 报 **ABI 10**（比本机 man-pages 的 ABI 表还新一档，[`02`](02-linux-sandbox-primitives-and-tools.md) §③ 有版本表）。

规则集内容：`/` 给「执行 + 读 + 列目录」，工作区给全部文件权限，`/tmp` **故意不给**。施加后（`PR_SET_NO_NEW_PRIVS` + `LANDLOCK_RESTRICT_SELF`）当场探测：

| 探测 | 结果 |
| --- | --- |
| 写工作区（已授权） | 允许 |
| 写 `/tmp`（**宿主可写**，未授权） | **拒绝 —— Permission denied** |
| 写家目录（宿主自己就只读） | 拒绝（**归因无效**，见方法论一节） |
| 读 `/etc/hostname` | 允许 |
| 执行 `/usr/bin/true` | 允许 |
| 父进程（沙箱外）写 `/tmp` | 正常 —— 限制只影响施加它的进程与其后代 |

结论：**Landlock 这条路在这台机器上跑得通**，allow-list 语义、无特权、不可撤销、不牵连父进程，全部实测到位。

## ③ 拒绝信号长什么样（这条影响实现）

| 来源 | errno | `LC_ALL=C` 的文本 | 中文 locale 的文本 |
| --- | --- | --- | --- |
| bwrap（只读 mount） | `EROFS` | `cannot touch '...': Permission denied` | 「无法 touch '...'：权限不够」 |
| Landlock（LSM 拒绝） | `EACCES` | `Permission denied` | 「权限不够」 |
| 命令自己失败（例如写 0444 文件） | `EACCES` | 同上 | 同上 |

两点从这张表读得出来：**①** 文本随 locale 变，任何「匹配 stderr 方言」的判定都会在中文环境下失效；**②** 退出码全都是 1，命令自身失败与沙箱拒绝在退出码上分不开。所以「这次失败是不是沙箱拦的」需要一个**自己造的判据**（哨兵退出码，或者自己包一层 runner 而不是读命令的 stderr）。

## ④ `systemd-run --user`（[`03`](03-agent-sandbox-precedents.md) 没提的一条现成路）

```sh
$ systemd-run --user --pipe --wait --collect \
    -p ReadOnlyPaths=/ -p ReadWritePaths="$PWD" -p PrivateTmp=yes -p NoNewPrivileges=yes \
    /bin/bash -c 'touch "$PWD/target/probe"; touch $HOME/x'
工作区写: OK
touch: 无法 touch '/home/forty/x': 只读文件系统
```

能跑、形状对。但 [`02`](02-linux-sandbox-primitives-and-tools.md) §⑧ 引 systemd 文档的警告要一并记住：**这些沙箱选项「gracefully turned off」——静默不生效，不报错**。拿它当边界，必须自己验一遍生效没有。

## ⑤ 本机没能走通的路

- `podman`：`Failed to obtain podman configuration: set sticky bit on: chmod /run/user/1000/libpod: read-only file system`（在当前环境里起不来；容器路线本机未验证）。
- `nsjail` / `firejail` / `srt` / `landlock-run`：未安装。

## ⑥ 边界

- **这些结论是「机制确实能施加」，不是「机制在任意环境下可用」。** 发行版矩阵与内核门槛看 [`02`](02-linux-sandbox-primitives-and-tools.md) §⑧。
- **只测了 Linux。** macOS 的 Seatbelt 一行都没跑（本机也不是 macOS）。
- **没测逃逸。** 没有验证任何已知绕过（`/proc` 访问、继承的 fd、unix socket 等），这份文件不是安全性评估。
- **本机没有 Ubuntu 24.04**，所以 [`02`](02-linux-sandbox-primitives-and-tools.md) §① 第 5 条那颗地雷是引用，不是复现。
