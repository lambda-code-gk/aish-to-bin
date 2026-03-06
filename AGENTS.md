# AGENTS.md - AI開発エージェント向けプロジェクト知識ベース

## ⚠️ 作業前の必須確認

- コード編集・追加・修正の**前**にこのAGENTS.mdを読む
- **作業前**: `./tests/architecture.sh && ./tests/units.sh && ./tests/integration.sh` で全テスト成功を確認
- **作業後**: `cargo fmt && ./tests/architecture.sh && ./tests/units.sh && ./tests/integration.sh` で既存機能が壊れていないことを確認
- **エラー修正時**: 修正後、同様の失敗を繰り返さないように AGENTS.md に注意・ルールを追記する

---

## アーキテクチャ（逆流禁止の判断基準）

### 1. 依存方向（一方向のみ許可）

```
  CLI（main + cli） → usecase → ports（outbound） ← adapters（実装）
                              ↑
                         wiring がここで「adapter を usecase に注入」
```

- **CLI**: `main.rs` と `cli/`。引数解析・用法表示・エラー表示・終了コード。`parse_args()` で `Config` を取得し、必要に応じて **wiring で提供されるポート（例: `ResolveModeConfig`, `ResolveSystemPromptFromHooks`）で Config を補完**したうえで、`Runner` を通じて usecase を呼ぶ。
- **usecase**: アプリの手続き（`AiUseCase`, `TaskUseCase`, `ShellUseCase` 等）。**port（trait）経由でのみ** I/O や外界に触れる。
- **ports**: インターフェース定義。usecase は「outbound」の trait にのみ依存する。adapter はその trait を実装する。
- **adapters**: 具体実装（FS・プロセス・LLM・承認・タスク実行等）。**wiring 以外から生成・参照されない**。
- **wiring**: 唯一の「composition root」。adapter を生成し、usecase に trait として渡す。main は「コマンド分岐」と「wiring の呼び出し」のみ行う。

許可される依存の向きだけを書く：

- `main` → `cli`, `wiring`, `ports::inbound`
- `wiring` → `adapter`, `usecase`, `ports::outbound`（および common）
- `usecase` → `ports::outbound`, `domain`, `common` のみ
- `adapter` → `ports::outbound`（実装）、`common` 等

### 2. usecase 層の禁止事項（明文化）

usecase モジュール（`apps/ai/src/usecase/`, `apps/aish/src/usecase/`）では以下を**禁止**する。

- **adapter を import しない**  
  `use crate::adapter::*` や `use crate::adapter::StdTaskRunner` 等は書かない。必要なのは port（trait）だけ。
- **cli に依存しない**  
  `use crate::cli::*` は禁止。Config 等は main が解釈し、usecase には「必要な値」だけを引数で渡す。
- **std::env を直接読まない**  
  環境変数・カレントディレクトリは adapter や cli で読み、usecase には `SessionDir` 等の値として注入する。
- **stdout / stderr に直接出力しない**  
  `println!` / `eprintln!` / `std::io::stdout()` 等は usecase では使わない。表示は port（例: `EventSinkFactory`）や cli の責務。
- **wiring に依存しない**  
  `use crate::wiring::*` は禁止。usecase は「trait を受け取って動く」だけであり、誰がその実装を渡すかは知らない。

### 3. wiring（composition root）の責務

- **adapter の生成は wiring のみが行う**  
  `StdTaskRunner::new(...)`, `PartSessionStorage::new(...)` 等、具象アダプタの `new` / ファクトリは `wiring.rs` 内だけに書く。
- **usecase は trait（port）だけを受け取る**  
  `AiUseCase::new(fs, history_loader, response_saver, ...)` のように、引数はすべて `Arc<dyn SomePort>` などの trait 型。wiring が adapter を `Arc<dyn SomePort>` にしたうえで渡す。
- **main の役割はコマンド分岐と wiring 呼び出しが中心**  
  `let config = parse_args()?;` → `let app = wire_ai();`（または `wire_aish()`）→ `Runner { app }.run(config)` の流れを基本としつつ、必要に応じて wiring から提供されるポート（例: `ResolveModeConfig`, `ResolveSystemPromptFromHooks`）を用いて Config を最小限に補完してよい。ビジネスロジックや OS 依存の詳細は main に書かない。

### 4. Inbound / Outbound port の役割

- **Inbound port（ドライバ → アプリ）**  
  呼び出し側（main）がアプリを実行するためのインターフェース。例: `UseCaseRunner::run(&self, config: Config) -> Result<i32, Error>`。main は `config_to_command(config)` で Command にし、`match` で分岐したうえで、各 usecase や `app.run_query` を呼ぶ。usecase は **inbound を実装しない**（main 側の `Runner` が実装する）。
- **Outbound port（アプリ → 外界）**  
  usecase が「ファイル」「プロセス」「LLM」「承認」「タスク実行」等を使うための trait。例: `SessionHistoryLoader`, `TaskRunner`, `ToolApproval`, `EventSinkFactory`。usecase はこれらの **trait にのみ依存**し、実装（adapter）は wiring が注入する。

### 5. 実装時のチェックリスト

コードを書いたら、以下を確認する。

- [ ] このコードは **usecase から adapter を参照していないか？**（`grep -r "crate::adapter" apps/ai/src/usecase apps/aish/src/usecase` が空であること）
- [ ] **stdout / stderr を usecase で触っていないか？**（`println!` / `eprintln!` / `std::io::stdout` 等が usecase に無いこと）
- [ ] **env を usecase で直接読んでいないか？**（`std::env::var` / `std::env::current_dir` 等が usecase に無いこと）
- [ ] **usecase が cli や wiring に依存していないか？**（`use crate::cli` / `use crate::wiring` が usecase に無いこと）
- [ ] **adapter の new / 生成は wiring にだけあるか？**（main や usecase から adapter を `new` していないこと）
- [ ] **main は「parse_args → wire → Runner.run」以外のロジックを持っていないか？**

---

## プロジェクト概要・構造

- **AISH**: CUI 自動化フレームワーク（LLM 連携）。シェルスクリプトから Rust への刷新中。
- **libs/common**: `ai` / `aish` 共通。エラー型、session、LLM ドライバ・プロバイダ、Part ID、Port trait（FileSystem, Process, Clock 等）と標準実装、Tool trait / ToolRegistry。**ai 専用・aish 専用のユースケースは置かない。**  
  - Outbound の trait のうち **Tool** と **LlmProvider** は、ドメイン型（ToolContext, Message 等）との循環参照を避けるため、それぞれ `common::tool` と `common::llm::provider` に定義し、`common::ports::outbound` から re-export している。その他の outbound trait は `ports/outbound` に定義。
- **apps/ai**: `ai` コマンド。main → cli → wiring → UseCaseRunner。usecase: `app.rs`（AiUseCase）, `task.rs`（TaskUseCase）, `query_loop.rs`（QueryLoop）, `agent_loop.rs`（AgentLoop 外側）。adapter: sinks, task, part_session_storage, approval, tools, resolve_system_prompt_from_hooks 等。CLI 層では、`-S/--system` 未指定時に hooks ベースでシステムプロンプトを解決して `Config` を補完する（解決順: `$AISH_HOME/config/hooks/system_prompt/`, `$HOME/.aish/hooks/system_prompt/`, プロジェクト直下の `.aish/hooks/system_prompt/`）。
- **apps/aish**: `aish` コマンド。main → cli → wiring → UseCaseRunner。usecase: shell, truncate_console_log, clear 等。adapter: shell, terminal, platform, logfmt 等。

ビルド・テストはプロジェクトルートで `./build.sh`, `./tests/units.sh`, `./tests/integration.sh`。個別は `cd apps/ai && cargo test` 等。

---

## 開発方針（要約）

- **TDD**: 失敗するテストを先に書く → 通す最小実装 → リファクタ。テスト省略禁止。
- **エラー**: usecase 内は `Result<T, common::error::Error>`。CLI 境界で `exit_code()` / `is_usage()` により終了コード・用法表示を決定。
- **common 肥大化防止**: 2 crate 以上で共有され安定したものだけ common に置く。ai 専用・aish 専用は各 crate の adapter / usecase に置く。OS 副作用のある具象ツール実装は `apps/*/adapter/` に置く。システムプロンプトの注入は hooks ベースのアダプタ（`ResolveSystemPromptFromHooks`）で行い、usecase からは直接扱わない。
- **文字列の切り詰め（UTF-8）**: `&str` をバイト長で切り詰める場合、**必ず文字境界で切る**こと。`&s[..n]` のようにバイト位置 `n` でそのままスライスすると、UTF-8 の多バイト文字（日本語の「コ」等）の途中で切り、`byte index N is not a char boundary` でパニックになる。切り詰め位置を `n` にしたあと、`str::is_char_boundary(n)` が真になるまで `n` を減らすか、`char_indices()` で文字境界だけを扱うこと。

### セッションディレクトリ構造と migrations 運用

- **スキーマバージョンの定義場所**  
  - 共通モジュール `libs/common/src/session_schema.rs` で `SESSION_SCHEMA_LATEST` と `SESSION_SCHEMA_VERSION_FILE`（`session_schema_version`）を定義し、`require_latest()` でバージョンチェックを行う。
  - 新規セッション作成時のみ、`Session::new()` が `<session_dir>/session_schema_version` に最新バージョン（例: `2\n`）を書き込む。既存セッションには**自動では書かない**。

- **セッションディレクトリ構造を変更するときの手順**  
  1. まず **新しい構造を決める**（例：`manifest.jsonl` → `reviewed_history.jsonl`、`events/events.ndjson` → `events.jsonl` など）。
  2. `SESSION_SCHEMA_LATEST` を +1 する（例：1 → 2）。
  3. `scripts/migrations/` に **新バージョン用のシェルスクリプト**を追加する（例：`0002_events_jsonl.sh`）。  
     - `<session_dir>` を引数に取り、**ファイル移動やrenameだけを行う**。
     - 途中で失敗したら即座に exit し、部分的な中途状態を残さないようにする。
  4. 既存の migration runner `scripts/migrate.sh` が **番号順（0001 → 0002 → ...）に適用**して `session_schema_version` を更新する想定で書かれているため、新しいスキーマにしたい場合は runner に手を入れず **migration スクリプトを足すだけ**でよい。

- **Rust 側での互換性の考え方**  
  - セッションディレクトリの互換性は **`scripts/migrate.sh` + `scripts/migrations/` がすべて担保する**。
  - Rust 側（adapter / usecase / storage）は、**常に最新スキーマだけを前提にしてよい**。古いレイアウト（古いファイル名・ディレクトリ構造）をコード内で条件分岐して扱わない。
  - 代表例：
    - 履歴ローダ `ManifestReviewedSessionStorage` やイベントストア `NdjsonSessionEventStore` は、処理の冒頭で `require_latest()` を呼び、`session_schema_version != SESSION_SCHEMA_LATEST` や version ファイル欠如は **明示的なエラー**にする（「Run scripts/migrate.sh ...」というメッセージ）。
    - ファイルパスは常に最新のもの（例：`<session_dir>/reviewed_history.jsonl`、`<session_dir>/events.jsonl`）だけを参照する。
  - 旧構造のセッションを開くときは、ユーザー/CI が **先に `scripts/migrate.sh -s <session_dir>` を実行してから** `ai` / `aish` を動かす想定とする。

- **今後の migrations 追加時のチェックリスト**  
  - [ ] `SESSION_SCHEMA_LATEST` を +1 したか？
  - [ ] 対応する `scripts/migrations/000N_*.sh` を追加したか？
  - [ ] 新スキーマ専用の Rust 実装（パスや構造）に書き換え、古いスキーマ向けの分岐を残していないか？
  - [ ] `./tests/units.sh` で **旧構造を前提にしていたテスト**が `session_schema_version` を明示的に書くようになっているか？

---

## 禁止事項

- テストを省略して実装を進めること
- **usecase から adapter / cli / wiring を参照すること**、**usecase で std::env / stdout / stderr を直接使うこと**
- この AGENTS.md を読まずに作業を開始すること

---

## 参照

- 結合テスト: `./tests/integration.sh`（作業前後に必須）
- 単体テスト: `./tests/units.sh`
- 既知のバグ: `BUGS.md`
- サブプロジェクト: `legacy/old_impl/tools/aish-capture/AGENTS.md` 等

## 更新履歴

- **2026年3月**: 文字列切り詰めの UTF-8 文字境界ルールを追加（`truncate_str` 等でバイトスライスが多バイト文字の途中で切れてパニックになる事象を踏まえ）。エラー修正時は AGENTS.md を更新して同様の失敗を防ぐことを必須確認に追加。セッションディレクトリ構造変更時の migrations 運用ルールと、互換性は shell migrations で担保し Rust 側は最新スキーマのみを扱う方針を明文化。
- **2026年2月**: common の port & adapter 整理。adapter から port の re-export を削除し、usecase は `common::ports::outbound` から trait を参照。StdIdGenerator を adapter に移動。Tool / LlmProvider が ports 外に定義されている理由を明記。
- **2026年2月**: 旧 sysq（システムプロンプトの専用サブコマンド/UseCase/Adapter）を廃止。代わりに hooks ベースのシステムプロンプト解決（`ResolveSystemPromptFromHooks`）を導入し、`-S` 未指定時は hooks（`$AISH_HOME/config/hooks/system_prompt/`, `$HOME/.aish/hooks/system_prompt/`, プロジェクト直下の `.aish/hooks/system_prompt/`）からの解決を試行する仕様に統一。
- **2026年2月**: アーキテクチャを「逆流防止」の判断基準として整理。依存方向・usecase 禁止事項・wiring 責務・inbound/outbound・実装時チェックリストを明文化。長さを抑え実務で参照しやすい形に変更。
- **2026年1月**: common / ai / aish の状態・モジュール・CLI を現状に合わせて見直し。
