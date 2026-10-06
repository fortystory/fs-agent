//! `web_fetch` 的传输层：URL 校验、SSRF 判据、重定向与四道上限
//! （`.scratch/web-search-tool/spec.md` §5；票 03）。
//!
//! **零网络**：这一份里的每一次断言都落在纯函数上，或者落在一个 `IP` 字面量上（它不需要
//! DNS）；要造一个响应体的地方从字节现造一个，不起服务器、不发请求。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use heng::web::fetch_http::{
    decode_body, is_global_v6, is_public_unicast, next_hop, read_bounded, validate_url, HttpFetch,
    MAX_RESPONSE_BYTES, MAX_URL_CHARS,
};
use heng::web::{FetchProvider, FetchedContent};
use reqwest::header::{ACCEPT, AUTHORIZATION, COOKIE, PROXY_AUTHORIZATION, USER_AGENT};
use url::Url;

fn v4(text: &str) -> IpAddr {
    IpAddr::V4(text.parse::<Ipv4Addr>().unwrap())
}

fn v6(text: &str) -> IpAddr {
    IpAddr::V6(text.parse::<Ipv6Addr>().unwrap())
}

// --- URL 判据 -------------------------------------------------------------

#[test]
fn only_http_and_https_are_fetchable() {
    for bad in [
        "file:///etc/passwd",
        "ftp://example.com/x",
        "data:text/plain,hi",
    ] {
        let error = validate_url(bad).unwrap_err();
        assert_eq!(error.code_str(), "WEB_INVALID_URL", "{bad}");
    }

    assert!(validate_url("https://example.com/a?b=c").is_ok());
    assert!(validate_url("http://example.com").is_ok());
}

#[test]
fn a_url_with_embedded_credentials_is_refused() {
    let error = validate_url("https://user:pass@example.com/").unwrap_err();
    assert_eq!(error.code_str(), "WEB_INVALID_URL");
    assert!(error.message().contains("凭据"), "{error}");

    // 只有用户名也算。
    assert!(validate_url("https://user@example.com/").is_err());
}

#[test]
fn an_absurdly_long_url_is_refused() {
    let long = format!("https://example.com/{}", "a".repeat(MAX_URL_CHARS));
    let error = validate_url(&long).unwrap_err();
    assert_eq!(error.code_str(), "WEB_INVALID_URL");
    assert!(error.message().contains("字符"), "{error}");
}

// --- SSRF 判据 ------------------------------------------------------------

#[test]
fn private_loopback_and_link_local_addresses_are_refused() {
    for bad in [
        "127.0.0.1",
        "127.1.2.3",
        "0.0.0.0",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.0.1",
        "169.254.169.254", // 云元数据
        "100.64.0.1",      // CGNAT
        "192.0.0.1",
        "198.18.0.1",
        "224.0.0.1",
        "255.255.255.255",
        "192.0.2.1", // 文档段
    ] {
        assert!(!is_public_unicast(v4(bad)), "{bad} 不该被放行");
    }

    for good in ["1.1.1.1", "8.8.8.8", "93.184.216.34"] {
        assert!(is_public_unicast(v4(good)), "{good} 是公共单播");
    }
}

#[test]
fn ipv6_special_ranges_and_translated_addresses_are_refused() {
    for bad in [
        "::1",
        "::",
        "fe80::1",
        "fc00::1",
        "fd12:3456::1",
        "ff02::1",
        "2001:db8::1",
        "2001::1",      // Teredo
        "2002:0a00::1", // 6to4（里面嵌着 10.0.0.1）
        "100::1",
        "::ffff:127.0.0.1", // v4-mapped
        "64:ff9b::7f00:1",  // NAT64 → 127.0.0.1
        "64:ff9b::a00:1",   // NAT64 → 10.0.0.1
        "64:ff9b:1::a00:1", // local-use NAT64 → 10.0.0.1
    ] {
        assert!(!is_public_unicast(v6(bad)), "{bad} 不该被放行");
    }

    for good in ["2606:4700:4700::1111", "2001:4860:4860::8888"] {
        assert!(is_public_unicast(v6(good)), "{good} 是公共单播");
    }
    // 转换地址指向的是**公网** v4 时放行：判据落在内嵌的那个地址上。
    assert!(is_public_unicast(v6("64:ff9b::808:808")));
    assert!(is_public_unicast(v6("::ffff:8.8.8.8")));
}

#[test]
fn a_discovered_dns64_prefix_is_honoured_too() {
    // 运营商自建的前缀（RFC 7050 探出来的那种）：前缀之下的地址，判的是内嵌的那个 IPv4。
    let prefix: Ipv6Addr = "2001:db8:64::".parse().unwrap();
    assert!(!heng::web::fetch_http::is_public_unicast_with(
        v6("2001:db8:64::a00:1"),
        Some(prefix)
    ));
    assert!(heng::web::fetch_http::is_public_unicast_with(
        v6("2001:db8:64::808:808"),
        Some(prefix)
    ));
}

#[test]
fn only_globally_looking_ipv6_addresses_trigger_a_dns64_probe() {
    // 探 DNS64 前缀要发一次 DNS 查询，所以它只对「看起来像全球单播」的地址做 —— 环回、
    // v4-mapped 与已知的转换段都不需要它。
    assert!(is_global_v6("2606:4700::1".parse().unwrap()));
    assert!(!is_global_v6("::1".parse().unwrap()));
    assert!(!is_global_v6("64:ff9b::1".parse().unwrap()));
    assert!(!is_global_v6("2001:db8::1".parse().unwrap()));
    assert!(!is_global_v6("2002::1".parse().unwrap()));
}

#[tokio::test]
async fn an_ip_literal_pointing_inward_is_refused_before_any_connection() {
    // IP 字面量不需要 DNS，所以这条走的是真的解析路径，而且一个请求都不会发出去。
    let fetch = HttpFetch::new(100_000, Duration::from_secs(5));
    for bad in [
        "http://127.0.0.1/",
        "http://169.254.169.254/latest/meta-data/",
        "http://10.0.0.1/",
        "http://192.168.0.1/",
        "http://[::1]/",
        "http://[::ffff:127.0.0.1]/",
    ] {
        let url = validate_url(bad).unwrap();
        let error = fetch.resolve_public(&url).await.unwrap_err();
        assert_eq!(error.code_str(), "WEB_BLOCKED_URL", "{bad}：{error}");
    }
}

// --- 重定向 ---------------------------------------------------------------

#[test]
fn a_redirect_leaves_the_origin_only_when_it_is_the_same_one() {
    let current = Url::parse("https://example.com/a/b").unwrap();

    let same = next_hop(&current, "/c", 1).unwrap();
    assert_eq!(same.as_str(), "https://example.com/c");
    let relative = next_hop(&current, "d", 1).unwrap();
    assert_eq!(relative.as_str(), "https://example.com/a/d");

    // 换 scheme、换 host、换 port 都算跨源。
    for cross in [
        "http://example.com/c",
        "https://other.example.com/c",
        "https://example.com:8443/c",
    ] {
        let error = next_hop(&current, cross, 1).unwrap_err();
        assert_eq!(error.code_str(), "WEB_BLOCKED_URL", "{cross}");
        assert!(error.message().contains("跨源"), "{error}");
    }
}

#[test]
fn the_redirect_chain_has_a_ceiling() {
    let current = Url::parse("https://example.com/").unwrap();
    assert!(next_hop(&current, "/one-more", 5).is_ok());
    let error = next_hop(&current, "/one-more", 6).unwrap_err();
    assert_eq!(error.code_str(), "WEB_BLOCKED_URL");
    assert!(error.message().contains("跳"), "{error}");
}

#[test]
fn a_location_that_is_not_a_url_is_a_readable_error() {
    let current = Url::parse("https://example.com/").unwrap();
    let error = next_hop(&current, "http://[", 1).unwrap_err();
    assert_eq!(error.code_str(), "WEB_INVALID_URL");
}

// --- 有界读取 -------------------------------------------------------------

fn response_of(bytes: Vec<u8>) -> reqwest::Response {
    reqwest::Response::from(http::Response::new(reqwest::Body::from(bytes)))
}

#[tokio::test]
async fn a_body_over_the_byte_ceiling_is_refused_rather_than_read() {
    let fetch = HttpFetch::new(100_000, Duration::from_secs(5));
    assert_eq!(fetch.name(), "http");

    let big = vec![b'a'; 1_024];
    let (body, overflowed) = read_bounded(response_of(big), 512).await.unwrap();
    assert!(overflowed, "到上限就停下并如实回报");
    assert!(body.len() <= 512);
    assert_eq!(MAX_RESPONSE_BYTES, 5 * 1024 * 1024);
}

#[tokio::test]
async fn a_small_body_comes_through_whole() {
    let (body, overflowed) = read_bounded(response_of(b"hello".to_vec()), 512)
        .await
        .unwrap();
    assert!(!overflowed);
    assert_eq!(body, b"hello");
}

// --- 类型与字符集 ---------------------------------------------------------

#[test]
fn html_and_plain_text_are_the_two_readable_kinds() {
    let url = Url::parse("https://example.com/").unwrap();

    let (content, truncated) =
        decode_body(&url, Some("text/html; charset=utf-8"), b"<p>hi</p>", 100).unwrap();
    assert!(matches!(content, FetchedContent::Html(_)));
    assert!(!truncated);

    let (content, _) = decode_body(&url, Some("text/plain"), b"hi", 100).unwrap();
    assert!(matches!(content, FetchedContent::Text(_)));

    let (content, _) = decode_body(&url, Some("application/json"), b"{}", 100).unwrap();
    assert!(matches!(content, FetchedContent::Text(_)));
}

#[test]
fn a_missing_content_type_is_treated_as_html() {
    // 缺这个头的多半是网页，而 HTML 那条路会去掉标签 —— 比把原始字节当文本更安全。
    let url = Url::parse("https://example.com/").unwrap();
    let (content, _) = decode_body(&url, None, b"<p>hi</p>", 100).unwrap();
    assert!(matches!(content, FetchedContent::Html(_)));
}

#[test]
fn binary_and_unknown_types_are_refused() {
    let url = Url::parse("https://example.com/blob").unwrap();
    for kind in ["application/octet-stream", "image/png", "application/pdf"] {
        let error = decode_body(&url, Some(kind), b"\x00\x01", 100).unwrap_err();
        assert_eq!(error.code_str(), "WEB_UNSUPPORTED_CONTENT", "{kind}");
    }
}

#[test]
fn a_charset_this_tool_cannot_read_is_refused_rather_than_mangled() {
    let url = Url::parse("https://example.com/").unwrap();
    for charset in ["gbk", "big5", "Shift_JIS"] {
        let header = format!("text/html; charset={charset}");
        let error = decode_body(&url, Some(&header), b"hi", 100).unwrap_err();
        assert_eq!(error.code_str(), "WEB_UNSUPPORTED_CONTENT", "{charset}");
        assert!(error.message().contains(charset), "{error}");
    }

    // UTF-8 家族与 `us-ascii` 照收，引号也认。
    for charset in ["utf-8", "UTF-8", "utf8", "us-ascii", "ascii"] {
        let header = format!("text/html; charset=\"{charset}\"");
        assert!(
            decode_body(&url, Some(&header), b"hi", 100).is_ok(),
            "{charset}"
        );
    }
}

#[test]
fn a_long_body_is_truncated_and_says_so() {
    let url = Url::parse("https://example.com/").unwrap();
    let (content, truncated) = decode_body(&url, Some("text/plain"), b"0123456789", 4).unwrap();
    assert!(truncated);
    match content {
        FetchedContent::Text(text) => assert_eq!(text, "0123"),
        other => panic!("要的是纯文本，得到 {other:?}"),
    }
}

// --- 不发送凭据 -----------------------------------------------------------

#[test]
fn the_request_carries_an_honest_identity_and_no_credentials() {
    let fetch = HttpFetch::new(100_000, Duration::from_secs(30));
    let client = reqwest::Client::new();
    let url = Url::parse("https://example.com/").unwrap();
    let request = fetch.build_request(&client, &url).unwrap();

    let headers = request.headers();
    assert!(
        headers[USER_AGENT].to_str().unwrap().starts_with("heng/"),
        "诚实的身份：{:?}",
        headers[USER_AGENT]
    );
    assert!(headers.contains_key(ACCEPT));
    for forbidden in [AUTHORIZATION, COOKIE, PROXY_AUTHORIZATION] {
        assert!(
            !headers.contains_key(&forbidden),
            "抓取是匿名的，不该带 {forbidden}"
        );
    }
}

// --- 代理接管 DNS（`[web] trust_proxy_dns`）--------------------------------

#[tokio::test]
async fn trust_proxy_dns_skips_resolution_for_hostnames() {
    // fake-IP 的机器上，本地解析回的是 `198.18.0.0/15` 那类假地址：校验它没有意义，连接也不该
    // 固定到它。代理路径因此**不解析**，直接答「这一次不固定」。
    let fetch = HttpFetch::new(100_000, Duration::from_secs(30)).with_trust_proxy_dns(true);
    let addrs = fetch
        .resolve_public(&validate_url("https://example.com/").unwrap())
        .await
        .expect("代理路径上主机名不再被校验");
    assert!(addrs.is_empty(), "代理路径不该把连接固定到任何地址");
}

#[tokio::test]
async fn trust_proxy_dns_still_refuses_literal_internal_addresses() {
    // 字面 IP 不需要 DNS，所以那条判据在两条路径上都成立 —— 代理**不会**替你拦内网
    // （实测：经代理访问局域网设备返回 200，所以这条不能省）。
    let fetch = HttpFetch::new(100_000, Duration::from_secs(30)).with_trust_proxy_dns(true);
    for bad in [
        "http://192.168.3.1/",
        "http://10.0.0.1/",
        "http://127.0.0.1/",
        "http://169.254.169.254/",
        "http://[::1]/",
    ] {
        let error = fetch
            .resolve_public(&validate_url(bad).unwrap())
            .await
            .unwrap_err();
        assert_eq!(error.code_str(), "WEB_BLOCKED_URL", "{bad}");
    }
}
