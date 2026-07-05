//! token-slim-mcp: MCP server (JSON-RPC 2.0 over stdio, newline-delimited)
//! that exposes token-slimming tools usable from any MCP client.

mod slim;
mod tokens;
mod tools;

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                write_msg(
                    &mut out,
                    &json!({"jsonrpc":"2.0","id":null,
                            "error":{"code":-32700,"message":format!("parse error: {e}")}}),
                );
                continue;
            }
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let is_notification = id.is_none() || id == Some(Value::Null);

        // Notifications and client-side responses need no reply.
        if method.starts_with("notifications/") || method.is_empty() {
            continue;
        }

        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let result: Result<Value, (i64, String)> = match method {
            "initialize" => Ok(initialize_result(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools::tool_definitions()})),
            "tools/call" => tools::call(&params),
            _ => Err((-32601, format!("method not found: {method}"))),
        };

        if is_notification {
            continue;
        }
        let resp = match result {
            Ok(r) => json!({"jsonrpc":"2.0","id":id,"result":r}),
            Err((code, message)) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
            }
        };
        write_msg(&mut out, &resp);
    }
}

fn write_msg(out: &mut impl Write, v: &Value) {
    if let Ok(s) = serde_json::to_string(v) {
        let _ = out.write_all(s.as_bytes());
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
}

fn initialize_result(params: &Value) -> Value {
    let pv = params
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("2025-06-18");
    json!({
        "protocolVersion": pv,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "token-slim-mcp",
            "title": "Token Slim MCP",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Token-saving tools. Prefer these over raw file reads/searches: \
read_slim (file read, comments/blanks stripped, capped), grep_slim (compact search), \
dir_map (compact tree), json_slim (prune/minify JSON), text_slim (compress text), \
token_count (estimate tokens)."
    })
}
