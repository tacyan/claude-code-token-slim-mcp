//! End-to-end test: spawn the real binary and speak MCP over stdio.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn handshake_list_and_call() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_token-slim-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn server");
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    let mut send = |s: &str| {
        stdin.write_all(s.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    };
    let mut recv = || {
        let mut l = String::new();
        reader.read_line(&mut l).unwrap();
        serde_json::from_str::<serde_json::Value>(&l).expect("valid json response")
    };

    // initialize
    send(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#);
    let r = recv();
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["serverInfo"]["name"], "token-slim-mcp");
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");

    // initialized notification (must produce no response)
    send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    // tools/list
    send(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let r = recv();
    assert_eq!(r["id"], 2);
    let tools = r["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 6);
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in ["read_slim", "grep_slim", "dir_map", "json_slim", "text_slim", "token_count"] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }

    // tools/call text_slim
    send(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"text_slim","arguments":{"text":"a   \n\n\n\nb\n"}}}"#);
    let r = recv();
    assert_eq!(r["id"], 3);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("[token-slim]"), "got: {text}");
    assert!(text.contains("a\n\nb"), "got: {text}");

    // tools/call json_slim
    send(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"json_slim","arguments":{"json":"{\"a\":[1,2,3,4,5],\"b\":\"hello\"}","max_array":2}}}"#);
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("…+3 more items"), "got: {text}");

    // tools/call token_count
    send(r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"token_count","arguments":{"text":"hello world, this is a test"}}}"#);
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("tokens"), "got: {text}");

    // unknown tool -> JSON-RPC error
    send(r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#);
    let r = recv();
    assert_eq!(r["error"]["code"], -32602);

    // missing file -> tool error result (isError), not protocol error
    send(r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"read_slim","arguments":{"path":"/no/such/file.xyz"}}}"#);
    let r = recv();
    assert_eq!(r["result"]["isError"], true);

    // ping
    send(r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#);
    let r = recv();
    assert_eq!(r["id"], 8);

    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}
