# AISH

AISH は、LLM によって Linux の CUI 体験を強化するための **CUI 自動化フレームワーク**です。  
Rust 製の統合バイナリ `aish` (およびそのエイリアス `ai`) として提供され、ターミナルの入出力をコンテキストとして LLM に渡し、

- シェルに対する操作や質問を **自然言で記述**したり
- エージェントに **コマンド実行・ファイル編集・検索などを自律的に任せたり**
- **レビューやコミットメッセージ生成などの開発タスクを自動化**したり

といったことができます。

⚠️ **Important**: AISH はターミナルの入出力やファイル内容を外部の LLM API（例: OpenAI, Gemini など）に送信します。  
機密情報・大きなバイナリ・個人情報などを含むデータは送らないでください。自己責任で利用してください。

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/lambda-code-gk/aish)

## ✨ Features

* **Integrated Command System**: `aish` バイナリが記録（shell）、応答（ai）、セッション管理（sessions）などの機能をサブコマンドとして集約。
* **Context-aware interactions**: ターミナルの直近の入出力をコンテキストとして自動収集。
* **Policy & Security Control**: `config.toml` による強力なポリシー制御。ツールの実行承認、LLM 送信データの検知・マスキング（leakscan 連携）が可能。
* **Memory / Session System**: 過去の履歴を永続化し、必要に応じて LLM コンテキストへ注入。
* **External Tooling (MCP)**: Model Context Protocol (MCP) を通じた外部ツールの拡張に対応。
* **Task-oriented workflows**: タスクスクリプトによる複雑なワークフローの自動化。

## 📚 Documentation

- **はじめに**: [docs/README.md](docs/README.md) — ドキュメントの目次と読む順番
- 詳細なコンセプトとユースケース: [docs/overview.md](docs/overview.md)
- コマンド一覧と概要: [docs/commands.md](docs/commands.md)
- `aish` の使い方とセッション管理: [docs/aish-usage.md](docs/aish-usage.md)
- `ai` の使い方（最も詳しいガイド）: [docs/ai-usage.md](docs/ai-usage.md)
- セキュリティ・プライバシーと leakscan: [docs/security.md](docs/security.md)

## 🚀 Quick Start

### Installation

1. **リポジトリをクローン**:
    ```bash
    git clone https://github.com/lambda-code-gk/aish.git
    cd aish
    ```

2. **バイナリをビルド**:
    ```bash
    ./build.sh
    ```
    ビルド成果物（`aish`, `ai`, `leakscan`, `md-fmt`）は **`dist/bin/`** に配置されます。

3. **環境設定**:
    利用する LLM プロバイダの API キーを設定します。
    ```bash
    export OPENAI_API_KEY=sk-...
    # または
    export GOOGLE_API_KEY=...
    ```

4. **セッション開始**:
    ```bash
    export PATH=$PWD/dist/bin:$PATH
    aish
    ```

## ⚙️ Configuration

AISH の挙動は `config.toml` および環境変数で細かく制御できます。

### config.toml

プロジェクトルートの `.aish/config.toml` または `$XDG_CONFIG_HOME/aish/config.toml` を参照します。
サンプルは `assets/defaults/config/config.toml.sample` にあります。

- **Policy**: ツール実行の承認モード（`allow` | `require_approval` | `deny`）を機能（capability）やツールごとに設定。
- **Sensitive Data**: LLM 送信時の秘密情報の扱い（`deny` | `mask` | `allow`）。
- **Limits**: 送信文字数のハード上限（`egress_hard_cap_chars`）など。

### Environment Variables

| 変数名 | 説明 |
| :--- | :--- |
| `AISH_CONTEXT_STRATEGY` | コンテキスト削減戦略 (`tail` (default), `legacy`) |
| `AISH_CONTEXT_MAX_CHARS` | LLM に送るコンテキストの最大文字数 |
| `AISH_DAEMON` | バックグラウンド記録プロセスの使用 (`on`, `off`, `auto`) |

## 🕹 Usage

AISH は統合バイナリ `aish` を通じて利用します。また、`ai` は `aish ai` へのエイリアスとして機能します。

### Subcommands

| コマンド | 説明 |
| :--- | :--- |
| `aish` | インタラクティブ・シェルを開始（ターミナル記録の開始） |
| `ai <msg>` | 自然言語による LLM への問い合わせ |
| `ai <task>` | タスクスクリプトの実行 |
| `aish sessions` | セッション一覧の表示 |
| `aish policy` | 解決済みポリシーとルール順の表示 |
| `aish config` | 現在の有効な設定とソースの表示 |
| `aish memory` | 保存されたメモリ（ナレッジ）の管理 |

### Task Examples

| タスク名 | 説明 |
| :--- | :--- |
| `ai commit_msg` | ステージ済み変更からコミットメッセージ候補を生成 |
| `ai fixit` | 直前のコマンドエラーを解析して修正案を提示 |

利用可能なタスク一覧は `ai --list-tasks` で確認できます。

## 📂 Project Structure

```text
aish/
├── crates/aish            # 統合バイナリ（ai / aish）のエントリポイント
├── core/                  # コアロジック (Library)
│   ├── ai                 # LLM 連携・エージェント・ポリシー制御
│   ├── aish               # ターミナル記録・セッション管理
│   └── common             # 共通ドメイン・抽象ポート・ドライバ
├── tools/                 # 補助ツール (leakscan, md-fmt 等)
├── assets/defaults/       # 設定テンプレート
├── dist/bin/              # ビルド成果物
└── tests/                 # テストスクリプト
```

## 📄 License

This project is licensed under the MIT License. See the `LICENSE` file for details.
