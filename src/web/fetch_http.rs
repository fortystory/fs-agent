//! `web_fetch` 的传输层：URL 校验、SSRF 防护与有界读取
//! （`.scratch/web-search-tool/spec.md` §5）。
//!
//! 这一层**不**把 HTML 变成给人看的东西 —— 那是工具层的事。它只保证一件事：出去的这一次
//! 请求，目标是我们愿意去的、回来的字节是有界的。
//!
//! 沙箱不管网络（[ADR 0006](../../docs/adr/0006-sandbox-by-bubblewrap.md)），所以**这一层就是
//! 全部的防线**，没有第二道网。做法照 DSH 的 `dsh-web-fetch-http`：
//!
//! * 主机名**只解析一次**，结果里只要有一个地址不是公共单播就整体拒绝；
//! * 连接**固定**到那批已校验的地址（不让第二次解析被换成别处的目标）；
//! * 重定向**只走同源**，且每一跳重新解析与校验；
//! * 字节、字符、跳数、时间各有上限。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::header::{HeaderValue, ACCEPT, LOCATION, USER_AGENT};
use reqwest::redirect::Policy;
use url::Url;

use super::{FetchOutcome, FetchProvider, FetchedContent, WebError, WebErrorCode};

/// 后端名，与配置里的 `fetch_provider` 对应。
pub const HTTP_FETCH_PROVIDER: &str = "http";

/// URL 的字符数上限，照 DSH。
pub const MAX_URL_CHARS: usize = 2_048;

/// 一次响应最多收多少字节（照 DSH 的 5 MB）。到这儿就拒，而不是截断 —— 一个 5 MB 的页面
/// 不是「长了一点」，它是另一个东西。
pub const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;

/// 最多跟几跳重定向，照 DSH。
pub const MAX_REDIRECTS: usize = 5;

/// 一次请求带的 `Accept`。
const ACCEPT_HEADER: &str =
    "text/html,application/xhtml+xml,text/plain;q=0.9,application/json;q=0.8";

/// 一次抓取的后端。
pub struct HttpFetch {
    /// 诚实的身份：含项目名与版本，不伪装浏览器。
    user_agent: String,
    /// 解码后的字符上限（来自 `[web] fetch_max_chars`）。
    max_chars: usize,
    /// 一次抓取的墙钟上限（来自 `[web] fetch_timeout_ms`）。
    timeout: Duration,
    /// 这个网络上 DNS64 的前缀（如果有）。探测一次，之后一直用。
    ///
    /// RFC 7050 的做法：查 `ipv4only.arpa`，它回的 AAAA 地址形如 `<前缀>:<192.0.0.170>`，
    /// 前缀就在里面。**不知道前缀就没法认出转换地址**，而一个指向 `10.0.0.1` 的 DNS64
    /// 地址长得和一个普通的全球单播 IPv6 地址一模一样。
    dns64: tokio::sync::OnceCell<Option<Ipv6Addr>>,
}

impl HttpFetch {
    pub fn new(max_chars: usize, timeout: Duration) -> Self {
        Self {
            user_agent: format!("fs-agent/{}", env!("CARGO_PKG_VERSION")),
            max_chars: max_chars.max(1),
            timeout,
            dns64: tokio::sync::OnceCell::new(),
        }
    }

    /// 一次 GET 的请求：**只有**身份与 `Accept` 两个头。
    ///
    /// 单独一处，因为「不发送任何凭据」是这一层的一条承诺，而承诺要能被断言：抓取是匿名的，
    /// 没有 `Authorization`、没有 `Cookie`、没有 `Referer`。
    pub fn build_request(
        &self,
        client: &reqwest::Client,
        url: &Url,
    ) -> Result<reqwest::Request, WebError> {
        let user_agent = HeaderValue::from_str(&self.user_agent).map_err(|error| {
            WebError::new(
                WebErrorCode::ProviderError,
                format!("User-Agent 不合法：{error}"),
            )
        })?;
        client
            .get(url.as_str())
            .header(USER_AGENT, user_agent)
            .header(ACCEPT, ACCEPT_HEADER)
            .build()
            .map_err(|error| {
                WebError::new(
                    WebErrorCode::ProviderError,
                    format!("请求构造失败：{error}"),
                )
            })
    }

    /// 一个把 DNS 固定到 `addrs` 的 client。
    ///
    /// 每次抓取现建一个：`resolve_to_addrs` 是按 host 的覆盖，而「固定」正是这一层的要点
    /// —— 校验过的地址与真正连上去的地址必须是同一批。
    fn client_for(
        &self,
        host: &str,
        addrs: &[SocketAddr],
        budget: Duration,
    ) -> Result<reqwest::Client, WebError> {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(budget)
            .redirect(Policy::none())
            .resolve_to_addrs(host, addrs)
            .build()
            .map_err(|error| {
                WebError::new(
                    WebErrorCode::ProviderError,
                    format!("HTTP client 建不出来：{error}"),
                )
            })
    }

    /// 这个网络上的 DNS64 前缀（探测一次）。
    async fn dns64_prefix(&self) -> Option<Ipv6Addr> {
        *self
            .dns64
            .get_or_init(|| async { probe_dns64().await })
            .await
    }
}

#[async_trait]
impl FetchProvider for HttpFetch {
    fn name(&self) -> &str {
        HTTP_FETCH_PROVIDER
    }

    async fn fetch(&self, url: &str) -> Result<FetchOutcome, WebError> {
        let deadline = Instant::now() + self.timeout;
        let mut current = validate_url(url)?;
        let mut hops = 0usize;

        loop {
            let addrs = self.resolve_public(&current).await?;
            let budget = deadline.saturating_duration_since(Instant::now());
            if budget.is_zero() {
                return Err(timeout_error(&current));
            }
            let host = current.host_str().unwrap_or_default().to_owned();
            let client = self.client_for(&host, &addrs, budget)?;
            let request = self.build_request(&client, &current)?;
            let response = client
                .execute(request)
                .await
                .map_err(|error| transport_error(&current, error))?;

            let status = response.status();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);

            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| {
                        WebError::new(
                            WebErrorCode::ProviderError,
                            format!("{current} 回了 {status}，但没有 `Location` 头"),
                        )
                    })?;
                hops += 1;
                current = next_hop(&current, location, hops)?;
                continue;
            }

            let (bytes, overflowed) = read_bounded(response, MAX_RESPONSE_BYTES).await?;
            if overflowed {
                return Err(WebError::new(
                    WebErrorCode::FetchTooLarge,
                    format!("{current} 的响应超过 {} 字节，拒绝", MAX_RESPONSE_BYTES),
                ));
            }
            let (content, truncated) =
                decode_body(&current, content_type.as_deref(), &bytes, self.max_chars)?;
            return Ok(FetchOutcome {
                url: current.to_string(),
                status: status.as_u16(),
                content,
                truncated,
            });
        }
    }
}

impl HttpFetch {
    /// 解析这一次要去的主机名，并**整体**校验它。
    ///
    /// 只解析一次，而且是把连接固定到的那一次：第二次解析是 SSRF 的经典入口（第一次校验、
    /// 第二次连到别处）。返回的就是要被固定住的那批地址。
    ///
    /// 是 `pub` 的，好让 SSRF 判据能在不发任何请求的前提下被逐条钉住。
    pub async fn resolve_public(&self, url: &Url) -> Result<Vec<SocketAddr>, WebError> {
        let host = lookup_host_name(url)?;
        let port = url.port_or_known_default().ok_or_else(|| {
            WebError::new(
                WebErrorCode::InvalidUrl,
                format!("{url} 没有端口，也不是一个能推出来的 scheme"),
            )
        })?;
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|error| {
                WebError::new(
                    WebErrorCode::ProviderError,
                    format!("`{host}` 解析不了：{error}"),
                )
            })?
            .collect();
        if addrs.is_empty() {
            return Err(WebError::new(
                WebErrorCode::ProviderError,
                format!("`{host}` 解析不出任何地址"),
            ));
        }

        // DNS64 前缀只在真有全球单播 IPv6 地址时才去探：探它要发一次 DNS 查询，而
        // `::1`、v4-mapped、已知的 NAT64 段都不需要它就能判出来。
        let dns64 = if addrs
            .iter()
            .any(|addr| matches!(addr.ip(), IpAddr::V6(v6) if is_global_v6(v6)))
        {
            self.dns64_prefix().await
        } else {
            None
        };
        for addr in &addrs {
            if !is_public_unicast_with(addr.ip(), dns64) {
                return Err(WebError::new(
                    WebErrorCode::BlockedUrl,
                    format!(
                        "`{host}` 指向 {}，那不是公共单播地址（内网、环回、链路本地、\
                         保留段与转换地址都拒）",
                        addr.ip()
                    ),
                ));
            }
        }
        Ok(addrs)
    }
}

// --- 纯函数：URL、地址、跳数 ----------------------------------------------

/// 校验一个模型给的 URL。
///
/// 只接受 `http:` / `https:`；拒绝内嵌凭据；长度上限 2 048 字符（照 DSH）。这几条都是纯函数，
/// 所以它们能在没有网络的情况下被逐条钉住。
pub fn validate_url(raw: &str) -> Result<Url, WebError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(WebError::new(WebErrorCode::InvalidUrl, "URL 是空的"));
    }
    if raw.chars().count() > MAX_URL_CHARS {
        return Err(WebError::new(
            WebErrorCode::InvalidUrl,
            format!("URL 超过 {MAX_URL_CHARS} 个字符"),
        ));
    }
    let url = Url::parse(raw).map_err(|error| {
        WebError::new(
            WebErrorCode::InvalidUrl,
            format!("`{raw}` 解析不了：{error}"),
        )
    })?;
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(WebError::new(
                WebErrorCode::InvalidUrl,
                format!("只抓 `http` / `https`，拿到的是 `{other}`"),
            ))
        }
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(WebError::new(
            WebErrorCode::InvalidUrl,
            format!("`{raw}` 里内嵌了凭据，拒绝"),
        ));
    }
    if url.host_str().is_none() {
        return Err(WebError::new(
            WebErrorCode::InvalidUrl,
            format!("`{raw}` 没有主机名"),
        ));
    }
    Ok(url)
}

/// 交给系统解析器的主机名。
///
/// `Url::host_str` 对 IPv6 字面量返回的是带方括号的形式（`[::1]`），而解析器不认那个形状 ——
/// 它会变成一条「名字解析不了」，把「这是内网地址，拒」这条结论盖掉。走 `Url::host` 拿到那个
/// 地址本身，再按字面量重新写。
fn lookup_host_name(url: &Url) -> Result<String, WebError> {
    match url.host() {
        Some(url::Host::Domain(domain)) => Ok(domain.to_owned()),
        Some(url::Host::Ipv4(ip)) => Ok(ip.to_string()),
        Some(url::Host::Ipv6(ip)) => Ok(ip.to_string()),
        None => Err(WebError::new(
            WebErrorCode::InvalidUrl,
            format!("{url} 没有主机名"),
        )),
    }
}

/// 这个地址是不是一个可以去的公共单播地址。
pub fn is_public_unicast(ip: IpAddr) -> bool {
    is_public_unicast_with(ip, None)
}

/// 同上，外加上「这个网络上的 DNS64 前缀」这一条信息。
pub fn is_public_unicast_with(ip: IpAddr, dns64: Option<Ipv6Addr>) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6, dns64),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    if ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || ip.is_unspecified()
    {
        return false;
    }
    // 标准库没覆盖到的那些保留段。
    !(octets[0] == 0
        // 100.64.0.0/10：运营商级 NAT，常常就是内网。
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        // 192.0.0.0/24：IETF 协议分配。
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        // 198.18.0.0/15：基准测试。
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        // 192.88.99.0/24：6to4 中继任播（已弃用，别去）。
        || (octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
        // 240.0.0.0/4：保留，含 255.255.255.255。
        || octets[0] >= 240)
}

fn is_public_v6(ip: Ipv6Addr, dns64: Option<Ipv6Addr>) -> bool {
    // v4-mapped（`::ffff:0:0/96`）与 NAT64 的转换地址：「这个 IPv6 地址」的真实目标是一个
    // IPv4 地址，所以判据要落在那个 IPv4 上。不认这一条，`::ffff:127.0.0.1` 就绕过去了。
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    if let Some(v4) = embedded_v4_in_nat64(ip) {
        return is_public_v4(v4);
    }
    if let Some(prefix) = dns64 {
        if let Some(v4) = embedded_v4_under(ip, prefix) {
            return is_public_v4(v4);
        }
    }

    let octets = ip.octets();
    // 未指定、环回、唯一本地（fc00::/7）、链路本地（fe80::/10）、组播（ff00::/8）。
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return false;
    }
    if octets[0] & 0xfe == 0xfc {
        return false;
    }
    if octets[0] == 0xfe && octets[1] & 0xc0 == 0x80 {
        return false;
    }
    // 文档段 2001:db8::/32。
    if octets[..4] == [0x20, 0x01, 0x0d, 0xb8] {
        return false;
    }
    // Teredo（2001::/32）与 6to4（2002::/16）：都是隧道，入口地址里嵌着一个 IPv4，
    // 而那个 IPv4 完全不受我们的判据约束 —— 直接拒，不试着解它。
    if octets[..4] == [0x20, 0x01, 0x00, 0x00] || octets[..2] == [0x20, 0x02] {
        return false;
    }
    // 100::/64：discard-only。
    if octets[..8] == [0x01, 0x00, 0, 0, 0, 0, 0, 0] {
        return false;
    }
    true
}

/// 这个 IPv6 地址是不是「看起来像全球单播」—— 只有它才值得去探 DNS64 前缀。
///
/// 已知的特殊段（环回、v4-mapped、NAT64、6to4、文档段）都不算，因为它们不需要前缀就能判。
pub fn is_global_v6(ip: Ipv6Addr) -> bool {
    let octets = ip.octets();
    octets[0] & 0xe0 == 0x20
        && octets[..4] != [0x20, 0x01, 0x0d, 0xb8]
        && octets[..4] != [0x20, 0x01, 0x00, 0x00]
        && octets[..2] != [0x20, 0x02]
}

/// `64:ff9b::/96`（well-known）与 `64:ff9b:1::/48`（local-use）底下的内嵌 IPv4。
fn embedded_v4_in_nat64(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let octets = ip.octets();
    if octets[..12] == [0x00, 0x64, 0xff, 0x9b, 0, 0, 0, 0, 0, 0, 0, 0]
        || octets[..6] == [0x00, 0x64, 0xff, 0x9b, 0x00, 0x01]
    {
        return Some(Ipv4Addr::new(
            octets[12], octets[13], octets[14], octets[15],
        ));
    }
    None
}

/// 落在 `prefix` 的 `/96` 之下时，低 32 位里那个 IPv4。
fn embedded_v4_under(ip: Ipv6Addr, prefix: Ipv6Addr) -> Option<Ipv4Addr> {
    let (ip, prefix) = (ip.octets(), prefix.octets());
    (ip[..12] == prefix[..12]).then(|| Ipv4Addr::new(ip[12], ip[13], ip[14], ip[15]))
}

/// RFC 7050：查 `ipv4only.arpa`，从它回的 AAAA 地址里取出这个网络的 DNS64 前缀。
async fn probe_dns64() -> Option<Ipv6Addr> {
    let addrs = tokio::net::lookup_host(("ipv4only.arpa", 0)).await.ok()?;
    for addr in addrs {
        let IpAddr::V6(v6) = addr.ip() else {
            continue;
        };
        // well-known 的那两个 IPv4（192.0.0.170 / 192.0.0.171）就在低 32 位里；不是它们
        // 就说明这不是一个 DNS64 合成地址。
        let octets = v6.octets();
        let embedded = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
        if embedded == Ipv4Addr::new(192, 0, 0, 170) || embedded == Ipv4Addr::new(192, 0, 0, 171) {
            let mut prefix = octets;
            prefix[12..].fill(0);
            return Some(Ipv6Addr::from(prefix));
        }
    }
    None
}

/// 一次重定向要去哪。
///
/// 三条规矩：跳数有上限；**只走同源**（换源就要模型重新调用一次，于是「先跳到一个无关站点
/// 再跳到内网」这条链断在这里）；`Location` 必须能解析成绝对 URL。
///
/// 目的地址是内网这件事**不在这里**判 —— 判据在下一轮循环里，对新的主机名重新解析与校验。
/// 两者分开，是因为一个要网络、一个不要。
pub fn next_hop(current: &Url, location: &str, hops: usize) -> Result<Url, WebError> {
    if hops > MAX_REDIRECTS {
        return Err(WebError::new(
            WebErrorCode::BlockedUrl,
            format!("重定向超过 {MAX_REDIRECTS} 跳，停在这里"),
        ));
    }
    let next = current.join(location.trim()).map_err(|error| {
        WebError::new(
            WebErrorCode::InvalidUrl,
            format!("`{location}` 不是一个能解析的 `Location`：{error}"),
        )
    })?;
    if origin(&next) != origin(current) {
        return Err(WebError::new(
            WebErrorCode::BlockedUrl,
            format!(
                "`{current}` 想跨源跳到 `{next}`：跨源重定向一律拒，请直接抓你真正要的那个地址"
            ),
        ));
    }
    Ok(next)
}

/// scheme + host + port。跨源重定向就是这三样里任何一样变了。
fn origin(url: &Url) -> (String, String, Option<u16>) {
    (
        url.scheme().to_owned(),
        url.host_str().unwrap_or_default().to_ascii_lowercase(),
        url.port_or_known_default(),
    )
}

/// 读一个有界的响应体。
///
/// 到上限就停下并如实回报「溢出了」，而不是把剩下的读进内存再看 —— 上限的意义就是不让它进来。
pub async fn read_bounded(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<(Vec<u8>, bool), WebError> {
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        WebError::new(
            WebErrorCode::ProviderError,
            format!("读响应体时断了：{error}"),
        )
    })? {
        if body.len() + chunk.len() > limit {
            return Ok((body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, false))
}

/// 把字节变成能给模型看的正文。
///
/// * **类型**：`text/html` 与 `application/xhtml+xml` 是 HTML，`text/*` 与几个结构化文本类型
///   是纯文本，其余（图片、`application/octet-stream`、PDF……）直接拒。**没有 `Content-Type`
///   时按 HTML 处理** —— 网页是缺这个头最常见的一类，而 HTML 那条路会去掉标签，比把原始字节
///   当文本更安全。
/// * **字符集**：只认 `Content-Type` 里声明的那个（缺省 UTF-8）。声明了 UTF-8 之外的字符集时
///   **拒绝**：这条工具不做转码，而乱码进上下文比一条错误更坏 —— 模型会照着乱码作答。
/// * **字符上限**：解码之后按字符截断，并如实回报。
pub fn decode_body(
    url: &Url,
    content_type: Option<&str>,
    bytes: &[u8],
    max_chars: usize,
) -> Result<(FetchedContent, bool), WebError> {
    let declared = content_type.unwrap_or("text/html");
    let (media_type, charset) = split_content_type(declared);

    let kind = if media_type == "text/html" || media_type == "application/xhtml+xml" {
        Some(true)
    } else if media_type.starts_with("text/")
        || media_type == "application/json"
        || media_type == "application/xml"
        || media_type.ends_with("+xml")
        || media_type == "application/javascript"
        || media_type == "application/x-javascript"
    {
        Some(false)
    } else {
        None
    };
    let Some(is_html) = kind else {
        return Err(WebError::new(
            WebErrorCode::UnsupportedContent,
            format!("{url} 的类型是 `{media_type}`，这条工具只读 HTML 与纯文本"),
        ));
    };

    if let Some(charset) = charset {
        let normalized = charset.to_ascii_lowercase();
        let utf8_family = matches!(normalized.as_str(), "utf-8" | "utf8" | "us-ascii" | "ascii");
        if !utf8_family {
            return Err(WebError::new(
                WebErrorCode::UnsupportedContent,
                format!("{url} 声明了字符集 `{charset}`；这条工具只读 UTF-8，不做转码"),
            ));
        }
    }

    let text = String::from_utf8_lossy(bytes).into_owned();
    let truncated = text.chars().count() > max_chars;
    let text: String = if truncated {
        text.chars().take(max_chars).collect()
    } else {
        text
    };
    Ok((
        if is_html {
            FetchedContent::Html(text)
        } else {
            FetchedContent::Text(text)
        },
        truncated,
    ))
}

/// `text/html; charset=utf-8` → (`text/html`, Some("utf-8"))。
///
/// 只做这一点点解析：`mime` 那类 crate 带来一棵依赖树，而我们只关心两样东西。
fn split_content_type(value: &str) -> (String, Option<String>) {
    let mut parts = value.split(';');
    let media_type = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let mut charset = None;
    for parameter in parts {
        let Some((key, value)) = parameter.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("charset") {
            charset = Some(value.trim().trim_matches('"').to_owned());
        }
    }
    (media_type, charset)
}

fn transport_error(url: &Url, error: reqwest::Error) -> WebError {
    if error.is_timeout() {
        return timeout_error(url);
    }
    WebError::new(WebErrorCode::ProviderError, format!("{url}：{error}"))
}

fn timeout_error(url: &Url) -> WebError {
    WebError::new(WebErrorCode::FetchTimeout, format!("{url} 超时"))
}
