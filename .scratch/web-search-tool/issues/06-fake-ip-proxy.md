# 06 — fake-IP 代理下的 `web_fetch`：一个 `trust_proxy_dns` 例外

Type: implement
Status: done
Blocked by: 03

> 来源：维护者 2026-10-03 报的缺陷 —— 开着 Clash/Mihomo（fake-IP + `https_proxy`）的机器上，
> `web_fetch` 对**每一个**域名都报 `WEB_BLOCKED_URL`。规格回改在
> [`../spec.md`](../spec.md) §5（新增的那条例外）与 §9（字段表）。

## 目标

让「DNS 被代理接管」这台机器上的 `web_fetch` 能用，同时**不**放宽字面内网 IP 的判据。
缺省行为一个字不变，要用得显式打开 `[web] trust_proxy_dns = true`。

## 现象与复现（2026-10-03 实测）

- 环境与报障完全一致：`nameserver 192.168.3.1`、`www.accuweather.com → 198.18.0.26`、
  `api.open-meteo.com → 198.18.0.29`、`https_proxy=http://127.0.0.1:7897`。
- 原来的行为：`resolve_public` 解析出 `198.18.0.26` → `is_public_unicast_with` 拒
  （`198.18/15` 是 RFC 2544 保留段）→ `WEB_BLOCKED_URL`。
  **判据是对的，错的是它的输入是假的** —— fake-IP 代理把每个域名都解析成假地址，真实解析在
  代理那边做。
- 复现 loop：一条临时集成测试直接调 `HttpFetch::resolve_public`（只解析、不发请求），
  7.9 秒编译、0.00 秒运行、确定性变红。

## 具体行为

- `[web] trust_proxy_dns`（缺省 `false`）打开时：**主机名**不解析、不固定连接地址，解析与连接
  交给代理；地址集为空时不再调 `resolve_to_addrs`（它会把那个 host 覆盖成空集，那不是「不固定」）。
  URL 校验（scheme / 内嵌凭据 / 长度）照旧。**字面 IP 仍然整体校验** —— 那条判据不需要 DNS。
- 关着时：与从前逐字相同（解析一次 + 整体校验 + 固定连接）。
- 两条路径的拒绝共用同一句措辞（`blocked_error`）。

## 为什么不能只做「检测到代理就放宽」

实测：`http://192.168.3.1/` **经代理返回 200** —— 代理会把局域网请求转发出去，所以它不是
SSRF 的防线。放宽必须只针对「本地 DNS 不可信」这一件事，字面 IP 那条判据要留着。

## 验证

- `tests/web_fetch_transport.rs`：代理路径下主机名返回空（不解析、不固定）；代理路径下
  `192.168.3.1` / `10.0.0.1` / `127.0.0.1` / `169.254.169.254` / `[::1]` 仍然
  `WEB_BLOCKED_URL`。
- `tests/config_profiles.rs`：缺省为 `false`；写进配置能读出来。
- 真机（一次性诊断 harness，验证完即删）：打开开关后三个域名 `resolve_public` 都回 `Ok([])`，
  端到端抓到 `api.open-meteo.com` 的 **200 与正文**。
- `cargo test` 全量、`cargo clippy --all-targets`、`cargo fmt --check`、
  `python3 scripts/check-language.py` 全绿。

## 不做什么

- 不做「检测到代理就自动放宽」，也不猜 fake-IP 网段：要人显式声明这台机器的 DNS 不可信。
- 不动严格路径的任何一条判据；不动搜索这一侧。
- 不把 `no_proxy` 的语义接进来（打开开关后，主机名的目标校验整条交给代理）。

## Comments

- **落地（2026-10-03）**：`HttpFetch` 加 `trust_proxy_dns` 与 `with_trust_proxy_dns()`；
  `resolve_public` 加代理分支；`client_for` 在地址集为空时跳过 `resolve_to_addrs`；
  配置加 `[web] trust_proxy_dns`（缺省 `false`）；拒绝措辞抽成 `blocked_error` 两处共用。
  诊断用的临时 harness 已删除。
