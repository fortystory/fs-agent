# 沙箱的 argv 拼装：`wrap()` 纯函数

Type: implement
Status: done
Blocked by: —

> 规格：`.scratch/sandbox/spec.md` §1（形状）、§4（边界）。
> 这一票不碰进程、不读环境：它只把一个 argv 变成另一个 argv，所以**在没有 bubblewrap 的机器上也能断言每一个分支**。

## 目标

新增 `src/tools/sandbox.rs`，核心是一个纯函数：给定原 argv、cwd 与一份「可写根 / 遮罩 / 保护路径」清单，拼出 bubblewrap 的完整 argv。

## 落点

`src/tools/sandbox.rs`（新增）、`src/tools/mod.rs`（模块注册）、`tests/sandbox.rs`（新增）。

## 具体行为

1. **函数形状**：`wrap(argv: &[String], cwd: &Path, spec: &SandboxSpec) -> Vec<String>`。不 spawn、不读环境；除了下面第 4 条要判存在性，不做别的 IO。
2. **flag 顺序**（逐字）：
   ```
   bwrap --new-session --die-with-parent --ro-bind / /
         [每个可写根一条 --bind <root> <root>]
         --tmpfs /tmp --dev /dev --proc /proc
         --unshare-user --unshare-pid --unshare-ipc --unshare-uts
         [每个遮罩目录两条 --tmpfs <dir> --remount-ro <dir>]
         [每个保护路径一条 --ro-bind <path> <path>]
         -- <原 argv>
   ```
   **顺序有意义**：`--ro-bind / /` 必须在所有 `--bind` 之前；保护路径的 `--ro-bind` 必须在可写根的 `--bind` **之后**——顺序反了等于没保护。
3. **cwd 与每个可写根都先 canonicalize**：`--bind` 两边都吃绝对路径，而 symlink 会骗过「看起来在工作区里」的判定。
4. **存在性**：遮罩目录与保护文件**不存在就整条跳过**。实测过：`--tmpfs` 一个不存在的目标会让 bwrap 直接报错退出（`bwrap: Can't create file …: Read-only file system`）。
5. **`.env` 家族**：只压回**存在**的那几个文件，`.example` / `.sample` / `.template` 三个后缀除外 —— 与 `src/permissions.rs` 的 `ENV_TEMPLATE_SUFFIXES` **用同一份常量**，不要抄第二份。
6. **保护路径清单**：工作区里的 **`.git/config` 与 `.git/hooks`**（**不是整个 `.git`**——那样会把 `git add` / `git commit` 一起挡死，它们写的第一样东西就是 `.git/index.lock`）+ 上面那批 `.env` 家族文件。shell rc 文件（`.bashrc` 等）不在工作区里，`--ro-bind / /` 已经管了，这里不处理。
7. **`mode = "off"` 退化成单位函数**（原样返回输入）。这一票只留出这个分支，配置怎么解析是票 02 的事。

## 测试

`tests/sandbox.rs`，全是纯函数断言：

- 最小组合（一个可写根 + 一个遮罩目录 + 一个保护路径）的 flag 序列**逐字比对**；
- 多个可写根 → 多条 `--bind`，顺序与输入一致；
- 遮罩目录成对出现（`--tmpfs` 紧接 `--remount-ro`）；
- 保护路径出现在**所有** `--bind` 之后；
- 不存在的遮罩目录 / 保护文件被跳过；
- `.env` 在保护之列、`.env.example` 不在；
- `.git/config` 与 `.git/hooks` 在保护之列，而 **`.git/index` 不在**（它必须可写，否则 `git add` / `git commit` 全废）；
- cwd 与可写根被 canonicalize（拿一个 symlink 路径验）；
- `mode = "off"` → 输出等于输入。
