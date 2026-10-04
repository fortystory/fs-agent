# 13 — server 进程：环境白名单与可写根

Type: implement
Status: done
Part of: ../map.md
Blocked by: 12

> 规格：[`../spec.md`](../spec.md) §4。

## 目标

server 子进程只拿到「配置里显式声明的环境变量 + 最小必需」，工作区外的写只有显式声明才放开。

## 具体行为

1. **`env_clear()` 之后只注入**：server 配置 `env` 里声明的项 + 最小必需的 `PATH` / `HOME` /
   `LANG`。**清洗必须发生在交给 `rmcp` 之前** —— 它不做环境清洗。
2. **`writable_roots`**：server 配置里的路径追加到这台 server 的沙箱可写集；不声明就只有「会话
   工作区 + 沙箱默认的那列缓存目录」。**不给默认的 per-server 临时目录**。
3. **`stderr` 显式 piped**（`rmcp` 默认 `inherit`），好让 server 的崩溃信息进得了日志。

## 验证

`cargo test` + `cargo clippy --all-targets`：

1. **白名单**：父进程导出的一个假密钥**不出现在**子进程环境里；配置 `env` 里声明的出现（用假 spawn
   记录 argv / env，真进程留给票 12 的集成测试）。
2. **可写根**：声明后那个路径可写、未声明的区外路径仍然只读（沙箱生效）。
3. **stderr**：假 server 往 stderr 写一行，断言它被接住而不是丢掉。

## 不做什么

- 不做信任位（票 14）—— 本票只做「默认过沙箱、默认白名单」。
- 不动 `bash` 那一侧的环境处理。

## 评论

- 2026-10-03 落地（`Status: done`）。落点：`src/mcp/rmcp_client.rs`（`ConnectOptions` /
  `StderrSink` / `BASE_ENV_KEYS`，`stdio_transport` 里的 `env_clear()` + 白名单 +
  `Sandbox::with_grants` + `stderr` piped）、`src/mcp/mod.rs`（re-export）、`src/cli.rs`
  （`mcp_service` 多收 `env` 与 `home`，stderr 走措辞层）、`src/render/wording.rs`
  （`mcp_server_stderr`）、`tests/support/fake_mcp_server.rs`（`env` / `write` 工具与一行
  stderr）、`tests/mcp_process.rs`（新，4 条：白名单、可写根、stderr、`sandbox` 位）。
- **一处坑（值得记）**：stderr 管道**必须总有人读**。第一版只在有诊断口时才 spawn 读取任务，
  于是没有 sink 的会话里读端被丢掉，server 的 `eprintln!` 拿到 EPIPE —— 而 `eprintln!` 写失败
  是 panic：一个只是想抱怨一句的 server 会当场死掉，连接报 `connection closed: discover
  response`。现在无论有没有 sink 都把那条管道读掉（没 sink 就丢）。
- **可写根复用 `Sandbox::with_grants`**（与 `workspace` 模式的升级批准同一条通道）：沙箱只能绑
  已经存在的路径，所以一条不存在的声明会明确失败，而不是静默不生效。
- **白名单写死三个键**（`PATH` / `HOME` / `LANG`）：黑名单 fail open，而这一层与沙箱同一条
  fail-closed 的调性。
- 验证：`cargo test --test mcp_process --test mcp_stdio`（4 + 5 条全绿）·
  `cargo clippy --all-targets`（无 warning）· `cargo fmt` · `python3 scripts/check-language.py`
  全通过。
