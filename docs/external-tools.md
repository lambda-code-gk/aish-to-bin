# 外部ツールプラグイン

AISH は「外部ツールプラグインホスト」により、**本体のソース変更なし**で Playwright や Slack/Jira 等のツール連携を追加できる。

## 概要

- 信頼ディレクトリに置いた **manifest** を読み、**stdio** で起動する外部プロセスと **JSON-RPC 2.0** で通信する。
- プラグインが返す `list_tools` の結果を AISH のツールレジストリに **動的登録** し、LLM からのツール呼び出し時に **call_tool を中継** する。
- 起動失敗・タイムアウト・不正応答時は **fail-closed**（そのプラグインのみ無効化し、AISH 全体は継続）。

**現在の主軸は stdio + JSON-RPC ベース**である。狭義の Model Context Protocol (MCP) を正式対応済みという意味ではない。将来的に MCP 互換のブリッジ拡張を入れる余地はある。

---

## 現在の正式仕様

### 正規 manifest: `plugin.toml`（TOML）

- 正規の manifest 形式は **TOML** の **`plugin.toml`** である。
- 各探索ディレクトリ内で、**サブディレクトリ直下の `plugin.toml`** または **ディレクトリ直下の `*.toml` ファイル** が 1 プラグインとして読み込まれる。
- 同一ディレクトリに `plugin.toml` と `*.yaml` が混在していてもよい（後述の legacy 互換）。

### トランスポート

- 現状サポートしているのは **stdio** のみ。manifest の `command` と `args` で子プロセスを起動し、stdin/stdout で JSON-RPC 2.0 のメッセージをやり取りする。

---

## 探索場所と優先順

プラグインは次の **4 種類のディレクトリ** を **この順** で探索する。いずれも「ディレクトリが存在する場合のみ」対象となり、無くてもエラーにはならない。

| 順序 | 場所 | スコープ | 説明 |
|------|------|----------|------|
| 1 | `{project_root}/.aish/plugins` | Project | プロジェクト直下。カレントディレクトリがプロジェクトルートと解釈される場合に使用される。 |
| 2 | `config/plugins` | UserConfig | 設定ディレクトリの `plugins`。`AISH_HOME` が設定されていれば `$AISH_HOME/config/plugins`、そうでなければ XDG に従い `$XDG_CONFIG_HOME/aish/plugins`（未設定時は `$HOME/.config/aish/plugins`）。 |
| 3 | `config/plugins.d` | LegacyUser | 上記と同じ config ベースの `plugins.d`。 |
| 4 | `~/.aish/plugins.d` | LegacyUser | ホームの `.aish/plugins.d`。 |

各ディレクトリ内では、**サブディレクトリに `plugin.toml` があるもの** および **直下の `plugin.toml` または `*.toml` / `*.yaml` / `*.yml`** を収集し、**パス文字列の昇順** で処理する。同一 `id` のプラグインは **先に現れたものだけ有効** となり、後続はスキップされる（誤配送防止）。

---

## plugin.toml の例とフィールド

### 最小例（TOML）

```toml
id = "my-browser"
namespace = "my-browser"
command = "node"
args = ["/path/to/plugin.mjs"]
enabled = true
```

### フィールド一覧（plugin.toml）

| フィールド | 必須 | 説明 |
|-----------|------|------|
| `id` | ○ | プラグイン識別子。一意必須。重複時は先に処理した manifest のみ有効化し、後続はスキップされる。 |
| `namespace` | ○ | ツール名の名前空間。LLM に渡すツール名は `namespace` とツール名から合成される。 |
| `command` | ○ | 起動するコマンド（stdio プロセス）。 |
| `args` | - | 引数リスト。省略時は `[]`。 |
| `cwd` | - | 子プロセスの作業ディレクトリ。省略時は未指定。 |
| `env_allowlist` | - | 子プロセスに渡す環境変数名の allowlist。安全のためデフォルトは空。 |
| `enabled` | - | `false` のときそのプラグインはスキップ。**デフォルトは false**（deny-by-default）。有効にするには明示的に `enabled = true` を書く。 |
| `timeout_ms` | - | call_tool 1 回あたりのタイムアウト（ms）。省略時は実装依存のデフォルト。 |
| `[policy]` | - | ポリシー補助。`default_tool_mode`（例: `require_approval`）、`capabilities_hint`、`notes` など。 |

---

## stdio の使い方

- **stdout**: JSON-RPC 専用。1 行 1 JSON でリクエストへの応答のみを出力すること。ログやデバッグ出力は stdout に書かない（応答の id 不一致や parse エラーの原因になる）。
- **stderr**: デバッグ・ログ用。ホストは stderr を読み捨ててパイプ詰まりを防ぎ、末尾のみバッファに保持する場合がある。内容は transcript/イベントに出す場合はマスク・切り詰めを前提とする。

---

## JSON-RPC メソッド（現状）

いずれも **1 行 1 JSON**（stdout に 1 行ずつレスポンスを返す）。レスポンスの `id` はリクエストの `id` と一致すること（不一致時は protocol error で fail-closed）。

### initialize

- **params**: `{ "protocolVersion": "0.1" }`（任意）
- **result**: 任意（成功の JSON-RPC result があればよい）

### list_tools

- **params**: なし
- **result**: ツール定義の **配列**。各要素は:
  - `name` (string): ツール名（例: `browser.open`）
  - `description` (string): LLM 用説明
  - `input_schema` (object): JSON Schema（LLM の parameters にそのまま渡す）

### call_tool

- **params**: `{ "name": "<tool_name>", "arguments": <JSON> }`
- **result**: `{ "content": <JSON> }`（AISH は `content` をツール結果として LLM に返す）

---

## Legacy 互換（YAML）

旧仕様の **YAML manifest**（`*.yaml` / `*.yml`）も、上記の探索ディレクトリに置かれていれば読み込まれる。

- 探索順・ディレクトリの種類は TOML と同じ（project `.aish/plugins`、config/plugins、config/plugins.d、~/.aish/plugins.d）。
- **enabled** は YAML では省略時 **false**（deny-by-default）。有効にするには `enabled: true` を明示する。
- 形式の対応: `transport.type` = `stdio`、`transport.command` / `transport.args`、`timeouts.call_ms` など。`namespace` は YAML に無い場合は `id` が使われる。

YAML は「互換目的の旧形式」として扱い、新規には **plugin.toml（TOML）** を推奨する。

---

## セキュリティ方針

- **上記 4 種類の探索場所のみ** から manifest を読む。それ以外の任意パスは読まない。
- **プロジェクト配下**（`{project_root}/.aish/plugins`）も探索対象に含まれる。プロジェクト内に manifest を置けば、そのプロジェクトでだけ有効なプラグインを配布できる。
- プラグインは **ユーザーが明示的に配置した** コマンドを実行する。manifest の `command` / `args` は信頼された設定として扱う。
- 起動失敗・list_tools 失敗・同名衝突時は **そのプラグインのみスキップ**（fail-closed）。AISH 本体は継続。
- TOML では `env_allowlist` で子プロセスに渡す環境変数を制限する。YAML legacy では env の扱いは実装に依存する。

---

## イベント（EventHub / Transcript）

次の kind を emit する（payload は機密を含まない範囲で記録）。

| kind | 説明 |
|------|------|
| `external_plugin.discovered` | manifest を発見 |
| `external_plugin.start_requested` | プロセス起動試行 |
| `external_plugin.started` | 起動成功 |
| `external_plugin.start_failed` | 起動失敗 |
| `external_plugin.skipped_id_conflict` | plugin_id 重複のためスキップ（先勝ち） |
| `external_plugin.tools_listed` | list_tools 成功 |
| `external_plugin.tool_name_conflict` | 同名ツールをスキップ |
| `external_tool.call_requested` | 外部ツール呼び出し開始 |
| `external_tool.call_completed` | 呼び出し成功 |
| `external_tool.call_failed` | 呼び出し失敗 |

---

## 将来拡張

- **MCP bridge**: トランスポートに `mcp` を追加し、MCP プロトコルで通信するプラグイン対応。現状は stdio + JSON-RPC のみ。
- **ストリーミング**: call_tool の結果をストリームで返すオプション。
- **cancel / restart**: プラグインプロセスの再起動や呼び出しキャンセルの制御。
- **プラグイン署名検証**: manifest やバイナリの署名検証（任意）。
