//! `fs-agent-mcp-time`：仓库自带的时间 server（`.scratch/time-mcp/spec.md` §1–§3）。
//!
//! 它是一个**手写**的 stdio MCP server，与 `tests/support/fake_mcp_server.rs` 同一形状（那个是
//! 测试辅助，这个进产品）。只答三条方法 —— `server/discover`、`tools/list`、`tools/call` —— 只
//! 提供一个工具 `get_current_time`。不写盘、不出网、不读配置、不看环境：整个进程除了一次时钟
//! 读数与标准输出，什么也不做，所以它能过缺省的沙箱。
//!
//! **为什么手写、不开 `rmcp` 的 server 侧**：`Cargo.toml` 那行依赖注写着只开 client，而这里要的
//! 只是三条方法、约四十行；开了它，产品就多背一整套服务端实现。协议只谈 `2026-07-28`
//! （`Discover` 生命周期），与 client 侧同一条纪律：不做旧版 `initialize` 回退。

use std::io::{BufRead, Write};

use chrono::{DateTime, Datelike, Local, TimeZone, Weekday};
use serde_json::{json, Value};

/// 只认这一个版本，与 client 侧固定 `ClientLifecycleMode::Discover` 是同一条纪律。
const PROTOCOL_VERSION: &str = "2026-07-28";

/// 这台 server 唯一那个工具的名字。它是契约的一部分：身份里那句指引按名字点它
/// （`src/agent.rs` 的 `TIME_GUIDANCE`）。
const TOOL_NAME: &str = "get_current_time";

fn main() {
    // 人看的话走 stderr（client 把它接住、交给诊断口 `[外部工具] …`），stdout 只出现协议帧。
    eprintln!("fs-agent-mcp-time: 起来了");

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // 通知（没有 `id` 的帧）按 JSON-RPC 不回响应：`id` 缺席才是通知，写 `null` 不是。
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let response = handle(method, &id, &request);
        if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
            break;
        }
    }
}

/// 一条请求 → 一条响应。不认识的工具与方法一律 `-32601`（与假 server 同一档语义）。
fn handle(method: &str, id: &Value, request: &Value) -> Value {
    match method {
        "server/discover" => ok(
            id,
            json!({
                "resultType": "complete",
                "supportedVersions": [PROTOCOL_VERSION],
                "capabilities": { "tools": {} },
                "ttlMs": 0,
                "cacheScope": "private",
            }),
        ),
        "tools/list" => ok(
            id,
            json!({
                "resultType": "complete",
                "tools": [{
                    "name": TOOL_NAME,
                    "description": "报出这台机器当下的本地时间：年月日、时分秒、UTC 偏移、时区名与星期几。",
                    "inputSchema": { "type": "object", "properties": {}, "required": [] }
                }]
            }),
        ),
        "tools/call" => {
            let name = request
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            if name == TOOL_NAME {
                let zone = iana_time_zone::get_timezone().ok();
                text_result(id, &time_line(Local::now(), zone.as_deref()))
            } else {
                error(id, -32601, "Unknown tool")
            }
        }
        _ => error(id, -32601, "Method not found"),
    }
}

/// 模型读到的那一行。
///
/// 时区名缺席时**省掉尾部的括号** —— 编一个名字、或者让这次调用失败，都比这更糟：时间本身还是
/// 对的。签名对时区泛化，是为了让测试能拿固定偏移构造时刻，不必依赖跑测试那台机器的时区。
fn time_line<Tz: TimeZone>(now: DateTime<Tz>, zone: Option<&str>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let stamp = now.format("%Y-%m-%d %H:%M:%S %:z");
    let weekday = weekday_in_chinese(now.weekday());
    match zone {
        Some(zone) => format!("本机现在：{stamp} {weekday}（{zone}）"),
        None => format!("本机现在：{stamp} {weekday}"),
    }
}

/// 星期几用中文写：`%A` 出来的是英文，而模型可见的散文走中文（ADR 0005）。
fn weekday_in_chinese(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "星期一",
        Weekday::Tue => "星期二",
        Weekday::Wed => "星期三",
        Weekday::Thu => "星期四",
        Weekday::Fri => "星期五",
        Weekday::Sat => "星期六",
        Weekday::Sun => "星期日",
    }
}

/// 一次成功的工具结果：一个 text block —— 与假 server 逐字同形。
fn text_result(id: &Value, text: &str) -> Value {
    ok(
        id,
        json!({
            "resultType": "complete",
            "content": [{ "type": "text", "text": text }]
        }),
    )
}

fn ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// `time_line` 也被协议测试间接盖到，但「时区名取不到」那条路在真机上构造不出来
/// （`iana-time-zone` 几乎总能拿到名字），所以在这里直接断言那两种形状。
#[cfg(test)]
mod tests {
    use super::*;

    /// 一个固定时刻：2026-10-06 14:32:05 星期二，东八区。
    fn fixed() -> DateTime<chrono::FixedOffset> {
        chrono::FixedOffset::east_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 6, 14, 32, 5)
            .unwrap()
    }

    #[test]
    fn the_line_carries_the_zone_when_it_is_known() {
        let line = time_line(fixed(), Some("Asia/Shanghai"));
        assert_eq!(
            line,
            "本机现在：2026-10-06 14:32:05 +08:00 星期二（Asia/Shanghai）"
        );
    }

    #[test]
    fn the_line_drops_the_parentheses_when_the_zone_is_unknown() {
        let line = time_line(fixed(), None);
        assert_eq!(line, "本机现在：2026-10-06 14:32:05 +08:00 星期二");
    }
}
