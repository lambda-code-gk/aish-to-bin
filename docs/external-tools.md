# 外部ツールプラグイン（MVP）

AISH は「外部ツールプラグインホスト」により、**本体のソース変更なし**で Playwright や Slack/Jira 等のツール連携を追加できる。

## 概要

- 信頼ディレクトリに置いた **manifest（YAML）** を読み、**stdio** で起動する外部プロセスと **JSON-RPC 2.0** で通信する。
- プラグインが返す `list_tools` の結果を AISH のツールレジストリに **動的登録** し、LLM からのツール呼び出し時に **call_tool を中継** する。
- 起動失敗・タイムアウト・不正応答時は **fail-closed**（そのプラグインのみ無効化し、AISH 全体は継続）。

## 信頼ディレクトリ（MVP）

次の **いずれか/両方** のみ対象。**project 配下は読まない**（セキュリティのため）。

- `~/.config/aish/plugins.d/*.yaml`（XDG / AISH_HOME の config に依存）
- `~/.aish/plugins.d/*.yaml`（$HOME 基準）

ディレクトリが無くてもエラーにしない。読み込み順は **ファイル名昇順** で安定化。

## Plugin Manifest 例（YAML）

```yaml
id: my-browser
version: "0.1.0"
transport:
  type: stdio
  command: node
  args: ["/path/to/plugin.js"]
env:
  DEBUG: "0"
timeouts:
  startup_ms: 15000
  call_ms: 60000
enabled: true
```

| フィールド | 必須 | 説明 |
|-----------|------|------|
| `id` | ○ | プラグイン識別子（一意推奨） |
| `version` | ○ | バージョン文字列 |
| `transport.type` | ○ | MVP は `stdio` のみ |
| `transport.command` | ○ | 起動するコマンド |
| `transport.args` | - | 引数リスト（省略時 `[]`） |
| `env` | - | 環境変数（key-value） |
| `timeouts.startup_ms` | - | 起動（initialize）タイムアウト（ms）。省略時 10000 |
| `timeouts.call_ms` | - | call_tool 1 回あたりのタイムアウト（ms）。省略時 30000 |
| `enabled` | - | `false` のときスキップ。省略時 `true` |

## JSON-RPC メソッド（MVP）

いずれも **1 行 1 JSON**（stdout に 1 行ずつレスポンスを返す）。

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

## セキュリティ方針

- **信頼ディレクトリのみ** から manifest を読む。project 配下の `.aish/plugins.d` 等は **今回は読まない**（将来 opt-in で追加する余地あり）。
- プラグインは **ユーザーが明示的に配置した** コマンドを実行する。manifest の `command` / `args` は信頼された設定として扱う。
- 起動失敗・list_tools 失敗・同名衝突時は **そのプラグインのみスキップ**（fail-closed）。AISH 本体は継続。

## イベント（EventHub / Transcript）

次の kind を emit する（payload は機密を含まない範囲で記録）。

| kind | 説明 |
|------|------|
| `external_plugin.discovered` | manifest を発見 |
| `external_plugin.start_requested` | プロセス起動試行 |
| `external_plugin.started` | 起動成功 |
| `external_plugin.start_failed` | 起動失敗 |
| `external_plugin.tools_listed` | list_tools 成功 |
| `external_plugin.tool_name_conflict` | 同名ツールをスキップ |
| `external_tool.call_requested` | 外部ツール呼び出し開始 |
| `external_tool.call_completed` | 呼び出し成功 |
| `external_tool.call_failed` | 呼び出し失敗 |

## 将来拡張

- **MCP bridge**: トランスポートに `mcp` を追加し、MCP プロトコルで通信するプラグイン対応。
- **ストリーミング**: call_tool の結果をストリームで返すオプション。
- **cancel / restart**: プラグインプロセスの再起動や呼び出しキャンセルの制御。
- **project 配下プラグイン**: opt-in で `.aish/plugins.d` 等を読むオプション。
- **プラグイン署名検証**: manifest やバイナリの署名検証（任意）。
