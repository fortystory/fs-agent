# 03 — `web_fetch` 的安全传输层：URL 校验、SSRF 与有界读取

Type: implement
Status: ready-for-agent
Blocked by: 01

> 规格：[`../spec.md`](../spec.md) §5 —— 那一节逐条对着 DSH 的 `dsh-web-fetch-http` 写的。

## 目标

给 `FetchProvider` 一个能安全取字节的实现：**拒绝非公开目标**、**连接固定到已校验的地址**、
**重定向逃不出源站**、**每个响应都有上限**、**不发送任何凭据**。这一层不负责把 HTML 变成给人
看的东西 —— 那是 [票 04](04-fetch-tool-and-extraction.md)。

## 现状（2026-10-03 核实，改前先复核）

- 沙箱**不管网络**：bwrap 的参数只有 `--unshare-user/pid/ipc/uts`（`src/tools/sandbox.rs:299-302`），
  `docs/sandbox.md:22` 自己写着「网络——`curl` 带着 key 出去这一层拦不住」，ADR 0006 把网络隔离
  单独排期。**所以这一层就是全部的防线**，没有第二道网。
- `reqwest 0.13` 的特性只有 `json` / `stream` / `native-tls`（`Cargo.toml:22`）—— 处理压缩响应
  （gzip/br/zstd）可能要加特性；**改前先确认要不要**。
- 重定向与退避的既有写法可参考 `src/provider/openai.rs:47-87` 与 `:895-905`（本票不做限流，
  只借形状）。
- `ToolError` / `ToolOutput` 的形状在 `src/tools/tool.rs:35-55`。

## 落点

`src/web/fetch_http.rs`（新）、`Cargo.toml`（可能）、`tests/`。

## 具体行为

1. **URL 校验**：只接受 `http:` / `https:`；拒绝内嵌凭据；长度上限 2,048 字符（照 DSH）。
2. **SSRF（最不能省的一条）**：主机名**只解析一次**；只要结果里有**任何一个** IPv4/IPv6 地址
   不是公共单播地址，就**整体拒绝**；**把连接固定到已校验的地址集合**（不让第二次解析被换成
   别处的目标）；IPv6 要发现 DNS64 前缀并拒绝指向非公开 IPv4 的转换地址。
3. **重定向**：最多 5 跳；**每跳同源重定向都重新解析与校验**；跨源重定向直接失败
   （要求模型重新调用）。
4. **四道上限**：响应字节（5 MB）、解码字符（100k）、跳数（5）、时间（30s，资源兜底 ——
   面向模型的工具预算归 timeout 那一层）。
5. **解码**：charset 只认 `Content-Type`（缺省 UTF-8）；不支持的类型与二进制直接拒绝。
6. **不发送凭据**：请求匿名，带一个诚实的 `User-Agent`（含项目名与版本）。
7. **非 2xx 不是错误**：状态码是结果的一部分，往下传给票 04 渲染。

## 验证

`cargo test` + `cargo clippy --all-targets`：

1. **URL 判据（纯函数单测）**：`127.0.0.1`、`169.254.169.254`、`10.0.0.1`、`192.168.0.1`、
   `[::1]`、内嵌凭据的 URL、`file:` / `ftp:` 一律拒；`https://example.com` 通过。
2. **重定向**：同源重定向跟随且**每跳重新校验**；跨源重定向失败；重定向到内网地址失败。
3. **上限**：超过字节上限的响应被拒；超过字符上限的正文被截断（截断标志往下传）。
4. **内容类型**：`application/octet-stream`、缺 `Content-Type`、无法识别的 charset 三种情况
   按 spec §5 处理。
5. **不发凭据**：断言请求里没有 `Authorization` / `Cookie` 之类（用假 HTTP 接缝）。
6. **零网络**：所有测试都不发真请求。

## 不做什么

- 不做正文提取与 markdown 转换（票 04）。
- 不做缓存、robots.txt、按域限流（spec 的《明确不做》）。
- 不做 JS 渲染、不做 headless browser。
- 不做域名白名单（spec §6 的「不加额外防线」）。
