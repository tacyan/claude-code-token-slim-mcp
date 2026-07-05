# token-slim-mcp

**全 LLM クライアント共通で使える、トークン削減用ローカル MCP サーバー(Rust 製)**

Claude Code / Codex CLI / Cursor / Claude Desktop / Gemini CLI など、MCP(Model Context Protocol)対応クライアントなら何でも接続できます。一度設定すれば、以後のファイル読み取り・検索・JSON 処理を「削減済みの形」でコンテキストに入れられるため、恒常的にトークン消費を減らせます。

- 依存は `serde` / `serde_json` / `regex` のみ。外部通信なし・完全ローカル。
- JSON-RPC 2.0 over stdio(改行区切り)の MCP 標準実装。
- シングルバイナリ約 1.7MB。

## 仕組み

LLM のトークン消費の大半は「ファイルの中身・検索結果・API レスポンスをそのままコンテキストに入れること」で発生します。本サーバーは、それらを **入れる前に** 圧縮します。

| ツール | 代替対象 | 削減内容 | 削減目安 |
|---|---|---|---|
| `read_slim` | 通常の Read/cat | コメント・空行除去+トークン上限キャップ(先頭+末尾保持) | 10〜60% |
| `read_slim` (mode=outline) | ファイル全読み | 関数/クラス/見出しのシグネチャ行のみ抽出 | **90%以上** |
| `grep_slim` | grep / 検索ツール | `path:行番号:一致行` のみ・件数上限・vendor/バイナリ除外 | 大 |
| `dir_map` | ls -R / find | 1行1エントリの省トークンツリー(深さ・件数上限) | 大 |
| `json_slim` | JSON 全貼り | minify+深さ制限+配列サンプリング+長文字列切詰め | 50〜99% |
| `text_slim` | ログ全貼り | 空行・空白圧縮、重複行の集約 | 内容次第 |
| `token_count` | — | 貼る前にトークン数を見積もる(±20%) | — |

全ツールの応答先頭に `[token-slim] ~4835→~238 tok (-95%)` の形式で削減実績が付きます。

## インストール

Rust([rustup](https://rustup.rs/))が必要です。

### 方法 A: cargo install(推奨・パスが環境に依存しない)

```bash
git clone https://github.com/tacyan/claude-code-token-slim-mcp.git
cd claude-code-token-slim-mcp
cargo install --path .
```

バイナリは `~/.cargo/bin/token-slim-mcp` に入ります(以降の設定例はこのパスを使用)。

### 方法 B: 手動ビルド

```bash
git clone https://github.com/tacyan/claude-code-token-slim-mcp.git
cd claude-code-token-slim-mcp
cargo build --release
# → <クローンした場所>/target/release/token-slim-mcp
```

方法 B の場合は、以降の設定例の `~/.cargo/bin/token-slim-mcp` を
`<クローンした場所>/target/release/token-slim-mcp` の**絶対パス**に読み替えてください。

> **注意**: MCP クライアントによっては `~` を展開しないものがあります。うまく接続できない場合はフルパス(例: `/Users/<ユーザー名>/.cargo/bin/token-slim-mcp`、Linux なら `/home/<ユーザー名>/...`)で指定してください。パスは `which token-slim-mcp` で確認できます。

## クライアント設定(一度設定すればずっと有効)

### Claude Code

```bash
# 全プロジェクトで有効(user スコープ)
claude mcp add --scope user token-slim \
  --env TOKEN_SLIM_MAX_TOKENS=4000 \
  -- ~/.cargo/bin/token-slim-mcp
```

### Codex CLI(`~/.codex/config.toml`)

```toml
[mcp_servers.token-slim]
command = "~/.cargo/bin/token-slim-mcp"

[mcp_servers.token-slim.env]
TOKEN_SLIM_MAX_TOKENS = "4000"
```

### Cursor(`~/.cursor/mcp.json` またはプロジェクトの `.cursor/mcp.json`)

```json
{
  "mcpServers": {
    "token-slim": {
      "command": "~/.cargo/bin/token-slim-mcp",
      "env": { "TOKEN_SLIM_MAX_TOKENS": "4000" }
    }
  }
}
```

### Claude Desktop

設定ファイルの場所:
- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

Cursor と同じ `mcpServers` 形式です(Claude Desktop は `~` を展開しないためフルパス推奨)。

### Gemini CLI(`~/.gemini/settings.json`)

```json
{
  "mcpServers": {
    "token-slim": {
      "command": "~/.cargo/bin/token-slim-mcp"
    }
  }
}
```

### Claude Code スキル `/slim`(同梱)

セッション単位で省トークンモードを確実に有効化するスキルを [`skills/slim/`](skills/slim/) に同梱しています。導入はフォルダごとコピーするだけ:

```bash
cp -r skills/slim ~/.claude/skills/
```

以降、任意のリポジトリ・フォルダのセッションで `/slim` と打つと、ファイル読取→`read_slim`、検索→`grep_slim`、一覧→`dir_map` に置き換わります。MCP 未登録でも同梱の `ts.sh` がバイナリを直接呼び出すため動作します(探索順: `$TOKEN_SLIM_BIN` → `~/dev/claude-code-token-slim-mcp/target/release/` → `PATH` → `~/.cargo/bin/`)。

### Claude Code スキル `/bulk`(同梱)

`/slim` と対になる爆速モードを [`skills/bulk/`](skills/bulk/) に同梱しています。サブエージェントの並列大量投入と多段ワークフローでトークンを一気に使い、壁時計時間を最小化します(オーケストレータ側は `/slim` 規律で軽量のまま)。導入:

```bash
cp -r skills/bulk ~/.claude/skills/
```

任意のセッションで `/bulk <ミッション>` と打つと、その起動自体がマルチエージェント編成へのオプトインになります。`/slim` と併用すると最大効果です。

## モデルに使わせるための推奨設定(重要)

MCP ツールは「登録しただけ」では標準の Read/Grep より優先されないことがあります。プロジェクトの `CLAUDE.md` や `AGENTS.md` に以下を追記すると、恒常的に削減されます:

```
ファイル読み取り・コード検索・ディレクトリ確認には token-slim MCP の
read_slim / grep_slim / dir_map を優先して使うこと。
大きな JSON は json_slim、長いログは text_slim を通してから引用すること。
```

## 環境変数(デフォルト設定)

| 変数 | 既定値 | 意味 |
|---|---|---|
| `TOKEN_SLIM_MAX_TOKENS` | 4000 | read_slim / text_slim の出力トークン上限 |
| `TOKEN_SLIM_GREP_MAX_RESULTS` | 50 | grep_slim の結果件数上限 |
| `TOKEN_SLIM_DIR_MAX_ENTRIES` | 300 | dir_map の合計エントリ上限 |
| `TOKEN_SLIM_JSON_MAX_DEPTH` | 6 | json_slim の深さ上限 |
| `TOKEN_SLIM_JSON_MAX_ARRAY` | 20 | json_slim の配列保持件数 |
| `TOKEN_SLIM_JSON_MAX_STRING` | 200 | json_slim の文字列保持文字数 |

各ツール呼び出し時の引数(`max_tokens` など)が環境変数より優先されます。

## ツールリファレンス

### read_slim
```jsonc
{ "path": "src/main.rs",        // 必須
  "mode": "slim",               // slim(既定) | outline | raw
  "max_tokens": 4000,           // 出力上限(超過分は中間を snip)
  "offset": 100, "limit": 50,   // 行範囲指定
  "strip_comments": true }      // slim 時のコメント除去
```
対応言語(コメント除去): Rust, JS/TS, Python, Go, Java, C/C++, C#, Swift, Kotlin, Ruby, Shell, SQL, Lua, HTML/XML, YAML, TOML ほか。

### grep_slim
```jsonc
{ "pattern": "fn \\w+_slim",    // 必須(Rust regex)
  "path": ".",                  // ルート(ファイル単体も可)
  "ext": "rs,toml",             // 拡張子フィルタ
  "max_results": 50,
  "ignore_case": false,
  "literal": false }            // true でリテラル一致
```

### dir_map
```jsonc
{ "path": ".", "depth": 3, "max_entries": 300 }
```

### json_slim
```jsonc
{ "json": "{...}",              // または "path": "package-lock.json"
  "max_depth": 6, "max_array": 20, "max_string": 200 }
```

### text_slim
```jsonc
{ "text": "長いログ...", "level": "normal" }  // normal | aggressive
```

### token_count
```jsonc
{ "text": "..." }               // または "path": "file"
```

## 注意事項

- 削減は**非可逆(lossy)**です。コンパイル・実行用途にはなりません(LLM のコンテキスト投入専用)。
- トークン推定は tokenizer 非依存のヒューリスティック(ASCII 4文字≒1トークン+非ASCII 1文字≒1トークン)で、±20% 程度の誤差があります。
- 出力上限超過時は先頭70%+末尾20%を保持し、中間に snip マーカーを入れます。

## 開発

```bash
cargo test        # ユニット11件+MCP 統合テスト1件
cargo build --release
```

手動での動作確認:
```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}' | ./target/release/token-slim-mcp
```

## ライセンス

MIT
