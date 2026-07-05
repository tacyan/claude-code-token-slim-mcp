#!/bin/sh
# ts.sh — call a token-slim MCP tool directly (no MCP connection needed).
# Usage: sh ~/.claude/skills/slim/ts.sh <tool> ['<json-arguments>']
#   e.g. sh ~/.claude/skills/slim/ts.sh read_slim '{"path":"src/main.rs"}'
#        sh ~/.claude/skills/slim/ts.sh dir_map
# Tools: read_slim grep_slim dir_map json_slim text_slim token_count
# Binary discovery: $TOKEN_SLIM_BIN -> known build path -> PATH -> ~/.cargo/bin
set -eu

BIN="${TOKEN_SLIM_BIN:-}"
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
  BIN="$HOME/dev/claude-code-token-slim-mcp/target/release/token-slim-mcp"
fi
if [ ! -x "$BIN" ]; then
  BIN="$(command -v token-slim-mcp 2>/dev/null || true)"
fi
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
  BIN="$HOME/.cargo/bin/token-slim-mcp"
fi
if [ ! -x "$BIN" ]; then
  echo "token-slim-mcp binary not found. Set TOKEN_SLIM_BIN, or install it:" >&2
  echo "  git clone https://github.com/tacyan/token-slim-mcp && cd token-slim-mcp && cargo install --path ." >&2
  exit 127
fi

TOOL="${1:?usage: ts.sh <tool> ['<json-arguments>'] | ts.sh --which}"
if [ "$TOOL" = "--which" ]; then
  printf '%s\n' "$BIN"
  exit 0
fi
ARGS="${2-}"
[ -n "$ARGS" ] || ARGS='{}'

OUT=$(printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"slim-skill","version":"1.0"}}}' \
  "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"$TOOL\",\"arguments\":$ARGS}}" \
  | "$BIN" 2>/dev/null | tail -n 1)

if command -v python3 >/dev/null 2>&1; then
  printf '%s' "$OUT" | python3 -c '
import json, sys
r = json.load(sys.stdin)
if "result" in r:
    c = r["result"].get("content", [])
    print(c[0].get("text", "") if c else json.dumps(r["result"], ensure_ascii=False))
else:
    print(json.dumps(r.get("error", r), ensure_ascii=False))
    sys.exit(1)
'
else
  printf '%s\n' "$OUT"
fi
