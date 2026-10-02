//! 测试用的假 MCP server（`.scratch/mcp-support/spec.md` §3；票 12）。
//!
//! 它是**手写**的 newline-delimited JSON-RPC 对端，刻意不依赖 `rmcp` 的 server 侧 —— 测试要
//! 覆盖的正是「真 spawn + 沙箱包装 + 协议帧 + 进程组清理」，拿 SDK 自己的 server 去测自己的
//! client 会让两头一起漂。它只实现五条方法：`server/discover`（2026-07-28 的无状态握手）、
//! `tools/list`、`tools/call`、`resources/list`、`resources/read`。
//!
//! 行为由环境变量控制：
//!
//! - `FAKE_MCP_LEGACY=1`：对 `server/discover` 回一条 JSON-RPC error（只认旧版 `initialize`），
//!   用来断言 client 是**明确拒绝**、不是静默降级。
//! - `FAKE_MCP_INSTRUCTIONS=<text>`：握手时带一句自述。
//! - `FAKE_MCP_SELF_HEARTBEAT=<path>`：起一条线程，每 200 毫秒把 `alive` 写进这个文件。
//! - `FAKE_MCP_CHILD_HEARTBEAT=<path>`：spawn 一个 `sh` 循环，同样每 200 毫秒写一次。
//!
//! 后两条给「会话结束时整组被收掉」那条用例：**不看 pid**（这个沙箱里 `/proc` 的语义不可靠），
//! 只看这两个文件的 mtime 还有没有在前进 —— 停了就是那一组真没了。
//!
//! 它不是产品的一部分，只是一个测试辅助程序（`Cargo.toml` 里的 `[[bin]]` 有注）。

use std::io::{BufRead, Write};
use std::time::Duration;

use serde_json::{json, Value};

fn main() {
    // 一行 stderr：测试据此断言「server 的诊断被接住了」，而不是直接砸进终端。
    eprintln!("fake-mcp-server: 起来了");
    start_heartbeats();

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
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let response = handle(method, &id, &request);
        if writeln!(stdout, "{response}").is_err() {
            break;
        }
        if stdout.flush().is_err() {
            break;
        }
    }
}

/// 两路心跳：一路是 server 自己，一路是它 spawn 的长跑子进程 —— 后者才验得到「整组」。
fn start_heartbeats() {
    if let Ok(path) = std::env::var("FAKE_MCP_SELF_HEARTBEAT") {
        std::thread::spawn(move || loop {
            let _ = std::fs::write(&path, b"alive");
            std::thread::sleep(Duration::from_millis(200));
        });
    }
    if let Ok(path) = std::env::var("FAKE_MCP_CHILD_HEARTBEAT") {
        // `/bin/sh` 走绝对路径：假 server 以后会被跑在环境白名单之下，那时 PATH 不保证有它。
        let _ = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(
                "while true; do printf alive > '{}'; sleep 0.2; done",
                path
            ))
            .spawn();
    }
}

fn handle(method: &str, id: &Value, request: &Value) -> Value {
    match method {
        "server/discover" => {
            if std::env::var("FAKE_MCP_LEGACY").is_ok() {
                return error(id, -32601, "Method not found");
            }
            record_discover(request);
            let mut result = json!({
                "resultType": "complete",
                "supportedVersions": ["2026-07-28"],
                "capabilities": { "tools": {}, "resources": {} },
                "ttlMs": 0,
                "cacheScope": "private",
            });
            if let Ok(instructions) = std::env::var("FAKE_MCP_INSTRUCTIONS") {
                result["instructions"] = json!(instructions);
            }
            ok(id, result)
        }
        "tools/list" => ok(
            id,
            json!({
                "resultType": "complete",
                "tools": [
                    {
                        "name": "echo",
                        "description": "把 text 原样回给你",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "text": { "type": "string", "description": "要回显的东西" }
                            },
                            "required": ["text"]
                        }
                    },
                    {
                        "name": "env",
                        "description": "报一个环境变量的值（没有就报「(没有)」）",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "name": { "type": "string" } },
                            "required": ["name"]
                        }
                    },
                    {
                        "name": "write",
                        "description": "往一个路径写一段文本",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "text": { "type": "string" }
                            },
                            "required": ["path", "text"]
                        }
                    },
                    {
                        "name": "ask",
                        "description": "要走一次 MRTR：先跟人要一个输入，再回结果",
                        "inputSchema": {
                            "type": "object",
                            "properties": {},
                            "required": []
                        }
                    },
                    {
                        "name": "ancestors",
                        "description": "报出自己往上那一串父进程的名字（诊断用）",
                        "inputSchema": {
                            "type": "object",
                            "properties": {},
                            "required": []
                        }
                    }
                ]
            }),
        ),
        "tools/call" => {
            let name = request
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let arguments = request.pointer("/params/arguments");
            let argument = |key: &str| {
                arguments
                    .and_then(|args| args.get(key))
                    .and_then(Value::as_str)
                    .unwrap_or("")
            };
            match name {
                "echo" => text_result(id, &format!("echo: {}", argument("text"))),
                "env" => {
                    let key = argument("name");
                    let value = std::env::var(key).unwrap_or_else(|_| "(没有)".to_owned());
                    text_result(id, &format!("{key}={value}"))
                }
                "write" => {
                    let path = argument("path");
                    match std::fs::write(path, argument("text")) {
                        Ok(()) => text_result(id, &format!("written {path}")),
                        Err(error) => failed_result(id, &format!("写不了 {path}：{error}")),
                    }
                }
                "ask" => {
                    // MRTR：第一次回 `input_required`，第二次（client 带着 `inputResponses`
                    // 重试）才给最终结果。`requestState` 原样回显，便于断言回路真的走了。
                    let state = request
                        .pointer("/params/requestState")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    match request.pointer("/params/inputResponses") {
                        None => input_required(id),
                        Some(responses) => {
                            let action = responses
                                .pointer("/q1/action")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            if action != "accept" {
                                return failed_result(id, "用户没有给这个输入");
                            }
                            let answer = responses
                                .pointer("/q1/content/answer")
                                .and_then(Value::as_str)
                                .unwrap_or("(没有)");
                            text_result(id, &format!("补好了：{answer}（state={state}）"))
                        }
                    }
                }
                "ancestors" => text_result(id, &ancestor_names().join(">")),
                _ => error(id, -32601, "Unknown tool"),
            }
        }
        // 提示词模板：给 `/` 菜单那一半用（票 17）。两个模板 —— 一个带必填参数、一个不带，
        // 好让人在真终端里两条路都走一遍。
        "prompts/list" => ok(
            id,
            json!({
                "resultType": "complete",
                "prompts": [
                    {
                        "name": "user_report",
                        "description": "按 id 出一份报告",
                        "arguments": [
                            { "name": "id", "description": "用户 id", "required": true }
                        ]
                    },
                    {
                        "name": "standup",
                        "description": "把今天做的事写成三条"
                    }
                ]
            }),
        ),
        "prompts/get" => {
            let name = request
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let user_id = request
                .pointer("/params/arguments/id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let text = match name {
                "user_report" => format!("请给用户 {user_id} 出一份报告"),
                "standup" => "把今天做的事写成三条".to_owned(),
                _ => return error(id, -32602, "Unknown prompt"),
            };
            ok(
                id,
                json!({
                    "resultType": "complete",
                    "description": "假 server 的模板",
                    "messages": [
                        { "role": "user", "content": { "type": "text", "text": text } }
                    ]
                }),
            )
        }
        "resources/list" => ok(
            id,
            json!({
                "resultType": "complete",
                "resources": [{
                    "uri": "db://users/42",
                    "name": "user 42",
                    "description": "一份外部数据",
                    "mimeType": "text/plain"
                }]
            }),
        ),
        "resources/read" => {
            let uri = request
                .pointer("/params/uri")
                .and_then(Value::as_str)
                .unwrap_or("");
            if uri != "db://users/42" {
                return error(id, -32002, "Resource not found");
            }
            ok(
                id,
                json!({
                    "resultType": "complete",
                    "contents": [{
                        "uri": uri,
                        "mimeType": "text/plain",
                        "text": "{\"id\":42}"
                    }]
                }),
            )
        }
        _ => error(id, -32601, "Method not found"),
    }
}

fn ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// 把整条 `Discover` 请求落盘，好让测试断言 client 在握手时报了哪些能力。
///
/// 写整条而不是只挑 `clientCapabilities`：`_meta` 的具体位置是协议实现细节，测试关心的是
/// 「elicitation 这个名字出现在握手里」。
fn record_discover(request: &Value) {
    let Ok(path) = std::env::var("FAKE_MCP_CAPABILITIES_FILE") else {
        return;
    };
    let _ = std::fs::write(path, request.to_string());
}

/// 一条 MRTR 中间结果：要 client 补一个 form 输入之后再重试。
fn input_required(id: &Value) -> Value {
    ok(
        id,
        json!({
            "resultType": "input_required",
            "inputRequests": {
                "q1": {
                    "method": "elicitation/create",
                    "params": {
                        "mode": "form",
                        "message": "这一笔需要你补一个值",
                        "requestedSchema": {
                            "type": "object",
                            "properties": {
                                "answer": { "type": "string", "title": "答案" }
                            },
                            "required": ["answer"]
                        }
                    }
                }
            },
            "requestState": "state-1"
        }),
    )
}

/// 自己往上那一串父进程的名字（最多十层）。
///
/// 测试用它来回答「这个 server 到底过没过 bwrap」：过了的话链子里有 `bwrap`，没过就没有。
/// 不靠 pid（这个沙箱里 pid 命名空间每次调用一套），也就不用猜。
fn ancestor_names() -> Vec<String> {
    let mut names = Vec::new();
    let mut pid = std::process::id();
    for _ in 0..10 {
        let Some(ppid) = parent_of(pid) else { break };
        if ppid == 0 {
            break;
        }
        names.push(comm_of(ppid).unwrap_or_else(|| ppid.to_string()));
        pid = ppid;
    }
    names
}

fn parent_of(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("PPid:")?.trim().parse::<u32>().ok())
}

fn comm_of(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|name| name.trim().to_owned())
}

/// 一次成功的工具结果：一个 text block。
fn text_result(id: &Value, text: &str) -> Value {
    ok(
        id,
        json!({
            "resultType": "complete",
            "content": [{ "type": "text", "text": text }]
        }),
    )
}

/// 一次**工具级**失败：`isError: true`，协议上仍是一条正常响应。
fn failed_result(id: &Value, text: &str) -> Value {
    ok(
        id,
        json!({
            "resultType": "complete",
            "content": [{ "type": "text", "text": text }],
            "isError": true
        }),
    )
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
