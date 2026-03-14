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

### 5. domain 層の設計方針（Decision / Plan / Policy パターン）

domain 層は「型定義の置き場」ではなく、**判断ロジック（純関数）の置き場** でもある。

- **domain service (`*Service`) は原則作らない**。判断は **Decision / Plan / Policy / Candidate / Resolution** の値型 + 純関数で表す。
- **domain は I/O を一切知らない**。`FileSystem`, `Process`, `Repo` 等の trait に依存しない。材料収集は adapter / usecase が行い、domain には値だけを渡す。
- **判断が必要な箇所の設計パターン**: usecase は「材料を集める → domain の純関数に渡す → 戻った結果を adapter に渡す」だけ。adapter は I/O に徹する。
- **命名ルール**: `*Service` の代わりに `*Decision`, `*Plan`, `*Policy`, `*Resolution`, `*Candidate` を使う。
- **戻り値に diagnostics を含める**: Decision / Plan 型は `selected`, `rejected`, `warnings`, `provenance` 等を持ち、判断のブラックボックス化を防ぐ。
- **domain 内のモジュール構成**: `domain/prompt/`, `domain/task/`, `domain/context/`, `domain/policy/`, `domain/query/` のように対象コンセプト配下に value object・enum・decision・plan・pure function を置く。

### 6. 実装時のチェックリスト

コードを書いたら、以下を確認する。

- [ ] このコードは **usecase から adapter を参照していないか？**（`grep -r "crate::adapter" apps/ai/src/usecase apps/aish/src/usecase` が空であること）
- [ ] **stdout / stderr を usecase で触っていないか？**（`println!` / `eprintln!` / `std::io::stdout` 等が usecase に無いこと）
- [ ] **env を usecase で直接読んでいないか？**（`std::env::var` / `std::env::current_dir` 等が usecase に無いこと）
- [ ] **usecase が cli や wiring に依存していないか？**（`use crate::cli` / `use crate::wiring` が usecase に無いこと）
- [ ] **adapter の new / 生成は wiring にだけあるか？**（main や usecase から adapter を `new` していないこと）
- [ ] **main は「parse_args → wire → Runner.run」以外のロジックを持っていないか？**
- [ ] **domain に I/O 依存（`FileSystem`, `Process`, `std::fs`, `std::env` 等）がないか？**（domain は純関数と値型のみ）
- [ ] **判断ロジックが adapter / usecase に残っていないか？**（ルール・条件分岐・優先順位の決定は domain に寄せる）
- [ ] **編集したファイルに冒頭の責務一行がある場合、それに反していないか？**（確認時は `.cursor/skills/check-responsibility/SKILL.md` の手順に従う）

### 7. 責務の一行と確認

- **各ソースの冒頭に責務を一行で書く**  
  そのモジュールが「何のみを行い、何をしないか」を一行で明示する。Rust では `//! 責務: …` のモジュール doc をファイル先頭に置く。境界が重要な adapter（policy・usecase 等）から順に揃える。責務を記述する際は、**SRP（単一責任の原則）**・**関心の分離**・**Ports and Adapters** 等の原理原則を考慮する。
- **変更後は責務違反がないか確認する**  
  冒頭に責務の一行があるファイルを編集したら、プロジェクトの SKILL「check-responsibility」（`.cursor/skills/check-responsibility/SKILL.md`）の手順で、コードがその責務に反していないか確認する。AGENTS.md を読まないモデルでも、この SKILL を参照すれば確認手順が同じになる。

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
- **タスク内で `ai` をネスト呼び出しする場合**: 下位 `ai` コマンドが非ゼロ終了したら、その stderr/stdout を LLM 正常応答として後続パースしないこと。`tee` やパイプを挟む場合でも元コマンドの終了コードを保持し、失敗時はマーカー抽出や JSON パースに進まず即座に明示的なエラーで終了すること。
- **backend 実行中の task から `ai` をネスト呼び出しする場合**: 下位 `ai` も daemon 経由で動かせるように、`run_ai` は daemon 本体で直接実行せず worker process に分離すること。process-global な `cwd` / `AISH_HOME` / `AISH_SESSION` を daemon 本体に抱えたまま再入させると self-deadlock の原因になる。
- **backend 実行中の task から `ai` をネスト呼び出しする場合 その2**: task 子プロセスには `AISH_JOB_ID` / `AISH_JOB_DEPTH` / `AISH_MAX_BACKEND_JOB_DEPTH` を明示的に引き継ぎ、nested `ai` は `stdin` が非 TTY でも local task 経路に落とさず backend を優先すること。これが欠けると深さ制限や親子 job 関係が効かなくなる。
- **nested job の cancel**: 親 job を cancel したら active registry 上の子孫 job もまとめて cancel し、子 job を orphan のまま走らせないこと。nested worker を個別 kill できるよう parent-child 関係は daemon 側 registry に残す。
- **nested job の interaction**: backend 実行中の task から起動された nested `ai` は `stdin` が非 TTY になりやすい。この場合 approval / continue / sensitive prompt をローカル stdin で待たず、fail-closed（approval deny / continue false / sensitive deny）で自動応答すること。
- **nested job の出力帰属**: nested `ai` の本文出力は親 task の stdout/stderr に流れるため、backend client 側で child job の開始・終了を stderr に明示し、どの nested job の出力か追えるようにすること。
- **nested job の interactive notice**: nested `ai` が approval / continue / sensitive prompt を要求した場合、親 frontend 側の stderr に interaction 種別と prompt preview を出し、非 TTY の auto-resolve 時も結果を明示すること。
- **nested job の frontend TTY**: truly interactive な nested prompt を親 frontend に返すため、root `ai` は frontend TTY パスを backend request に載せ、worker は `AISH_FRONTEND_TTY` として保持すること。nested child は `stdin` 非 TTY でもこの TTY を開いて prompt を出せるようにする。
- **frontend SIGINT**: backend 実行中に frontend が `Ctrl+C` を受けたら、frontend だけ先に死なず current job を `cancel_ai` し、その subtree を停止させること。
- **read-only `aish` コマンドの backend 化**: `memory list/get`, `history ls/get`, `plugins list`, `tools list` は daemon 未起動時でも従来どおり local fallback で動作させること。read-only backend 化で daemon 必須にしてはいけない。
- **backend task 出力転送時の stdin**: 出力観測や転送を入れても task の stdin 意味論は変えないこと。`run()` と `run_observing()` で pipe / read の挙動が変わらないようにする。
- **foreground daemon の終了経路**: `aish daemon start` のような foreground 常駐プロセスは、`Ctrl+C`（SIGINT）で確実に accept loop を抜けて終了し、Unix ソケット等のランタイムファイルを掃除すること。手動確認や結合テストでは SIGINT で停止できることまで確認する。
- **backend job の cancel 永続化**: backend job を cancel した場合、`events.jsonl` 上の terminal state は `completed` に上書きせず `cancelled` のまま残すこと。`daemon jobs --persisted` でも cancel 後は `cancelled` を見せる。
- **`ai` の backend 経路を非 TTY で結合テストするとき**: 現在の CLI は先頭位置引数を task と解釈するため、`stdin` が TTY でない実行では `should_use_backend()` が local 実行を選ぶことがある。backend 実行を明示的に検証する integration / manual test では `AISH_FORCE_BACKEND=1` を付けること。
- **backend 実行タスクの出力経路**: `ai` / `aish` の backend が task や subprocess を実行する場合、stdout / stderr / prompt を daemon 側コンソールへ直接出さないこと。ユーザーに見せる出力は frontend へ転送し、backend は中継に徹すること。

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
- **aish の実行テスト**: 以下のディレクトリを使用する。`aish -d <PROJ_ROOT>/tmp/home`
- 既知のバグ: `BUGS.md`
- サブプロジェクト: `legacy/old_impl/tools/aish-capture/AGENTS.md` 等

## 更新履歴

- **2026年3月**: domain 層の設計方針（Decision / Plan / Policy パターン）を追加。adapter / usecase の判断ロジックを domain の純関数に移動する方針と、実装時チェックリストに「domain に I/O 依存がないこと」「判断ロジックが adapter に残っていないか」を追加。
- **2026年3月**: 責務記述の際に SRP・関心の分離・Ports and Adapters 等の原理原則を考慮する旨を「責務の一行と確認」に追記。
- **2026年3月**: 責務の一行（ファイル冒頭）と SKILL（check-responsibility）による確認手順を追加。実装時チェックリストに「冒頭の責務に反していないか」を追加。
- **2026年3月**: 文字列切り詰めの UTF-8 文字境界ルールを追加（`truncate_str` 等でバイトスライスが多バイト文字の途中で切れてパニックになる事象を踏まえ）。エラー修正時は AGENTS.md を更新して同様の失敗を防ぐことを必須確認に追加。セッションディレクトリ構造変更時の migrations 運用ルールと、互換性は shell migrations で担保し Rust 側は最新スキーマのみを扱う方針を明文化。
- **2026年3月**: `evolve` のように task から `ai` をネスト実行する場合は、下位 `ai` の失敗ログを正常な LLM 応答としてパースしないルールを追加。`tee` 使用時も元コマンドの終了コードを保持し、失敗時は即時エラーにする。
- **2026年3月**: backend 実行中の task から nested `ai` を呼ぶ場合は、task 子プロセスに `AISH_JOB_ID` / `AISH_JOB_DEPTH` / `AISH_MAX_BACKEND_JOB_DEPTH` を明示的に渡し、nested `ai` は `AISH_JOB_DEPTH` があるとき backend を優先するルールを追加。`stdin` 非 TTY だからと local 経路へ落とすと深さ制限が効かない。
- **2026年3月**: nested job の cancel は親だけでなく active な子孫 job にも再帰的に伝播させるルールを追加。親だけ kill して child を orphan のまま残さない。
- **2026年3月**: backend task から起動された nested `ai` が `stdin` 非 TTY のまま approval / continue prompt で詰まらないよう、nested context では fail-closed の自動応答を返すルールを追加。
- **2026年3月**: nested `ai` の出力帰属を追えるよう、child job の開始・終了は stderr に job_id 付きで明示するルールを追加。
- **2026年3月**: nested `ai` の interactive request を親 frontend から追えるよう、approval / continue / sensitive prompt は prompt preview を stderr に出し、auto-resolve 結果も明示するルールを追加。
- **2026年3月**: root `ai` は frontend TTY パスを backend request に含め、nested child は `stdin` 非 TTY でも `AISH_FRONTEND_TTY` を使って truly interactive な prompt を返せるルールを追加。
- **2026年3月**: backend 実行中の `Ctrl+C` は frontend だけでなく daemon job subtree にも `cancel_ai` を送って停止させるルールを追加。
- **2026年3月**: daemon が worker stdout/stderr を同一 client socket へ relay する場合、書き込みは必ず直列化するルールを追加。複数 thread から `write_frame_sync` を並行実行すると frame が壊れ、nested backend failure 時に protocol parse error を起こす。
- **2026年3月**: integration で backend query/task の出力内容を固定文字列で検証する場合は、呼び出し時に `AISH_SESSION` / `AISH_JOB_DEPTH` / `AISH_FRONTEND_TTY` を明示的に外して外部シェル環境を混入させないルールを追加。既存 session/history が混ざると echo provider の query/history 表示が変わる。
- **2026年3月**: `aish daemon start` の foreground daemon は SIGINT で終了し、ソケットを掃除するルールを追加。手動確認や integration でも Ctrl+C 終了を確認する。
- **2026年2月**: common の port & adapter 整理。adapter から port の re-export を削除し、usecase は `common::ports::outbound` から trait を参照。StdIdGenerator を adapter に移動。Tool / LlmProvider が ports 外に定義されている理由を明記。
- **2026年2月**: 旧 sysq（システムプロンプトの専用サブコマンド/UseCase/Adapter）を廃止。代わりに hooks ベースのシステムプロンプト解決（`ResolveSystemPromptFromHooks`）を導入し、`-S` 未指定時は hooks（`$AISH_HOME/config/hooks/system_prompt/`, `$HOME/.aish/hooks/system_prompt/`, プロジェクト直下の `.aish/hooks/system_prompt/`）からの解決を試行する仕様に統一。
- **2026年2月**: アーキテクチャを「逆流防止」の判断基準として整理。依存方向・usecase 禁止事項・wiring 責務・inbound/outbound・実装時チェックリストを明文化。長さを抑え実務で参照しやすい形に変更。
- **2026年1月**: common / ai / aish の状態・モジュール・CLI を現状に合わせて見直し。
