---
name: slim
description: Token-slim mode — cut this session's token usage by routing every file read, code search, directory listing, and JSON dump through the token-slim MCP server (or its binary directly). Works in any repository or folder; the server itself is LLM-agnostic (any MCP client). Use when the user types /slim, or asks to save/reduce tokens, "トークン節約", "トークン削減", "コンテキスト節約", "token slim".
---

# /slim — Token-Slim Mode (session-wide)

Purpose: for the REST OF THIS SESSION, minimize tokens spent on tool results
and on your own output. Measured savings on real files: `-32%` reading Rust
source with comments stripped (mode=slim), `-95%` with the default mode=auto
outline on a large file, `-41%`
on commented Python. These rules stay active after context compaction —
re-read this skill if you lose them.

## Step 1 — Engage (pick the FIRST path that works)

1. **MCP tools available** (`mcp__token-slim__*` visible, possibly deferred):
   load ALL of them in ONE ToolSearch call, then use them for everything below.

   ToolSearch query: `select:mcp__token-slim__read_slim,mcp__token-slim__grep_slim,mcp__token-slim__dir_map,mcp__token-slim__json_slim,mcp__token-slim__text_slim,mcp__token-slim__token_count`

2. **MCP not connected** — call the binary directly via Bash using the helper
   shipped with this skill (identical behavior, works from any cwd):

   ```bash
   sh ~/.claude/skills/slim/ts.sh read_slim '{"path":"src/main.rs"}'
   sh ~/.claude/skills/slim/ts.sh dir_map
   ```

   The helper finds the binary via `$TOKEN_SLIM_BIN` →
   `~/dev/claude-code-token-slim-mcp/target/release/token-slim-mcp`
   → `which token-slim-mcp` → `~/.cargo/bin/` (no user-specific paths;
   print the resolved path with `sh ~/.claude/skills/slim/ts.sh --which`).
   Tell the user ONCE that the permanent, every-folder installation is:

   ```
   claude mcp add token-slim --scope user -- "$(sh ~/.claude/skills/slim/ts.sh --which)"
   ```

   (`--scope user` = available in every repo/folder. The server is plain
   stdio JSON-RPC with zero Claude-specific behavior, so the same binary
   also works in Codex / Cursor / Gemini CLI etc. via their `mcpServers`
   config — keep it that way; never suggest editing the server for one client.)

3. **Binary missing too** — install it (works on any machine):
   `git clone https://github.com/tacyan/claude-code-token-slim-mcp && cd claude-code-token-slim-mcp && cargo install --path .`
   (lands in `~/.cargo/bin`, auto-discovered by ts.sh). As a LAST resort run
   under the "no-tool discipline" (Step 2 rules B only, using narrow
   built-in reads and `head`-capped Bash output).

After engaging, confirm to the user in ONE short line (their language):
slim mode ON + which path (MCP / direct binary / discipline-only).

## Step 2 — Session rules (MANDATORY until the session ends)

### A. Tool substitution — never use the fat version when a slim one exists

| Instead of…                    | Use                                                        |
|--------------------------------|------------------------------------------------------------|
| Read (first look at any file)  | `read_slim {path}` — auto: outline (-95%) for big files, slim body for small ones |
| Read (specific range)          | `read_slim {path, offset, limit}` — the drill-down after an outline |
| Read (whole body on purpose)   | `read_slim {path, mode:"slim"}` — only when you truly need it |
| Grep / rg                      | `grep_slim {pattern, path, ext, max_results, exclude}`     |
| Grep with test noise           | `grep_slim {..., exclude_tests: true}` — usually the biggest single win |
| ls / Glob / tree               | `dir_map {path, depth}`                                    |
| cat of JSON / big API response | `json_slim {path}` or `json_slim {json}`                   |
| quoting long text back         | `text_slim {text, level:"aggressive"}`                     |
| "how big is this?"             | `token_count {path}` before any large read                 |

Every result starts with a `[token-slim] … ~X→~Y tok (-Z%)` header — trust it
for budgeting. Caps are tunable per call (`max_tokens`, `max_results`,
`depth`) or via env (`TOKEN_SLIM_MAX_TOKENS`, `TOKEN_SLIM_GREP_MAX_RESULTS`,
`TOKEN_SLIM_DIR_MAX_ENTRIES`).

`read_slim {path}` defaults to `mode=auto`: it returns the outline
(signatures + line numbers, nothing silently dropped) whenever the file is
big enough for the body to be worth skipping, and the slimmed body
otherwise. The header always says which one you got — `mode=outline(auto)`
means bodies are NOT in context; fetch them with `offset`/`limit`. Never
re-read the whole file just because you got an outline.

**Claude Code Edit exception**: `Edit` requires exact text previously seen via
the built-in `Read`. Workflow: locate the target with `grep_slim` /
`read_slim` (outline) first, then `Read` ONLY the narrow `offset`/`limit`
range you will edit. Never full-file `Read` when a range suffices.

### B. Output discipline (applies even with zero tools)

- Bash: cap output (`| head -50`, `2>/dev/null`, `-q` flags); never `cat` a
  file into the transcript when a slim read exists.
- Drill down, don't hoover up: outline → grep → narrow range. Read a whole
  file only when you will edit most of it.
- Don't re-read files already in context; don't paste back content the user
  already saw; quote only the lines you're discussing.
- Your replies: lead with the answer, cut boilerplate, no restating tool
  results that the user can already see.
- Delegate broad exploration to a subagent (Explore/fork) so raw dumps stay
  out of this context; keep only its conclusion.

## Tool cheat sheet (exact schemas)

- `read_slim {path, offset?, limit?, mode?: auto|slim|outline|raw, strip_comments?, max_tokens?}`
- `grep_slim {pattern, path?, ext?: "rs,toml", literal?, ignore_case?, max_results?, exclude?: ["**/test/**"], exclude_tests?, include?}`
- `dir_map {path?, depth? (default 3), max_entries?}`
- `json_slim {json | path, max_array?, max_depth?, max_string?}` — JSONC (comments / trailing commas: bun.lock, tsconfig.json, .vscode/*.json) parses too
- `text_slim {text, level?: normal|aggressive, max_tokens?}`
- `token_count {path | text}`

Direct-binary equivalent for any of the above:
`sh ~/.claude/skills/slim/ts.sh <tool> '<json-arguments>'`
