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
| `read_slim` (既定 mode=auto) | ファイル全読み | 大きなコードファイルは**アウトライン**(関数/**クラスメソッド**/見出し+行番号)、小さい・非構造ファイルは slim 本文 | **90%以上** / 10〜60% |
| `read_slim` (mode=slim) | 通常の Read/cat | コメント・空行除去+トークン上限キャップ(先頭+末尾保持) | 10〜60% |
| `grep_slim` | grep / 検索ツール | `path:行番号:一致行` のみ・件数上限・vendor/バイナリ除外・**glob 除外**(`exclude` / `exclude_tests`) | 大 |
| `refs_slim` | grep で「呼び出し元」を探す | シンボルの参照を**定義/呼び出し/テスト/import/コメント/言及に分類**し、実際の呼び出しだけを「それを含む関数名」付きで返す。`depth=2` で変更の影響範囲 | **10〜90%** |
| `dir_map` | ls -R / find | 1行1エントリの省トークンツリー(深さ・件数上限) | 大 |
| `json_slim` | JSON 全貼り | minify+深さ制限+配列サンプリング+長文字列切詰め。**JSONC 対応**(コメント・末尾カンマ) | 50〜99% |
| `text_slim` | ログ全貼り | 空行・空白圧縮、重複行の集約 | 内容次第 |
| `token_count` | — | 貼る前にトークン数を見積もる(±20%) | — |

全ツールの応答先頭に `[token-slim] ~4835→~238 tok (-95%)` の形式で削減実績が付きます。

### read_slim の既定は「まずアウトライン」

大きなファイルで本文を返すと、上限にかかって**中間が黙って切り捨てられます**(例: `src/tools.rs` は mode=slim で `-48% [capped]`)。既定の `mode=auto` は、そういうファイルでは代わりに構造だけを完全な形で返します(`-95%`、欠落は「本文」だけと明示)。

```
[token-slim] src/tools.rs mode=outline(auto) lines=626 ~6426→~292 tok (-95%)
  — structure only, no bodies: fetch a function with offset/limit, or the whole file with mode=slim
L69: pub fn tool_definitions() -> Value {
L140: pub fn call(params: &Value) -> Result<Value, (i64, String)> {
...
```

必要な関数だけ `offset`/`limit` で取りに行く流れになるため、事故が少なく削減も大きくなります。判定基準:

- `offset`/`limit` 指定時は常に本文(その呼び出し自体がドリルダウン)
- 本文が `max_tokens` を超える(=切り捨てが発生する)ならアウトライン
- そうでなくても、本文が `TOKEN_SLIM_AUTO_OUTLINE_MIN_TOKENS`(既定 400)超 かつ アウトラインが本文の半分以下ならアウトライン
- 上記以外(小さいファイル、シグネチャが無い設定/データファイル)は slim 本文

`mode` を明示すればいつでも上書きでき、既定自体も `TOKEN_SLIM_DEFAULT_MODE=slim` で戻せます。

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
read_slim / grep_slim / refs_slim / dir_map を優先して使うこと。
「この関数を誰が呼んでいるか」「この変更の影響範囲は」は grep ではなく refs_slim で調べること。
大きな JSON は json_slim、長いログは text_slim を通してから引用すること。
```

## 環境変数(デフォルト設定)

| 変数 | 既定値 | 意味 |
|---|---|---|
| `TOKEN_SLIM_MAX_TOKENS` | 4000 | read_slim / text_slim の出力トークン上限 |
| `TOKEN_SLIM_DEFAULT_MODE` | auto | read_slim の既定モード(auto/slim/outline/raw) |
| `TOKEN_SLIM_AUTO_OUTLINE_MIN_TOKENS` | 400 | auto がアウトラインを選び始める本文サイズ |
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
  "mode": "auto",               // auto(既定) | slim | outline | raw
  "max_tokens": 4000,           // 出力上限(超過分は中間を snip)
  "offset": 100, "limit": 50,   // 行範囲指定(指定時は常に本文)
  "strip_comments": true }      // slim/auto 時のコメント除去
```
対応言語(コメント除去): Rust, JS/TS, Python, Go, Java, C/C++, C#, Swift, Kotlin, Ruby, Shell, SQL, Lua, HTML/XML, YAML, TOML ほか。

### grep_slim
```jsonc
{ "pattern": "fn \\w+_slim",    // 必須(Rust regex)
  "path": ".",                  // ルート(ファイル単体も可)
  "ext": "rs,toml",             // 拡張子フィルタ
  "max_results": 50,
  "ignore_case": false,
  "literal": false,             // true でリテラル一致
  "exclude": ["**/test/**"],    // glob 除外(* ? ** 対応、"a,b" 文字列も可)
  "exclude_tests": false,       // true で test/spec/fixture 系を一括除外
  "include": ["src/**"] }       // 指定時はこれに一致するパスのみ検索
```

glob は `*`(1セグメント内の任意文字)・`?`・`**`(0個以上のセグメント)に対応。
スラッシュを含まないパターン(`tests`、`*.spec.ts`)は任意のパスセグメントに一致し、
複数セグメントのパターン(`tests/**`)は階層の途中(`crates/foo/tests/…`)にも一致します。
除外ディレクトリは走査自体をスキップするため、除外は速度にも効きます。

```
# before
[token-slim] grep /read_slim/ in .: 29 matches, 13 files scanned
# after
[token-slim] grep /read_slim/ in .: 4 matches, 8 files scanned, exclude=**/tests/**,README.md,skills, 1 files skipped
```

`exclude_tests: true` は `**/test/**` `**/tests/**` `**/__tests__/**` `**/spec/**`
`**/testdata/**` `**/fixtures/**` `*_test.*` `*.test.*` `*.spec.*` などに展開されます。
ヘッダーには常に有効なフィルタが表示されるので、0 件を「どこにも無い」と誤読しません。

### dir_map
```jsonc
{ "path": ".", "depth": 3, "max_entries": 300 }
```

### json_slim
```jsonc
{ "json": "{...}",              // または "path": "bun.lock" / "tsconfig.json"
  "max_depth": 6, "max_array": 20, "max_string": 200 }
```
**JSONC 対応**: まず厳密な JSON として解析し、失敗した場合のみ `//` 行コメント・
`/* */` ブロックコメント・末尾カンマを除去して再解析します(文字列リテラル内は保護、
行番号もずれません)。`bun.lock` / `tsconfig.json` / `.vscode/*.json` がそのまま通ります。
JSONC 経路を通った場合はヘッダーに `jsonc (comments/trailing commas stripped)` と出ます。
本当に壊れた JSON はこれまで通りエラーになります。

### text_slim
```jsonc
{ "text": "長いログ...", "level": "normal" }  // normal | aggressive
```

### refs_slim

「この関数を誰が呼んでいるか」に答えます。grep はこれに答えられません — 宣言・import・テスト・実際の呼び出しを区別できないからです。

```json
{"symbol": "computeFrameComp", "path": "packages", "ext": "ts"}
```

```
[token-slim] refs computeFrameComp in packages: definition 1, call 2, test-call 17, import 2, comment 5, mention 9; 62 files scanned
def  renderer-webgpu/src/look-math.ts:82: export function computeFrameComp(input: FrameCompInput): FrameComp {
call renderer-webgl/src/renderer.ts:505  in frame: const { fadeAlpha, ... } = computeFrameComp({
call renderer-webgpu/src/renderer.ts:697  in frame: const { fadeAlpha, ... } = computeFrameComp({
```

同じ問いに対する出力量(実測、TypeScript 62 ファイル):

| 手法 | 出力量 | 答え |
|---|---:|---|
| 素の `grep -rn` | 4,237 B | 36 行(うち 34 行はノイズ) |
| `grep_slim` | 3,881 B | 同じ 36 行 |
| **`refs_slim`** | **482 B** | **定義 1 + 呼び出し 2、正解と一致** |

各呼び出しは**それを含む関数名**に紐付きます(`in frame`)。ブレース深度を追跡するため、既に閉じた内側のアロー関数が後続の行を横取りしません。

`depth: 2` を渡すと、その関数群の呼び出し元まで辿ります — 変更の影響範囲です。

```
hop2 buildRenderPipeline -> gpuBlendState  (renderer-webgpu/src/renderer.ts:315)
hop2 frame -> syncLookModes  (renderer-webgpu/src/renderer.ts:642)
```

索引を持たないので**常に最新**です。1 秒前に書いた関数もそのまま見えます。

> **制約**: 呼び出し元の帰属はブレース深度で解決するため、`in <関数名>` の付与と `depth=2` は**ブレースで区切る言語**（TypeScript / JS / Java / C# / Rust / Go / C++ など）でのみ機能します。Python や Ruby では分類と一覧までは正しく動きますが、`depth=2` はその旨を明示して何も返しません（誤った「呼び出し元なし」は返しません）。

| 引数 | 既定 | 説明 |
|---|---|---|
| `symbol` | (必須) | 追跡する識別子 |
| `path` | `.` | 探索ルート |
| `ext` | — | 拡張子フィルタ(`ts,tsx`) |
| `depth` | 1 | 2 で推移的な呼び出し元まで |
| `include_tests` | false | テスト内の呼び出しも一覧に出す(既定は件数のみ) |
| `max_results` | 50 | 一覧の上限 |
| `exclude` / `include` | — | glob(`grep_slim` と同じ構文) |

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
cargo test        # ユニット22件+MCP 統合テスト1件
cargo build --release
```

手動での動作確認:
```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}' | ./target/release/token-slim-mcp
```

## ライセンス

MIT
