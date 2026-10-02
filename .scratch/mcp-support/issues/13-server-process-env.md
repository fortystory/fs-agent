# 13 — server 进程：环境白名单与可写根

Type: implement
Status: ready-for-agent
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
