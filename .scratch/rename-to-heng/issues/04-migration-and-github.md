# 04 — 迁移与远端：搬旧状态、改仓库名

Status: done
Part of: [../spec.md](../spec.md) §5

**What to build:** 把本机旧状态搬到新路径，让旧会话与目标在新名下继续可用；然后改 GitHub 仓库名与本地目录名。

## 验收

- [x] `mv ~/.config/fs-agent ~/.config/heng`、`mv ~/.local/share/fs-agent ~/.local/share/heng`（3 个会话桶、2 个目标已在新路径）
- [x] 旧会话在新名下读得出来：`heng sessions ls` 列出搬过来的那些会话；`~/.config/heng/config.toml` 里写死的 `fs-agent-mcp-time` 也改成了 `heng-mcp-time`
- [x] `gh repo rename heng --repo fortystory/fs-agent`，本地 remote 已更新
- [x] `gh repo edit --description` 写明「衡（heng）：自用的 coding agent harness」
- [x] 本地工作目录改名 `fs-agent` → `heng` —— **2026-10-07 由维护者完成**：内容已全部在 `~/code/fortystory/heng`（remote 指向 `fortystory/heng`、`heng --version` 印 `heng 0.1.0`），旧路径 `~/code/fortystory/fs-agent` 只剩一个空目录。

  当时留给维护者的原因：那是**当前会话的 cwd 与文件沙箱的工作区**，agent 在里面改自己的目录会把这两样一起打断。会话桶按 cwd slug 分，改名之后老会话在 `heng` 这个工作区下不再直接可见 —— `heng sessions ls --all` 仍看得到（已核：旧 `fs-agent` 桶的会话都在），`-c <id>` 也仍续得上。
