## AISH のコマンド概要

このドキュメントでは、AISH が提供する主要なコマンドの「地図」を示します。  
詳細な使い方は、各専用ドキュメント（`aish-usage.md` / `ai-usage.md` など）を参照してください。

### `aish` コマンド

`aish` は「AISH シェル」のエントリーポイントです。

```bash
aish [options] [<command> [args...]]
```

- **役割**:  
  - セッションディレクトリを作成し、ターミナルの入出力を記録する。
  - `clear` や `memory list` など、AISH 独自のサブコマンドを提供する。

- **主なオプション**
  - `-h, --help` : ヘルプを表示。
  - `-d, --home-dir` : このプロセス専用の `AISH_HOME` を指定。
  - `-s, --session-dir` : 既存セッションディレクトリを明示して再開。
  - `-v, --verbose` : デバッグログを標準エラーに出力。
  - `--generate <shell>` : シェル補完スクリプトを生成（bash, zsh, fish）。

- **主なサブコマンド**

| サブコマンド | 説明 |
| --- | --- |
| *(なし)* | 省略すると、インタラクティブな `aish` シェルを起動します。 |
| `clear` | セッションディレクトリ内の Part ファイルを削除し、会話履歴をクリアします。 |
| `truncate_console_log` | コンソールログ/バッファを切り詰めます。主に `ai` との連携用です。 |
| `init [--force] [--dry-run] [--defaults-dir DIR]` | テンプレ（`AISH_DEFAULTS_DIR` または `--defaults-dir`）を XDG/AISH_HOME の config にコピーします。 |
| `memory list` | メモリ一覧（id, category, subject）を表示します。 |
| `memory get <id> [id...]` | 指定 ID のメモリ内容を取得します。 |
| `memory remove <id> [id...]` | 指定 ID のメモリを削除します。 |
| `history ls [-a][-u][--all]` | reviewed 履歴一覧。`-a`=assistant, `-u`=user, `--all`=すべて。 |
| `history get <id> [...]` | 指定 ID の reviewed 内容を取得します。 |
| `sessions` | セッション一覧・操作（例: `sessions rebuild-derived`）。 |
| `resume [<id>]` | セッションを再開します。 |
| `rollout` | バッファをフラッシュしログをロールオーバー（SIGUSR1 相当）。 |
| `mute` | ロールアウト後に console 記録を停止します。 |
| `unmute` | console 記録を再開します。 |
| `policy explain` | `ai --policy-explain` を実行します。 |
| `config explain` | `ai --config-explain` を実行します。 |
| `plugins list` | 外部プラグイン一覧を表示します。 |
| `tools list` | 外部ツール一覧を表示します。 |
| `daemon start \| ping \| status` | デーモン制御（起動・疎通・状態）。 |

> 詳細: `aish` の起動・セッションの挙動については `aish-usage.md` を参照してください。

### `ai` コマンド

`ai` は、現在のセッションコンテキストを踏まえて LLM に問い合わせるためのコマンドです。

```bash
ai [options] [task] [message...]
```

- **役割**
  - `message` に与えたプロンプトと、`aish` セッション（`AISH_SESSION`）から読み取ったログ・履歴などをコンテキストとして LLM に送信し、応答を表示します。
  - 特定のタスク名が `task` に指定され、対応するスクリプトが見つかった場合は、そのタスクスクリプトを実行します。

- **主なオプション**

| オプション | 説明 |
| --- | --- |
| `-h, --help` | ヘルプを表示します。 |
| `-L, --list-profiles` | 利用可能なプロバイダプロファイル一覧を表示します。 |
| `--list-tools` | 有効なツール一覧を表示します（現状はプロファイルに依存しません）。 |
| `-c, --continue` | 直前のエージェントループ状態から再開します（`AISH_SESSION` を利用）。 |
| `--no-interactive` | 非対話モード。ツール承認などを自動で拒否し、CI 向けに使うことを想定しています。 |
| `-v, --verbose` | デバッグログを標準エラーに出力します。 |
| `-p, --profile <profile>` | LLM プロファイルを指定します（例: `gemini`, `gpt`, `echo` など）。 |
| `-m, --model <model>` | モデル名を指定します（例: `gemini-2.0`, `gpt-4` など）。 |
| `-M, --mode <name>` | モード（プリセット）を指定します。`$AISH_HOME/config/mode.d/<name>.json` に定義された `system`/`profile`/`tools` を補完します。 |
| `-S, --system <instruction>` | この問い合わせ専用のシステムインストラクションを直接指定します。 |
| `--generate <shell>` | シェル補完スクリプトを生成します。 |
| `--list-tasks` | 利用可能なタスク名一覧を表示します（シェル補完用）。 |
| `--list-modes` | 利用可能なモード名一覧を表示します（シェル補完用）。 |

- **TAB キーでの補完**

  `ai` では、TAB キーでオプション値やタスク名を補完できます。有効にするには、利用中のシェル用の補完スクリプトを生成し、読み込んでください。

  ```bash
  # 例: bash の場合
  ai --generate bash   # 出力をファイルに保存するか、source で読み込む
  source <(ai --generate bash)   # 一時的に有効化

  # 例: 設定に永続的に追加する場合（bash）
  ai --generate bash >> ~/.bashrc
  ```

  `fish` の場合は `ai --generate fish`、`zsh` の場合は `ai --generate zsh` を同様に実行・読み込みます。

  補完される主な項目:
  - **タスク名** — `task.d` に存在するタスク（`ai ` の直後に TAB）
  - **`-p` / `--profile`** — 利用可能なプロファイル名（`ai -p ` の直後に TAB）
  - **`-M` / `--mode`** — 利用可能なモード名（`ai -M ` の直後に TAB）
  - オプション名（`-h`, `--help`, `-c`, `-v` など）

- **タスクスクリプト**

`task` に指定した名前に対応するスクリプトが見つかった場合、LLM への問い合わせの代わりにタスクスクリプトが実行されます。

- 検索パス（この順で探索し、先に存在する方を使用）
  1) **設定ディレクトリの task.d**: `AISH_HOME` 設定時は `$AISH_HOME/config/task.d/`、未設定時は `$XDG_CONFIG_HOME/aish/task.d/`（未設定時は `~/.config/aish/task.d/`）
  2) **互換パス**（`AISH_HOME` 設定時のみ）: `$AISH_HOME/task.d/`

> 詳細: `ai` の具体的な使い方やプロンプト設計、セッションとの関係については `ai-usage.md` を参照してください。

### サポートツール / leakscan

代表的なものに `leakscan` があります（機密情報の誤送信を防ぐための検査エンジン）。

- `ai` からの利用時は、以下の順でバイナリを探索し、ルールファイルが存在するときのみ有効化されます。
  1) **設定ホームの bin/leakscan**: `AISH_HOME` 設定時は `$AISH_HOME/bin/leakscan`、未設定時は `$XDG_CONFIG_HOME/aish/bin/leakscan`（未設定時は `~/.config/aish/bin/leakscan`）
  2) **`ai` バイナリと同じディレクトリ**の `leakscan`
  - ルールファイルは `AISH_HOME` 設定時は `$AISH_HOME/config/rules.json`、未設定時は `$XDG_CONFIG_HOME/aish/rules.json`（未設定時は `~/.config/aish/rules.json`）を参照します。
- 見つからない場合やルールが無い場合は、leakscan は無効になり、セッションの準備はスキップされます。

詳細が必要になったタイミングで `tools/` 以下や各ツールのヘルプを参照してください。

### モード（`-M, --mode`）

`ai` はモード機能を持ち、`$AISH_HOME/config/mode.d/<name>.json`（または XDG 配下）に定義されたプリセットから、`system`/`profile`/`tools` を補完できます。`--list-modes` で利用可能なモード名の一覧を表示でき、CLI で `-p/-m/-S` を明示した場合はそれらがモードより優先されます。`-S` 未指定かつモードで system が無い場合は、hooks ベースのシステムプロンプト解決を試行します。
