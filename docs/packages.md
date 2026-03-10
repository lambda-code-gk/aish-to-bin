# Package

Package は、**タスク・スキル・プロンプト・メモリトピックなどを束ねる単位**として扱われる。AISH は設定ディレクトリおよびプロジェクト配下の「packages ルート」を探索し、その直下の各サブディレクトリのうち **`package.toml` が存在するもの** を 1 つの package として読み込む。

## 目的

- 関連するタスク・スキル・システムプロンプト（hook）を一まとまりで管理する。
- プロジェクトごとやユーザーごとに、再利用可能な単位として配布・配置できる。

## 探索場所

Package は次の **2 種類のディレクトリ** を **この順** で探索する。いずれも「ディレクトリが存在する場合のみ」対象。

| 順序 | 場所 | 説明 |
|------|------|------|
| 1 | `{project_root}/.aish/packages` | プロジェクト直下。カレントディレクトリがプロジェクトルートと解釈される場合に使用される。 |
| 2 | `config/packages` | 設定ディレクトリの `packages`。`AISH_HOME` が設定されていれば `$AISH_HOME/config/packages`、そうでなければ `$XDG_CONFIG_HOME/aish/packages`（未設定時は `$HOME/.config/aish/packages`）。 |

各「packages ルート」の **直下の子ディレクトリ** のみが対象となる。子ディレクトリに **`package.toml`** が存在するものが 1 package として認識され、`package.toml` が無い、または壊れているディレクトリはスキップされる（警告ログのみで処理は継続）。

---

## package.toml の最小例

```toml
name = "my-package"
version = "0.1.0"
description = "Short description of this package"
```

### 主なフィールド（実装で利用しているもの）

| フィールド | 必須 | 説明 |
|-----------|------|------|
| `name` | ○ | package 名。 |
| `version` | - | バージョン文字列（セマンティックバージョンは強制しない）。 |
| `description` | - | 短い説明文。 |
| `system_hook` | - | この package 用のシステムプロンプトファイルへの **package ルートからの相対パス**。package 由来のタスク実行時に、タスク用プロンプトの前に挿入される。 |
| `memory_topics` | - | この package が主に扱うメモリトピックのリスト。 |

---

## package 配下の基本ディレクトリ構成例

```
packages/
└── my-package/
    ├── package.toml
    ├── tasks/           # タスクスクリプト（task.d と同様の名前解決）
    │   ├── foo.sh
    │   └── bar/
    │       └── execute
    ├── skills/          # スキル（探索時に package 配下も参照される）
    └── prompts/        # system_hook で参照するファイルなどを置く例
        └── system.md
```

- **`tasks/`**: この package に属するタスク。`ai <task_name>` で、**task.d で見つからない場合** に、各 package の `tasks/` が順に探索される。タスクの解決方法（`task_name.sh` または `task_name/execute`）は task.d と同じ。
- **`skills/`**: スキル定義。プロンプト解決時に、共通のスキル探索に package 配下の skills も含まれる。
- **`system_hook`**: `package.toml` の `system_hook` に書いた相対パス（例: `prompts/system.md`）が、package 由来タスクの実行時にシステムプロンプトとしてタスク用プロンプトの前に挿入される。

---

## task.d 由来のタスクと package 由来タスクの関係

- **タスクの解決順**: まず **task.d**（`config/task.d` および legacy の `$AISH_HOME/task.d`）を探索する。見つからなかった場合に、**各 package の `tasks/`** を順に探索する。
- **`ai --list-tasks`**: task.d から得たタスク名と、全 package の `tasks/` から得たタスク名を合わせて表示する（重複は除く）。
- **プロンプト解決**: package 由来のタスクの場合、その package に `system_hook` が指定されていれば、その内容をタスク用プロンプトより前に挿入する。

---

## 参照

- タスクの一般的な説明: [docs/commands.md](commands.md) および [docs/ai-usage.md](ai-usage.md)
- 設定ディレクトリの解決: `AISH_HOME` および XDG に従う（他ドキュメントおよび実装の `EnvResolver::resolve_dirs` を参照）
