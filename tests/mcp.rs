//! End-to-end test: spawn the real binary and speak MCP over stdio.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn tmp_dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn call(name: &str, args: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0", "id": 99, "method": "tools/call",
        "params": {"name": name, "arguments": args}
    })
    .to_string()
}

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
    send(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
    );
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
    assert_eq!(tools.len(), 7);
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "read_slim",
        "grep_slim",
        "refs_slim",
        "dir_map",
        "json_slim",
        "text_slim",
        "token_count",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }

    // tools/call text_slim
    send(
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"text_slim","arguments":{"text":"a   \n\n\n\nb\n"}}}"#,
    );
    let r = recv();
    assert_eq!(r["id"], 3);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("[token-slim]"), "got: {text}");
    assert!(text.contains("a\n\nb"), "got: {text}");

    // tools/call json_slim
    send(
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"json_slim","arguments":{"json":"{\"a\":[1,2,3,4,5],\"b\":\"hello\"}","max_array":2}}}"#,
    );
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("…+3 more items"), "got: {text}");

    // tools/call token_count
    send(
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"token_count","arguments":{"text":"hello world, this is a test"}}}"#,
    );
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("tokens"), "got: {text}");

    // unknown tool -> JSON-RPC error
    send(
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#,
    );
    let r = recv();
    assert_eq!(r["error"]["code"], -32602);

    // missing file -> tool error result (isError), not protocol error
    send(
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"read_slim","arguments":{"path":"/no/such/file.xyz"}}}"#,
    );
    let r = recv();
    assert_eq!(r["result"]["isError"], true);

    // ping
    send(r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#);
    let r = recv();
    assert_eq!(r["id"], 8);

    // json_slim on a JSONC file (bun.lock / tsconfig.json style)
    let dir = tmp_dir("jsonc");
    let lock = dir.join("bun.lock");
    std::fs::write(
        &lock,
        "{\n  // lockfile\n  \"lockfileVersion\": 1,\n  \"packages\": {\n    \"a\": [\"a@1.0.0\", {}, \"sha\"], /* inline */\n  },\n}\n",
    )
    .unwrap();
    send(&call(
        "json_slim",
        serde_json::json!({"path": lock.to_str().unwrap()}),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(r["result"]["isError"].is_null(), "jsonc must parse: {text}");
    assert!(text.contains("jsonc"), "got: {text}");
    assert!(text.contains("lockfileVersion"), "got: {text}");

    // genuinely broken JSON still reports an error
    send(&call("json_slim", serde_json::json!({"json": "{\"a\": }"})));
    let r = recv();
    assert_eq!(r["result"]["isError"], true);

    // grep_slim exclude globs
    let dir = tmp_dir("grep");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("test")).unwrap();
    std::fs::write(dir.join("src/lib.rs"), "fn computeFrameComp() {}\n").unwrap();
    std::fs::write(
        dir.join("test/lib_test.rs"),
        "computeFrameComp();\ncomputeFrameComp();\n",
    )
    .unwrap();
    let root = dir.to_str().unwrap();

    send(&call(
        "grep_slim",
        serde_json::json!({"pattern": "computeFrameComp", "path": root}),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("3 matches"), "got: {text}");

    send(&call(
        "grep_slim",
        serde_json::json!({
            "pattern": "computeFrameComp", "path": root, "exclude": ["**/test/**"]
        }),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("1 matches"), "got: {text}");
    assert!(!text.contains("lib_test.rs"), "got: {text}");

    // exclude_tests shorthand reaches the same result
    send(&call(
        "grep_slim",
        serde_json::json!({
            "pattern": "computeFrameComp", "path": root, "exclude_tests": true
        }),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("1 matches"), "got: {text}");

    // include filter
    send(&call(
        "grep_slim",
        serde_json::json!({
            "pattern": "computeFrameComp", "path": root, "include": ["src/**"]
        }),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("1 matches"), "got: {text}");

    // read_slim default mode: outline for a large code file, slim for a small one
    let dir = tmp_dir("read");
    let big = dir.join("big.rs");
    let body: String = (0..40)
        .map(|i| {
            let mut f = format!("pub fn f{i}(x: u32) -> u32 {{\n");
            for k in 0..12 {
                f.push_str(&format!("    let v{k} = x + {i} + {k} * 3;\n"));
            }
            f.push_str("    y * 2\n}\n");
            f
        })
        .collect();
    std::fs::write(&big, &body).unwrap();
    send(&call(
        "read_slim",
        serde_json::json!({"path": big.to_str().unwrap()}),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("mode=outline(auto)"), "got: {text}");
    assert!(
        text.contains("offset/limit"),
        "outline must say how to drill down"
    );
    assert!(text.contains("L1: pub fn f0"), "got: {text}");
    assert!(!text.contains("y * 2"), "outline must not carry bodies");

    // offset/limit always reads the range, never an outline
    send(&call(
        "read_slim",
        serde_json::json!({
            "path": big.to_str().unwrap(), "offset": 1, "limit": 15
        }),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("mode=slim(auto)"), "got: {text}");
    assert!(text.contains("y * 2"), "got: {text}");

    let small = dir.join("small.rs");
    std::fs::write(&small, "// c\npub fn a() -> u8 { 1 }\n").unwrap();
    send(&call(
        "read_slim",
        serde_json::json!({"path": small.to_str().unwrap()}),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("mode=slim(auto)"), "got: {text}");
    assert!(text.contains("pub fn a() -> u8 { 1 }"), "got: {text}");

    // a body that would be snipped mid-file yields the outline instead
    send(&call(
        "read_slim",
        serde_json::json!({
            "path": big.to_str().unwrap(), "max_tokens": 200
        }),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("mode=outline(auto)"), "got: {text}");

    // explicit mode still wins
    send(&call(
        "read_slim",
        serde_json::json!({
            "path": big.to_str().unwrap(), "mode": "slim"
        }),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("mode=slim "), "got: {text}");
    assert!(text.contains("y * 2"), "got: {text}");

    // refs_slim: the classification is the whole point — a plain grep over
    // this fixture returns 6 lines, only 2 of which answer "who calls it".
    let dir = tmp_dir("refs");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("test")).unwrap();
    std::fs::write(
        dir.join("src/core.ts"),
        "export function widen(x: number): number {\n  return x * 2\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/app.ts"),
        concat!(
            "import { widen } from './core'\n",
            "// widen is shared\n",
            "export class App {\n",
            "  private scale(\n",
            "    n: number,\n",
            "  ): number {\n",
            "    return widen(n)\n",
            "  }\n",
            "  run(): number {\n",
            "    return this.scale(2)\n",
            "  }\n",
            "}\n",
        ),
    )
    .unwrap();
    std::fs::write(dir.join("test/core_test.ts"), "widen(1)\nwiden(2)\n").unwrap();
    let root = dir.to_str().unwrap();

    send(&call(
        "refs_slim",
        serde_json::json!({"symbol": "widen", "path": root}),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(r["result"]["isError"].is_null(), "got: {text}");
    assert!(text.contains("definition 1"), "got: {text}");
    assert!(text.contains("call 1"), "got: {text}");
    assert!(text.contains("test-call 2"), "got: {text}");
    assert!(text.contains("import 1"), "got: {text}");
    assert!(text.contains("comment 1"), "got: {text}");
    // The call site is attributed to the method that contains it, even though
    // that method's argument list wraps across three lines.
    assert!(text.contains("in scale"), "got: {text}");
    // Test call sites are counted but not listed unless asked for.
    assert!(!text.contains("core_test.ts"), "got: {text}");

    send(&call(
        "refs_slim",
        serde_json::json!({"symbol": "widen", "path": root, "include_tests": true}),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("core_test.ts"), "got: {text}");

    // depth=2 walks from the call site's enclosing method to its own callers.
    send(&call(
        "refs_slim",
        serde_json::json!({"symbol": "widen", "path": root, "depth": 2}),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("depth=2"), "got: {text}");
    assert!(text.contains("hop2 run -> scale"), "got: {text}");

    // An unknown symbol reports cleanly instead of erroring.
    send(&call(
        "refs_slim",
        serde_json::json!({"symbol": "nowhere", "path": root}),
    ));
    let r = recv();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(r["result"]["isError"].is_null(), "got: {text}");
    assert!(text.contains("no references"), "got: {text}");

    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}
