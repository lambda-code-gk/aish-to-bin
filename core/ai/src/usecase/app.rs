use crate::domain::AgentMode;
use crate::domain::Query;
use crate::domain::{DryRunInfo, EventEnvelope, LifecycleEvent, PolicyVerdict, QueryOutcome};
use crate::ports::outbound::{
    AgentStateLoader, AgentStateSaver, CommandAllowRulesLoader, ContextArtifactStore,
    ContextPackBuilder, ContinueAfterLimitPrompt, DryRunReportSink, EventSinkFactory,
    InterruptChecker, LifecycleHooks, LlmEventStreamFactory, PolicyEngine,
    PrepareSessionForSensitiveCheck, ProfileLister, QueryPlacement, ResolveMemoryDir,
    ResolveProfileAndModel, RunQuery, SessionDerivedBuilder, SessionEventStore,
    SessionHistoryLoader, SessionResponseSaver, ToolApproval,
};
use crate::usecase::agent_judge::{
    AgentJudge, CompositeJudge, HeuristicJudge, LlmJudge, PlanJudge,
};
use crate::usecase::agent_loop::{AgentLoop, AgentLoopConfig, AgentLoopOutcome};
use common::domain::event::{Event, RunId, SessionId};
use common::domain::{EventEnvelopeWithoutSeq, SessionDir};
use common::error::Error;
use common::event_hub::EventHubHandle;
use common::msg::Msg;
use common::ports::outbound::Clock;
use common::ports::outbound::EnvResolver;
use common::ports::outbound::EventAppender;
use common::ports::outbound::{now_iso8601, FileSystem, Log, LogLevel, LogRecord, Process};
use common::tool::{CommandAllowRule, Tool, ToolContext, ToolRegistry};
use std::sync::Arc;

// --- 責務別 Deps（usecase が定義を所有し、wiring は組み立てるだけ）

pub struct AiDeps {
    pub session: SessionDeps,
    pub policy: PolicyDeps,
    pub tooling: ToolingDeps,
    pub model: ModelDeps,
    pub system: SystemDeps,
    pub obs: ObsDeps,
    pub lifecycle_hooks: Arc<dyn LifecycleHooks>,
    /// dry run の結果を出力する先（どこに出すかは adapter が実装）
    pub dry_run_report_sink: Arc<dyn DryRunReportSink>,
    /// CI 等で true のとき確認プロンプトを出さない（run イベントの payload に載せる）
    pub non_interactive: bool,
}

pub struct SessionDeps {
    pub fs: Arc<dyn FileSystem>,
    pub history_loader: Arc<dyn SessionHistoryLoader>,
    pub context_pack_builder: Arc<dyn ContextPackBuilder>,
    pub artifact_store: Arc<dyn ContextArtifactStore>,
    pub policy_engine: Arc<dyn PolicyEngine>,
    pub response_saver: Arc<dyn SessionResponseSaver>,
    pub agent_state_saver: Arc<dyn AgentStateSaver>,
    pub agent_state_loader: Arc<dyn AgentStateLoader>,
    pub prepare_session_for_sensitive_check: Option<Arc<dyn PrepareSessionForSensitiveCheck>>,
    /// leakscan が有効で manifest/reviewed 履歴を使っている場合 true（dry run 表示用）
    pub leakscan_enabled: bool,
    /// セッションイベント永続（events.ndjson 読み書きのうち読み取りと低レベル append）
    pub session_event_store: Arc<dyn SessionEventStore>,
    /// 派生物再生成（index.sqlite / snapshots/summary.json）
    pub session_derived_builder: Arc<dyn SessionDerivedBuilder>,
    /// イベントの ts_ms 採番用
    pub clock: Arc<dyn Clock>,
    /// セッションイベント append（単一入口。seq 採番 + append を担当）
    pub event_appender: Arc<dyn EventAppender>,
}

pub struct PolicyDeps {
    pub continue_prompt: Arc<dyn ContinueAfterLimitPrompt>,
    pub env_resolver: Arc<dyn EnvResolver>,
    pub resolve_memory_dir: Arc<dyn ResolveMemoryDir>,
    pub command_allow_rules_loader: Arc<dyn CommandAllowRulesLoader>,
    /// run_shell の allowlist（config の policy.tools.run_shell.allowlist）。ツール実行時の command_allow_rules にマージする。
    pub run_shell_allowlist: Vec<String>,
    pub approver: Arc<dyn ToolApproval>,
    pub interrupt_checker: Arc<dyn InterruptChecker>,
}

pub struct ToolingDeps {
    pub sink_factory: Arc<dyn EventSinkFactory>,
    pub tools: Vec<Arc<dyn Tool>>,
}

pub struct ModelDeps {
    pub profile_lister: Arc<dyn ProfileLister>,
    pub resolve_profile_and_model: Arc<dyn ResolveProfileAndModel>,
    pub llm_stream_factory: Arc<dyn LlmEventStreamFactory>,
    pub llm_completion: Arc<dyn crate::ports::outbound::LlmCompletion>,
}

pub struct SystemDeps {
    pub process: Arc<dyn Process>,
}

pub struct ObsDeps {
    pub log: Arc<dyn Log>,
}

/// ai のユースケース（アダプター経由で I/O を行う）
pub struct AiUseCase {
    deps: AiDeps,
}

impl AiUseCase {
    pub fn new(deps: AiDeps) -> Self {
        Self { deps }
    }

    pub(crate) fn session_is_valid(&self, session_dir: &Option<SessionDir>) -> bool {
        if let Some(ref dir) = session_dir {
            self.deps.session.fs.exists(dir.as_ref())
                && self
                    .deps
                    .session
                    .fs
                    .metadata(dir.as_ref())
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
        } else {
            false
        }
    }

    /// 現在有効なプロファイル一覧を返す（ソート済み名前リストとデフォルトプロファイル名）。
    /// 表示は CLI の責務のため、usecase はデータのみ返す。
    pub fn list_profiles(&self) -> Result<(Vec<String>, Option<String>), Error> {
        self.deps.model.profile_lister.list_profiles()
    }

    /// 有効なツール一覧を返す（名前と説明）。プロバイダごとの有効/無効は未対応のため常に全ツール。
    /// 表示は CLI の責務のため、usecase はデータのみ返す。
    pub fn list_tools(&self) -> Vec<(String, String)> {
        self.deps
            .tooling
            .tools
            .iter()
            .map(|t| (t.name().to_string(), t.description().to_string()))
            .collect()
    }

    /// dry run: LLM を呼ばず、採用されるプロファイル・モデル・システムプロンプト・メッセージ列・有効ツールを返す。
    /// 保存は行わない（save_user しない）。
    pub fn dry_run_query(
        &self,
        session_dir: Option<SessionDir>,
        provider: Option<common::domain::ProviderName>,
        model: Option<common::domain::ModelName>,
        query: Option<&Query>,
        system_instruction: Option<&str>,
        tool_allowlist: Option<&[String]>,
        mode_name: Option<String>,
    ) -> Result<DryRunInfo, Error> {
        let (profile_name, model_name) = self
            .deps
            .model
            .resolve_profile_and_model
            .resolve(provider.as_ref(), model.as_ref())?;

        let (messages, budget_report, attachments_count) = match query {
            None => {
                let dir = session_dir.as_ref().ok_or_else(|| {
                    Error::invalid_argument(
                        "No continuation state. Use -c with a session or provide a message.",
                    )
                })?;
                if !self.session_is_valid(&session_dir) {
                    return Err(Error::invalid_argument(
                        "No continuation state. Use -c with a session or provide a message.",
                    ));
                }
                let msgs = self
                    .deps
                    .session
                    .agent_state_loader
                    .load(dir)?
                    .ok_or_else(|| {
                        Error::invalid_argument(
                            "No continuation state. Use -c with a session or provide a message.",
                        )
                    })?;
                (msgs, None, None)
            }
            Some(q) => {
                let (history_messages, query_placement) = if self.session_is_valid(&session_dir) {
                    let dir = session_dir.as_ref().expect("session_dir is Some");
                    let loaded = self.deps.session.history_loader.load(dir);
                    match loaded {
                        Ok(history) => (history.messages().to_vec(), QueryPlacement::AppendAtEnd),
                        Err(_) => (Vec::new(), QueryPlacement::AppendAtEnd),
                    }
                } else {
                    (Vec::new(), QueryPlacement::AppendAtEnd)
                };
                let pack = self.deps.session.context_pack_builder.build(
                    &history_messages,
                    Some(q),
                    system_instruction,
                    query_placement,
                )?;
                (
                    pack.messages,
                    Some(pack.budget_report),
                    Some(pack.attachments.len()),
                )
            }
        };

        let allowlist: Option<std::collections::HashSet<&str>> =
            tool_allowlist.map(|s| s.iter().map(String::as_str).collect());
        let tools_enabled: Vec<String> = self
            .deps
            .tooling
            .tools
            .iter()
            .filter(|t| {
                allowlist
                    .as_ref()
                    .map_or(true, |list| list.contains(t.name()))
            })
            .map(|t| t.name().to_string())
            .collect();

        Ok(DryRunInfo {
            profile_name,
            model_name,
            system_instruction: system_instruction.map(String::from),
            mode_name,
            leakscan_enabled: self.deps.session.leakscan_enabled,
            tool_allowlist: tool_allowlist.map(|s| s.to_vec()),
            tools_enabled,
            messages,
            budget_report,
            attachments_count,
        })
    }

    /// dry run を実行する: 結果を組み立て、report 先に渡す（出力先は adapter が実装）。
    pub fn run_dry_run(
        &self,
        session_dir: Option<SessionDir>,
        provider: Option<common::domain::ProviderName>,
        model: Option<common::domain::ModelName>,
        query: Option<&Query>,
        system_instruction: Option<&str>,
        tool_allowlist: Option<&[String]>,
        mode_name: Option<String>,
    ) -> Result<(), Error> {
        let info = self.dry_run_query(
            session_dir,
            provider,
            model,
            query,
            system_instruction,
            tool_allowlist,
            mode_name,
        )?;
        self.deps.dry_run_report_sink.report(&info)?;
        Ok(())
    }

    /// payload が巨大な場合は artifacts/events に書き出し、payload は preview + artifact_rel_path に差し替える（P8-4）
    fn normalize_event_payload(
        &self,
        session_dir: &SessionDir,
        run_id: &RunId,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, Error> {
        const EVENTS_ARTIFACTS_DIR: &str = "artifacts/events";
        let serialized = serde_json::to_string(&payload).map_err(|e| Error::json(e.to_string()))?;
        if serialized.len() <= EventEnvelope::RECOMMENDED_MAX_PAYLOAD_BYTES {
            return Ok(payload);
        }
        let ts_ms = self.deps.session.clock.now_ms() as i64;
        let run_id_str: &str = run_id.0.as_str();
        let safe_kind: String = kind
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let filename = format!("{}_{}_{}.json", run_id_str, safe_kind, ts_ms);
        let rel_path = format!("{}/{}", EVENTS_ARTIFACTS_DIR, filename);
        let full_path = session_dir.as_ref().join(&rel_path);
        if let Some(parent) = full_path.parent() {
            self.deps.session.fs.create_dir_all(parent)?;
        }
        self.deps.session.fs.write(&full_path, &serialized)?;
        let preview_len = 300usize;
        let preview = if serialized.len() <= preview_len {
            serialized.clone()
        } else {
            format!("{}...", &serialized[..preview_len.saturating_sub(3)])
        };
        Ok(serde_json::json!({
            "preview": preview,
            "artifact_rel_path": rel_path,
        }))
    }

    /// 重要イベントを events.ndjson に追記（session_dir があるときのみ。fail-closed）。巨大 payload は正規化する（P8-4）。
    fn append_event_to_store(
        &self,
        session_dir: &SessionDir,
        session_id: &SessionId,
        run_id: &RunId,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<(), Error> {
        let payload = self.normalize_event_payload(session_dir, run_id, kind, payload)?;
        let ts_ms = self.deps.session.clock.now_ms() as i64;
        let envelope = EventEnvelopeWithoutSeq {
            v: EventEnvelope::SCHEMA_VERSION,
            ts_ms,
            session_id: session_id.0.clone(),
            run_id: Some(run_id.0.clone()),
            kind: kind.to_string(),
            payload,
        };
        self.deps
            .session
            .event_appender
            .append(session_dir, envelope)
            .map(|_| ())
    }

    /// run 終了時に派生物（index.sqlite / summary.json）を再生成する（session_dir があるとき）
    fn rebuild_derived(&self, session_dir: &SessionDir) -> Result<(), Error> {
        let mut iter = self
            .deps
            .session
            .session_event_store
            .read_all(session_dir)?;
        self.deps
            .session
            .session_derived_builder
            .rebuild(session_dir, &mut iter)
    }

    fn truncate_console_log(&self, session_dir: &SessionDir) -> Result<(), Error> {
        let args = vec![
            "-s".to_string(),
            session_dir.as_path().display().to_string(),
            "truncate_console_log".to_string(),
        ];
        let _ = self
            .deps
            .system
            .process
            .run(std::path::Path::new("aish"), &args);
        Ok(())
    }

    /// エラー終了時に続き用状態を保存する（LLM エラー・クラッシュ時などに resume 可能にする）。
    /// 保存に失敗してもエラーにはせず、呼び出し元の Err をそのまま返す。
    fn try_save_agent_state_on_error(&self, session_dir: &Option<SessionDir>, messages: &[Msg]) {
        if messages.is_empty() || !self.session_is_valid(session_dir) {
            return;
        }
        if let Some(dir) = session_dir {
            if let Err(e) = self.deps.session.agent_state_saver.save(dir, messages) {
                let _ = self.deps.obs.log.log(&LogRecord {
                    ts: now_iso8601(),
                    level: LogLevel::Warn,
                    message: format!("Failed to save agent state on error: {}", e),
                    layer: Some("usecase".to_string()),
                    kind: Some("error".to_string()),
                    fields: None,
                });
            }
        }
    }

    fn run_query_impl(
        &self,
        session_dir: Option<common::domain::SessionDir>,
        provider: Option<common::domain::ProviderName>,
        model: Option<common::domain::ModelName>,
        query: Option<&Query>,
        system_instruction: Option<&str>,
        max_turns_override: Option<usize>,
        tool_allowlist: Option<&[String]>,
        event_hub: Option<EventHubHandle>,
        agent_mode: Option<AgentMode>,
        max_queries_override: Option<usize>,
    ) -> Result<i32, Error> {
        let session_id = session_dir
            .as_ref()
            .map(|d| {
                let p = d.as_ref();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
                SessionId::new(name)
            })
            .unwrap_or_else(|| SessionId::new("global"));
        let sessionless = session_dir.is_none();
        let run_id = RunId::new(format!(
            "run_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let run_start = std::time::Instant::now();
        let query_len = query.as_ref().map(|q| q.len()).unwrap_or(0);
        if let Some(ref hub) = event_hub {
            let mut payload = serde_json::json!({
                "query_len": query_len,
                "non_interactive": self.deps.non_interactive,
            });
            if sessionless {
                payload["sessionless"] = serde_json::json!(true);
            }
            hub.emit(Event {
                v: 1,
                session_id: session_id.clone(),
                run_id: run_id.clone(),
                kind: "run.started".to_string(),
                payload: payload.clone(),
            });
            if let Some(ref dir) = session_dir {
                self.append_event_to_store(dir, &session_id, &run_id, "run.started", payload)?;
            }
        }

        let (profile_name, model_name) = self
            .deps
            .model
            .resolve_profile_and_model
            .resolve(provider.as_ref(), model.as_ref())?;
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("event".to_string(), serde_json::json!("query_started"));
        fields.insert(
            "profile".to_string(),
            serde_json::json!(profile_name.clone()),
        );
        fields.insert("model".to_string(), serde_json::json!(model_name.clone()));
        let _ = self.deps.obs.log.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: format!(
                "query started (profile: {}, model: {})",
                profile_name, model_name
            ),
            layer: Some("usecase".to_string()),
            kind: Some("query".to_string()),
            fields: Some(fields),
        });

        let messages: Vec<Msg> = match query {
            None => {
                // Resume: 保存された続き用状態から再開
                let dir = session_dir.as_ref().ok_or_else(|| {
                    Error::invalid_argument("No continuation state. Please provide a message.")
                })?;
                if !self.session_is_valid(&session_dir) {
                    return Err(Error::invalid_argument(
                        "No continuation state. Please provide a message.",
                    ));
                }
                self.deps
                    .session
                    .agent_state_loader
                    .load(dir)?
                    .ok_or_else(|| {
                        Error::invalid_argument("No continuation state. Please provide a message.")
                    })?
            }
            Some(q) => {
                let (history_messages, query_placement) = if self.session_is_valid(&session_dir) {
                    let dir = session_dir.as_ref().expect("session_dir is Some");
                    self.deps
                        .session
                        .response_saver
                        .save_user(dir, q.as_ref())?;
                    if let Some(ref prep) = self.deps.session.prepare_session_for_sensitive_check {
                        prep.prepare(dir)?;
                    }
                    let loaded = self.deps.session.history_loader.load(dir);
                    match loaded {
                        Ok(history) => (
                            history.messages().to_vec(),
                            QueryPlacement::AlreadyInHistory,
                        ),
                        Err(_) => (Vec::new(), QueryPlacement::AppendAtEnd),
                    }
                } else {
                    (Vec::new(), QueryPlacement::AppendAtEnd)
                };
                let mut pack = match self.deps.session.context_pack_builder.build(
                    &history_messages,
                    Some(q),
                    system_instruction,
                    query_placement,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        let elapsed_ms = run_start.elapsed().as_millis() as u64;
                        if let Some(ref hub) = event_hub {
                            let mut payload = serde_json::json!({
                                "reason": "context_pack_build",
                                "message": e.to_string(),
                                "exit_code": e.exit_code(),
                                "error_kind": format!("{:?}", e),
                                "elapsed_ms": elapsed_ms,
                            });
                            if sessionless {
                                payload["sessionless"] = serde_json::json!(true);
                            }
                            hub.emit(Event {
                                v: 1,
                                session_id: session_id.clone(),
                                run_id: run_id.clone(),
                                kind: "run.failed".to_string(),
                                payload,
                            });
                        }
                        self.try_save_agent_state_on_error(&session_dir, &[]);
                        if let Some(ref dir) = session_dir {
                            let _ = self.rebuild_derived(dir);
                        }
                        return Err(e);
                    }
                };

                // --- egress policy ---
                match self
                    .deps
                    .session
                    .policy_engine
                    .evaluate_egress_context_pack(&pack, self.deps.non_interactive)
                {
                    Ok(PolicyVerdict::Allow { value, decision }) => {
                        pack = value;
                        if let Some(ref hub) = event_hub {
                            let pl = decision.to_event_payload();
                            hub.emit(Event {
                                v: 1,
                                session_id: session_id.clone(),
                                run_id: run_id.clone(),
                                kind: "policy.evaluated".to_string(),
                                payload: pl.clone(),
                            });
                            if let Some(ref dir) = session_dir {
                                self.append_event_to_store(
                                    dir,
                                    &session_id,
                                    &run_id,
                                    "policy.evaluated",
                                    pl,
                                )?;
                            }
                        }
                    }
                    Ok(PolicyVerdict::RequireApproval { decision, .. }) => {
                        let mut block_decision = decision;
                        block_decision.status = "blocked".to_string();
                        block_decision.reason = "egress_require_approval_not_supported".to_string();
                        let elapsed_ms = run_start.elapsed().as_millis() as u64;
                        if let Some(ref hub) = event_hub {
                            let block_pl = block_decision.to_event_payload();
                            hub.emit(Event {
                                v: 1,
                                session_id: session_id.clone(),
                                run_id: run_id.clone(),
                                kind: "policy.evaluated".to_string(),
                                payload: block_pl.clone(),
                            });
                            if let Some(ref dir) = session_dir {
                                self.append_event_to_store(
                                    dir,
                                    &session_id,
                                    &run_id,
                                    "policy.evaluated",
                                    block_pl,
                                )?;
                            }
                            let mut payload = serde_json::json!({
                                "reason": "egress_policy_blocked",
                                "message": format!("Egress blocked: {} ({})", block_decision.reason, block_decision.status),
                                "exit_code": 1,
                                "elapsed_ms": elapsed_ms,
                            });
                            if sessionless {
                                payload["sessionless"] = serde_json::json!(true);
                            }
                            hub.emit(Event {
                                v: 1,
                                session_id: session_id.clone(),
                                run_id: run_id.clone(),
                                kind: "run.failed".to_string(),
                                payload: payload.clone(),
                            });
                            if let Some(ref dir) = session_dir {
                                self.append_event_to_store(
                                    dir,
                                    &session_id,
                                    &run_id,
                                    "run.failed",
                                    payload,
                                )?;
                            }
                        }
                        self.try_save_agent_state_on_error(&session_dir, &[]);
                        if let Some(ref dir) = session_dir {
                            let _ = self.rebuild_derived(dir);
                        }
                        return Err(Error::system(format!(
                            "Context pack blocked by egress policy: {} ({})",
                            block_decision.reason, block_decision.status
                        )));
                    }
                    Ok(PolicyVerdict::Deny { decision }) => {
                        let elapsed_ms = run_start.elapsed().as_millis() as u64;
                        if let Some(ref hub) = event_hub {
                            let pl = decision.to_event_payload();
                            hub.emit(Event {
                                v: 1,
                                session_id: session_id.clone(),
                                run_id: run_id.clone(),
                                kind: "policy.evaluated".to_string(),
                                payload: pl.clone(),
                            });
                            if let Some(ref dir) = session_dir {
                                self.append_event_to_store(
                                    dir,
                                    &session_id,
                                    &run_id,
                                    "policy.evaluated",
                                    pl,
                                )?;
                            }
                            let mut payload = serde_json::json!({
                                "reason": "egress_policy_blocked",
                                "message": format!("Egress blocked: {} ({})", decision.reason, decision.status),
                                "exit_code": 1,
                                "elapsed_ms": elapsed_ms,
                            });
                            if sessionless {
                                payload["sessionless"] = serde_json::json!(true);
                            }
                            hub.emit(Event {
                                v: 1,
                                session_id: session_id.clone(),
                                run_id: run_id.clone(),
                                kind: "run.failed".to_string(),
                                payload: payload.clone(),
                            });
                            if let Some(ref dir) = session_dir {
                                self.append_event_to_store(
                                    dir,
                                    &session_id,
                                    &run_id,
                                    "run.failed",
                                    payload,
                                )?;
                            }
                        }
                        self.try_save_agent_state_on_error(&session_dir, &[]);
                        if let Some(ref dir) = session_dir {
                            let _ = self.rebuild_derived(dir);
                        }
                        return Err(Error::system(format!(
                            "Context pack blocked by egress policy: {} ({})",
                            decision.reason, decision.status
                        )));
                    }
                    Err(e) => {
                        let _ = self.deps.obs.log.log(&LogRecord {
                            ts: now_iso8601(),
                            level: LogLevel::Warn,
                            message: format!("Egress policy evaluation failed: {}", e),
                            layer: Some("usecase".to_string()),
                            kind: Some("policy".to_string()),
                            fields: None,
                        });
                    }
                }

                if let Some(dir) = session_dir.as_ref() {
                    if !pack.attachments.is_empty() {
                        match self.deps.session.artifact_store.store(
                            dir,
                            &run_id,
                            &pack.attachments,
                        ) {
                            Ok(stored) => pack.attachments = stored,
                            Err(e) => {
                                let elapsed_ms = run_start.elapsed().as_millis() as u64;
                                if let Some(ref hub) = event_hub {
                                    let mut payload = serde_json::json!({
                                        "reason": "context_artifact_store",
                                        "message": e.to_string(),
                                        "exit_code": e.exit_code(),
                                        "elapsed_ms": elapsed_ms,
                                    });
                                    if sessionless {
                                        payload["sessionless"] = serde_json::json!(true);
                                    }
                                    hub.emit(Event {
                                        v: 1,
                                        session_id: session_id.clone(),
                                        run_id: run_id.clone(),
                                        kind: "run.failed".to_string(),
                                        payload: payload.clone(),
                                    });
                                    if let Some(ref dir) = session_dir {
                                        self.append_event_to_store(
                                            dir,
                                            &session_id,
                                            &run_id,
                                            "run.failed",
                                            payload,
                                        )?;
                                    }
                                }
                                self.try_save_agent_state_on_error(&session_dir, &[]);
                                if let Some(ref dir) = session_dir {
                                    let _ = self.rebuild_derived(dir);
                                }
                                return Err(e);
                            }
                        }
                    }
                }

                let addons_count = pack
                    .budget_report
                    .decisions
                    .iter()
                    .filter(|d| d.stage == "addon.select" && d.action == "keep")
                    .count();
                if let Some(ref hub) = event_hub {
                    let report = &pack.budget_report;
                    let artifact_refs: Vec<&str> = pack
                        .attachments
                        .iter()
                        .filter_map(|a| a.artifact_rel_path.as_deref())
                        .collect();
                    let pack_payload = serde_json::json!({
                        "budget": {
                            "max_messages": report.budget.max_messages,
                            "max_chars": report.budget.max_chars,
                        },
                        "input": {
                            "message_count": report.input.message_count,
                            "char_count": report.input.char_count,
                        },
                        "output": {
                            "message_count": report.output.message_count,
                            "char_count": report.output.char_count,
                        },
                        "decisions": report.decisions,
                        "addons_count": addons_count,
                        "attachments_count": pack.attachments.len(),
                        "artifact_refs": artifact_refs,
                    });
                    hub.emit(Event {
                        v: 1,
                        session_id: session_id.clone(),
                        run_id: run_id.clone(),
                        kind: "context.pack_built".to_string(),
                        payload: pack_payload.clone(),
                    });
                    if let Some(ref dir) = session_dir {
                        self.append_event_to_store(
                            dir,
                            &session_id,
                            &run_id,
                            "context.pack_built",
                            pack_payload,
                        )?;
                    }
                }

                pack.messages
            }
        };

        let (stream, ctx) = match self.deps.model.llm_stream_factory.create_stream(
            session_dir.as_ref(),
            provider.as_ref(),
            model.as_ref(),
            system_instruction,
        ) {
            Ok(s) => s,
            Err(e) => {
                let elapsed_ms = run_start.elapsed().as_millis() as u64;
                if let Some(ref hub) = event_hub {
                    let mut payload = serde_json::json!({
                        "reason": "create_stream",
                        "message": e.to_string(),
                        "exit_code": e.exit_code(),
                        "error_kind": format!("{:?}", e),
                        "elapsed_ms": elapsed_ms,
                    });
                    if sessionless {
                        payload["sessionless"] = serde_json::json!(true);
                    }
                    hub.emit(Event {
                        v: 1,
                        session_id: session_id.clone(),
                        run_id: run_id.clone(),
                        kind: "run.failed".to_string(),
                        payload: payload.clone(),
                    });
                    if let Some(ref dir) = session_dir {
                        self.append_event_to_store(
                            dir,
                            &session_id,
                            &run_id,
                            "run.failed",
                            payload,
                        )?;
                    }
                }
                self.try_save_agent_state_on_error(&session_dir, &messages);
                if let Some(ref dir) = session_dir {
                    let _ = self.rebuild_derived(dir);
                }
                return Err(e);
            }
        };

        const DEFAULT_MAX_TURNS: usize = 16;
        const DEFAULT_MAX_QUERIES_ACT: usize = 2;
        const DEFAULT_MAX_QUERIES_PLAN: usize = 1;
        let max_turns = max_turns_override.unwrap_or(DEFAULT_MAX_TURNS);
        let max_tool_calls = self
            .deps
            .policy
            .env_resolver
            .ai_max_tool_calls()
            .unwrap_or_else(|| max_turns.saturating_mul(4));
        let agent_mode = agent_mode.unwrap_or(AgentMode::Auto);
        let default_max_queries = match agent_mode {
            AgentMode::Plan => DEFAULT_MAX_QUERIES_PLAN,
            AgentMode::Act | AgentMode::Auto => DEFAULT_MAX_QUERIES_ACT,
        };
        let max_queries = if let Some(override_) = max_queries_override {
            override_
        } else {
            self.deps
                .policy
                .env_resolver
                .ai_max_queries()
                .unwrap_or(default_max_queries)
        };

        let command_rules_path = match self.deps.policy.env_resolver.resolve_command_rules_path() {
            Ok(p) => p,
            Err(e) => {
                let elapsed_ms = run_start.elapsed().as_millis() as u64;
                if let Some(ref hub) = event_hub {
                    let mut payload = serde_json::json!({
                        "reason": "resolve_command_rules_path",
                        "message": e.to_string(),
                        "exit_code": e.exit_code(),
                        "error_kind": format!("{:?}", e),
                        "elapsed_ms": elapsed_ms,
                    });
                    if sessionless {
                        payload["sessionless"] = serde_json::json!(true);
                    }
                    hub.emit(Event {
                        v: 1,
                        session_id: session_id.clone(),
                        run_id: run_id.clone(),
                        kind: "run.failed".to_string(),
                        payload: payload.clone(),
                    });
                    if let Some(ref dir) = session_dir {
                        self.append_event_to_store(
                            dir,
                            &session_id,
                            &run_id,
                            "run.failed",
                            payload,
                        )?;
                    }
                }
                self.try_save_agent_state_on_error(&session_dir, &messages);
                if let Some(ref dir) = session_dir {
                    let _ = self.rebuild_derived(dir);
                }
                return Err(e);
            }
        };
        let mut allow_rules = self
            .deps
            .policy
            .command_allow_rules_loader
            .load_rules(&command_rules_path);
        for prefix in &self.deps.policy.run_shell_allowlist {
            allow_rules.push(CommandAllowRule::Prefix(prefix.clone()));
        }

        let mut messages = messages;
        let ctx = ctx.0;
        let allowlist: Option<std::collections::HashSet<&str>> =
            tool_allowlist.map(|s| s.iter().map(String::as_str).collect());

        loop {
            let tools = self.deps.tooling.tools.clone();
            let sink_factory = Arc::clone(&self.deps.tooling.sink_factory);
            let approver = Arc::clone(&self.deps.policy.approver);
            let policy_engine = Arc::clone(&self.deps.session.policy_engine);
            let interrupt_checker = Arc::clone(&self.deps.policy.interrupt_checker);
            let event_hub_loop = event_hub.clone();
            let session_id_loop = session_id.clone();
            let run_id_loop = run_id.clone();
            let session_dir_loop = session_dir.clone();
            let event_appender = Arc::clone(&self.deps.session.event_appender);
            let clock = Arc::clone(&self.deps.session.clock);
            let artifact_store = if session_dir.is_some() {
                Some(Arc::clone(&self.deps.session.artifact_store))
            } else {
                None
            };

            let mut make_query_loop = || {
                let mut registry = ToolRegistry::new();
                for t in &tools {
                    let name = t.name();
                    if let Some(ref list) = allowlist {
                        if !list.contains(name) {
                            continue;
                        }
                    }
                    registry.register(Arc::clone(t));
                }
                let (memory_project, memory_global) =
                    match self.deps.policy.resolve_memory_dir.resolve() {
                        Ok((p, g)) => (p, Some(g)),
                        Err(_) => (None, None),
                    };
                let tool_context = ToolContext::new(
                    session_dir_loop
                        .as_ref()
                        .map(|s: &SessionDir| s.as_ref().to_path_buf()),
                )
                .with_command_allow_rules(allow_rules.clone())
                .with_memory_dirs(memory_project, memory_global)
                .with_log(Some(Arc::clone(&self.deps.obs.log)))
                .with_event_emitter(
                    event_hub_loop.clone(),
                    Some(session_id_loop.clone()),
                    Some(run_id_loop.clone()),
                );
                let sinks = sink_factory.create_sinks();
                crate::usecase::query_loop::QueryLoop::new(
                    Arc::clone(&stream),
                    registry,
                    tool_context,
                    sinks,
                    Arc::clone(&approver),
                    Arc::clone(&policy_engine),
                    self.deps.non_interactive,
                    Some(Arc::clone(&interrupt_checker)),
                    event_hub_loop.clone(),
                    session_id_loop.clone(),
                    run_id_loop.clone(),
                    session_dir_loop.clone(),
                    Some(Arc::clone(&event_appender)),
                    Some(Arc::clone(&clock)),
                    artifact_store.clone(),
                )
            };

            let judge: Box<dyn AgentJudge> = match agent_mode {
                AgentMode::Plan => Box::new(PlanJudge),
                AgentMode::Act => Box::new(HeuristicJudge::new()),
                AgentMode::Auto => Box::new(CompositeJudge {
                    heuristic: HeuristicJudge::new(),
                    llm: LlmJudge::new(Arc::clone(&self.deps.model.llm_completion)),
                }),
            };

            let outcome = match AgentLoop::run(
                &mut make_query_loop,
                judge.as_ref(),
                &messages,
                AgentLoopConfig {
                    max_queries,
                    max_turns,
                    max_additional_tool_calls: max_tool_calls,
                },
            )
            .map_err(|e| e.with_context(ctx.clone()))
            {
                Ok(o) => o,
                Err(e) => {
                    let elapsed_ms = run_start.elapsed().as_millis() as u64;
                    if let Some(ref hub) = event_hub {
                        let mut payload = serde_json::json!({
                            "reason": "agent_loop",
                            "message": e.to_string(),
                            "exit_code": e.exit_code(),
                            "error_kind": format!("{:?}", e),
                            "elapsed_ms": elapsed_ms,
                        });
                        if sessionless {
                            payload["sessionless"] = serde_json::json!(true);
                        }
                        hub.emit(Event {
                            v: 1,
                            session_id: session_id.clone(),
                            run_id: run_id.clone(),
                            kind: "run.failed".to_string(),
                            payload: payload.clone(),
                        });
                        if let Some(ref dir) = session_dir {
                            self.append_event_to_store(
                                dir,
                                &session_id,
                                &run_id,
                                "run.failed",
                                payload,
                            )?;
                        }
                    }
                    self.try_save_agent_state_on_error(&session_dir, &messages);
                    if let Some(ref dir) = session_dir {
                        let _ = self.rebuild_derived(dir);
                    }
                    return Err(e);
                }
            };

            match outcome {
                AgentLoopOutcome::Done(msgs_done, assistant_text) => {
                    if self.session_is_valid(&session_dir) {
                        let dir = session_dir.as_ref().expect("session_dir is Some");
                        self.deps
                            .session
                            .agent_state_saver
                            .clear_resume_keep_pending(dir)?;
                        if !assistant_text.trim().is_empty() {
                            self.deps
                                .session
                                .response_saver
                                .save_assistant(dir, &assistant_text)?;
                            self.truncate_console_log(dir)?;
                        }
                        if let Ok((mem_proj, mem_global)) =
                            self.deps.policy.resolve_memory_dir.resolve()
                        {
                            let event = LifecycleEvent::QueryEnd {
                                session_dir: dir.clone(),
                                memory_dir_project: mem_proj,
                                memory_dir_global: mem_global,
                                outcome: QueryOutcome::Done,
                                messages: msgs_done.clone(),
                            };
                            if let Err(e) = self.deps.lifecycle_hooks.on_event(&event) {
                                let _ = self.deps.obs.log.log(&LogRecord {
                                    ts: now_iso8601(),
                                    level: LogLevel::Warn,
                                    message: format!("lifecycle hook failed: {}", e),
                                    layer: Some("usecase".to_string()),
                                    kind: Some("lifecycle".to_string()),
                                    fields: None,
                                });
                            }
                        }
                    }
                    let elapsed_ms = run_start.elapsed().as_millis() as u64;
                    if let Some(ref hub) = event_hub {
                        let mut payload = serde_json::json!({
                            "outcome": "done",
                            "exit_code": 0,
                            "elapsed_ms": elapsed_ms,
                        });
                        if sessionless {
                            payload["sessionless"] = serde_json::json!(true);
                        }
                        hub.emit(Event {
                            v: 1,
                            session_id: session_id.clone(),
                            run_id: run_id.clone(),
                            kind: "run.completed".to_string(),
                            payload: payload.clone(),
                        });
                        if let Some(ref dir) = session_dir {
                            self.append_event_to_store(
                                dir,
                                &session_id,
                                &run_id,
                                "run.completed",
                                payload,
                            )?;
                        }
                    }
                    if let Some(ref dir) = session_dir {
                        self.rebuild_derived(dir)?;
                    }
                    let _ = self.deps.obs.log.log(&LogRecord {
                        ts: now_iso8601(),
                        level: LogLevel::Info,
                        message: "query finished".to_string(),
                        layer: Some("usecase".to_string()),
                        kind: Some("usecase".to_string()),
                        fields: None,
                    });
                    return Ok(0);
                }
                AgentLoopOutcome::ReachedLimit(msgs, assistant_text) => {
                    let continue_ = match self.deps.policy.continue_prompt.ask_continue() {
                        Ok(c) => c,
                        Err(e) => {
                            let elapsed_ms = run_start.elapsed().as_millis() as u64;
                            if let Some(ref hub) = event_hub {
                                let mut payload = serde_json::json!({
                                    "reason": "continue_prompt",
                                    "message": e.to_string(),
                                    "exit_code": e.exit_code(),
                                    "error_kind": format!("{:?}", e),
                                    "elapsed_ms": elapsed_ms,
                                });
                                if sessionless {
                                    payload["sessionless"] = serde_json::json!(true);
                                }
                                hub.emit(Event {
                                    v: 1,
                                    session_id: session_id.clone(),
                                    run_id: run_id.clone(),
                                    kind: "run.failed".to_string(),
                                    payload: payload.clone(),
                                });
                                if let Some(ref dir) = session_dir {
                                    self.append_event_to_store(
                                        dir,
                                        &session_id,
                                        &run_id,
                                        "run.failed",
                                        payload,
                                    )?;
                                }
                            }
                            self.try_save_agent_state_on_error(&session_dir, &msgs);
                            if let Some(ref dir) = session_dir {
                                let _ = self.rebuild_derived(dir);
                            }
                            return Err(e);
                        }
                    };
                    if !continue_ {
                        if self.session_is_valid(&session_dir) {
                            let dir = session_dir.as_ref().expect("session_dir is Some");
                            self.deps.session.agent_state_saver.save(dir, &msgs)?;
                            if !assistant_text.trim().is_empty() {
                                self.deps
                                    .session
                                    .response_saver
                                    .save_assistant(dir, &assistant_text)?;
                                self.truncate_console_log(dir)?;
                            }
                            if let Ok((mem_proj, mem_global)) =
                                self.deps.policy.resolve_memory_dir.resolve()
                            {
                                let event = LifecycleEvent::QueryEnd {
                                    session_dir: dir.clone(),
                                    memory_dir_project: mem_proj,
                                    memory_dir_global: mem_global,
                                    outcome: QueryOutcome::ReachedLimit,
                                    messages: msgs.clone(),
                                };
                                if let Err(e) = self.deps.lifecycle_hooks.on_event(&event) {
                                    let _ = self.deps.obs.log.log(&LogRecord {
                                        ts: now_iso8601(),
                                        level: LogLevel::Warn,
                                        message: format!("lifecycle hook failed: {}", e),
                                        layer: Some("usecase".to_string()),
                                        kind: Some("lifecycle".to_string()),
                                        fields: None,
                                    });
                                }
                            }
                        }
                        let elapsed_ms = run_start.elapsed().as_millis() as u64;
                        if let Some(ref hub) = event_hub {
                            let mut payload = serde_json::json!({
                                "outcome": "reached_limit_saved",
                                "exit_code": 0,
                                "elapsed_ms": elapsed_ms,
                            });
                            if sessionless {
                                payload["sessionless"] = serde_json::json!(true);
                            }
                            hub.emit(Event {
                                v: 1,
                                session_id: session_id.clone(),
                                run_id: run_id.clone(),
                                kind: "run.completed".to_string(),
                                payload: payload.clone(),
                            });
                            if let Some(ref dir) = session_dir {
                                self.append_event_to_store(
                                    dir,
                                    &session_id,
                                    &run_id,
                                    "run.completed",
                                    payload,
                                )?;
                            }
                        }
                        if let Some(ref dir) = session_dir {
                            self.rebuild_derived(dir)?;
                        }
                        let _ = self.deps.obs.log.log(&LogRecord {
                            ts: now_iso8601(),
                            level: LogLevel::Info,
                            message: "query finished (saved for resume)".to_string(),
                            layer: Some("usecase".to_string()),
                            kind: Some("usecase".to_string()),
                            fields: None,
                        });
                        return Ok(0);
                    }
                    messages = msgs;
                }
            }
        }
    }
}

impl RunQuery for AiUseCase {
    fn run_query(
        &self,
        session_dir: Option<common::domain::SessionDir>,
        provider: Option<common::domain::ProviderName>,
        model: Option<common::domain::ModelName>,
        query: Option<&Query>,
        system_instruction: Option<&str>,
        max_turns_override: Option<usize>,
        tool_allowlist: Option<&[String]>,
        event_hub: Option<EventHubHandle>,
        agent_mode: Option<AgentMode>,
        max_queries_override: Option<usize>,
    ) -> Result<i32, Error> {
        self.run_query_impl(
            session_dir,
            provider,
            model,
            query,
            system_instruction,
            max_turns_override,
            tool_allowlist,
            event_hub,
            agent_mode,
            max_queries_override,
        )
    }
}
