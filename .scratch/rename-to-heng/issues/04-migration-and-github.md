# 04 — 迁移与远端：搬旧状态、改仓库名

Status: ready-for-walkthrough
Part of: [../spec.md](../spec.md) §5

**What to build:** 把本机旧状态搬到新路径，让旧会话与目标在新名下继续可用；然后改 GitHub 仓库名与本地目录名。

## 验收

- [x] `mv ~/.config/fs-agent ~/.config/heng`、`mv ~/.local/share/fs-agent ~/.local/share/heng`（3 个会话桶、2 个目标已在新路径）
- [x] 旧会话在新名下读得出来：`heng sessions ls` 列出搬过来的那些会话；`~/.config/heng/config.toml` 里写死的 `fs-agent-mcp-time` 也改成了 `heng-mcp-time`
- [x] `gh repo rename heng --repo fortystory/fs-agent`，本地 remote 已更新
- [x] `gh repo edit --description` 写明「衡（heng）：自用的 coding agent harness」
- [ ] 本地工作目录改名 `fs-agent` → `heng` —— **留给维护者**：当前会话的 cwd 与文件沙箱的工作区都钉在旧路径上，agent 在里面改自己的目录会把这两样一起打断。改完后新会话在新路径下开：

  ```sh
  mv ~/code/fortystory/fs-agent ~/code/fortystory/heng
  ```

  注意：会话桶是按 cwd slug 分的，改名之后老会话在 `heng` 这个工作区下不再直接可见（`sessions ls --all` 仍看得到，`-c <id>` 也仍续得上）。
