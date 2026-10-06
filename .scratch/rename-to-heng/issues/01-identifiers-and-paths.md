# 01 — 标识符与配置路径：fs-agent → heng

Status: done
Part of: [../spec.md](../spec.md) §2

**What to build:** 包名、lib 名、两个 bin 名、命令前缀、配置与数据路径、技能根、环境变量、对外标识全部换成 `heng`；不引入任何行为变化。

## 验收

- [x] `Cargo.toml`：`name = "heng"`、`default-run = "heng"`、`[lib] name = "heng"`、`[[bin]] name = "heng"`、`[[bin]] name = "heng-mcp-time"`；`description` 写明 harness 定位
- [x] `src/config.rs`：配置路径 `~/.config/heng/config.toml`、会话根与目标根在 `$XDG_DATA_HOME/heng`、沙箱掩码、`HENG_MODEL`
- [x] `src/context/skills.rs`：项目级根 `.heng`、用户级 `~/.config/heng`
- [x] `src/render/wording.rs`：`heng: ` 前缀、usage / help 全文、`identity()` 跟着 `CARGO_PKG_NAME` 自动变
- [x] `src/agent.rs`：身份提示词改成「你是衡（heng），一套自用的 coding agent harness…」
- [x] `src/mcp/rmcp_client.rs`、`src/web/fetch_http.rs`：client 名与 user-agent
- [x] `scripts/tui-startup-check.py`：锚点跟着 `--version` 走
- [x] `tests/` 里的二进制名、路径与文案断言全部同步
- [x] `cargo build` + `cargo test` 全绿；`--version` 印 `heng {version}`
