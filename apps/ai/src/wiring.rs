//! 配線: 標準アダプタで UseCase を組み立てる

use std::path::PathBuf;
use std::sync::Arc;

use common::adapter::{FileJsonLog, NoopLog, StdClock, StdEnvResolver, StdFileSystem, StdProcess};
use common::event_hub::EventHubHandle;
use common::part_id::StdIdGenerator;
use common::ports::outbound::Clock;
use common::ports::outbound::{EnvResolver, FileSystem, Log, McpHost, Process};
use common::tool::EchoTool;
use plugins::StdioJsonRpcMcpBridgeHost;

use crate::adapter::lifecycle::LifecycleHandler;
use crate::adapter::StdPolicyExplainProvider;
use crate::adapter::{
    ChangedFilesSnippetSelector, CliContinuePrompt, CliPolicyOverrides, CliToolApproval,
    CompositeLifecycleHooks, ConfigurableToolProfileProvider, DeterministicCompactionStrategy,
    EgressBudgetHardCapRule, EgressSensitiveRule, FileAgentStateStorage, GetMemoryContentTool,
    GrepHitsSelector, GrepTool, HistoryGetTool, HistorySearchTool, LeakscanPrepareSession,
    LeakscanTextFilter, ManifestReviewedSessionStorage, ManifestTailCompactionViewStrategy,
    MemorySelector, NoContinuePrompt, NonInteractiveToolApproval, NoopInterruptChecker,
    PartSessionStorage, PassThroughReducer, QueueShellSuggestionTool, ReadFileTool,
    ReplaceFileTool, ReviewedTailViewStrategy, SaveMemoryTool, SearchMemoryTool,
    SelfImproveHandler, ShellAllowlistRule, ShellTool, SigintChecker, StdCommandAllowRulesLoader,
    StdConfigExplainProvider, StdConfigProvider, StdContextArtifactStore,
    StdContextPackBuilderWithAddons, StdEventSinkFactory, StdLlmCompletion,
    StdLlmEventStreamFactory, StdPolicyEngine, StdProfileLister, StdResolveMemoryDir,
    StdResolveModeConfig, StdResolveProfileAndModel, StdResolveSystemPromptFromHooks,
    StdSessionDerivedBuilder, StdTaskRunner, StdoutDryRunReportSink, TailWindowReducer,
    ToolModeRule, WriteFileTool,
};
use crate::adapter::{DaemonEventAppender, FallbackEventAppender};
use crate::domain::{ContextBudget, PolicyConfig, Query};
use crate::domain::{PolicyChain, SensitiveAction};
use crate::domain::{PolicyDecision, PolicyExplainExample};
use crate::ports::outbound::{
    AgentStateLoader, AgentStateSaver, ConfigExplainProvider, ConfigProvider, ContextAddonSelector,
    ContextArtifactStore, ContextPackBuilder, DryRunReportSink, EventAppender, LifecycleHooks,
    LlmCompletion, PolicyEngine, PolicyExplainProvider, PrepareSessionForSensitiveCheck,
    ResolveModeConfig, ResolveSystemPromptFromHooks, RunQuery, SessionDerivedBuilder,
    SessionEventStore, SessionHistoryLoader, SessionResponseSaver, TaskRunner, ToolProfileProvider,
};
use crate::usecase::app::{
    AiDeps, AiUseCase, ModelDeps, ObsDeps, PolicyDeps, SessionDeps, SystemDeps, ToolingDeps,
};
use crate::usecase::config_usecase::ConfigUseCase;
use crate::usecase::policy_usecase::PolicyUseCase;
use crate::usecase::session_usecase::SessionUseCase;
use crate::usecase::task::TaskUseCase;
use daemon_api;
use storage::{DerivedApplier, LocalEventAppender, NdjsonSessionEventStore};

/// Arc<AiUseCase> を RunQuery として渡すための薄いラッパ
struct AiRunQuery(Arc<AiUseCase>);
impl RunQuery for AiRunQuery {
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
        query_retry: Option<crate::domain::QueryRetry>,
        max_queries_override: Option<usize>,
    ) -> Result<i32, common::error::Error> {
        self.0.run_query(
            session_dir,
            provider,
            model,
            query,
            system_instruction,
            max_turns_override,
            tool_allowlist,
            event_hub,
            query_retry,
            max_queries_override,
        )
    }
}

/// 配線で組み立てた use case 群（main の Command ディスパッチで利用）
pub struct App {
    pub env_resolver: Arc<dyn EnvResolver>,
    pub fs: Arc<dyn FileSystem>,
    pub task_use_case: TaskUseCase,
    pub run_query: Arc<dyn RunQuery>,
    pub resolve_mode_config: Arc<dyn ResolveModeConfig>,
    /// -S 未指定時にフックからシステムプロンプトを解決する
    pub resolve_system_prompt_from_hooks: Arc<dyn ResolveSystemPromptFromHooks>,
    /// 構造化ログ（ファイルへ JSONL）。エラー時のコンソール表示とは別。
    pub logger: Arc<dyn Log>,
    /// テスト用に露出（Query 実行・session/history の単体テストで利用）
    #[cfg_attr(not(test), allow(dead_code))]
    pub ai_use_case: Arc<AiUseCase>,
    /// policy explain（解決済みポリシー・ルール順・代表例）
    pub policy_use_case: PolicyUseCase,
    /// config explain（解決済み設定と source 一覧）
    pub config_use_case: ConfigUseCase,
    /// セッション派生物の再生成（events.jsonl → index / summary）
    pub session_use_case: SessionUseCase,
}

/// AISH_CONTEXT_STRATEGY / AISH_CONTEXT_MAX_MESSAGES / AISH_CONTEXT_MAX_CHARS から reducer と budget を決定する。
/// 未設定時は tail（TailWindow + 実用 budget）。legacy を指定すると従来の PassThrough + 大きめ budget。
fn context_strategy_from_env() -> (Arc<dyn crate::domain::HistoryReducer>, ContextBudget) {
    let strategy = std::env::var("AISH_CONTEXT_STRATEGY").unwrap_or_else(|_| "tail".into());
    let (reducer, default_budget) = match strategy.to_lowercase().as_str() {
        "legacy" => (
            Arc::new(PassThroughReducer) as Arc<dyn crate::domain::HistoryReducer>,
            ContextBudget::legacy(),
        ),
        _ => (
            Arc::new(TailWindowReducer) as Arc<dyn crate::domain::HistoryReducer>,
            ContextBudget::tail_default(),
        ),
    };
    let max_messages = std::env::var("AISH_CONTEXT_MAX_MESSAGES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default_budget.max_messages);
    let max_chars = std::env::var("AISH_CONTEXT_MAX_CHARS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default_budget.max_chars);
    let budget = ContextBudget {
        max_messages,
        max_chars,
    };
    (reducer, budget)
}

fn history_load_max_from_budget(budget: ContextBudget) -> usize {
    std::env::var("AISH_HISTORY_LOAD_MAX_MESSAGES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or_else(|| {
            let a = budget.max_messages.saturating_add(16);
            let b = budget.max_messages.saturating_mul(2);
            a.max(b)
        })
}

/// leakscan バイナリと rules のパスを解決する。両方存在すれば Some((binary, rules))、でなければ None。
///
/// rules.json: EnvResolver::resolve_leakscan_rules_path() に従う（AISH_HOME 時は $AISH_HOME/config/rules.json、XDG 時は $XDG_CONFIG_HOME/aish/rules.json 等）。
/// leakscan バイナリの検索先: `$AISH_HOME/bin/leakscan` または `<ai バイナリの隣>/leakscan`。
fn resolve_leakscan_paths(
    fs: &Arc<dyn FileSystem>,
    env_resolver: &Arc<dyn EnvResolver>,
) -> Option<(PathBuf, PathBuf)> {
    let rules = env_resolver.resolve_leakscan_rules_path().ok()?;
    if !fs.exists(&rules) {
        return None;
    }
    // 1. $AISH_HOME/bin/leakscan
    if let Ok(home) = env_resolver.resolve_home_dir() {
        let binary = home.as_ref().join("bin").join("leakscan");
        if fs.exists(&binary) {
            return Some((binary, rules));
        }
    }
    // 2. カレント exe の隣に leakscan がある場合（開発時など）
    let current_exe = std::env::current_exe().ok()?;
    let bin_dir = current_exe.parent()?;
    let binary_alt = bin_dir.join("leakscan");
    if fs.exists(&binary_alt) {
        return Some((binary_alt, rules));
    }
    None
}

fn build_session_deps(
    fs: &Arc<dyn FileSystem>,
    env_resolver: &Arc<dyn EnvResolver>,
    interrupt_checker: &Arc<dyn crate::ports::outbound::InterruptChecker>,
    non_interactive: bool,
    config_provider: &Arc<dyn ConfigProvider>,
    project_root: PathBuf,
) -> (
    SessionDeps,
    Arc<dyn PolicyExplainProvider>,
    Arc<dyn ConfigExplainProvider>,
    Vec<String>,
) {
    let id_gen = Arc::new(StdIdGenerator::new(Arc::new(StdClock)));
    let part_storage = Arc::new(PartSessionStorage::new(Arc::clone(fs), id_gen));
    let (reducer, budget) = context_strategy_from_env();
    let history_load_max = history_load_max_from_budget(budget);

    // 履歴ローダは常に ManifestReviewedSessionStorage（manifest.jsonl + reviewed/ のみ参照。part_* は読まない）。
    // leakscan あり: PrepareSessionForSensitiveCheck も有効。leakscan なし: 履歴は空になりうるが、response_saver は part_* に書く。
    let reviewed_storage = Arc::new(ManifestReviewedSessionStorage::with_strategies(
        Arc::clone(fs),
        history_load_max,
        Arc::new(ManifestTailCompactionViewStrategy),
        Arc::new(ReviewedTailViewStrategy),
    ));
    let history_loader: Arc<dyn SessionHistoryLoader> =
        Arc::clone(&reviewed_storage) as Arc<dyn SessionHistoryLoader>;
    let response_saver: Arc<dyn SessionResponseSaver> =
        Arc::clone(&part_storage) as Arc<dyn SessionResponseSaver>;

    let (prepare_session_for_sensitive_check, leakscan_enabled) =
        if let Some((leakscan_binary, rules_path)) = resolve_leakscan_paths(fs, env_resolver) {
            let leakscan_prepare: Arc<dyn PrepareSessionForSensitiveCheck> =
                Arc::new(LeakscanPrepareSession::new(
                    Arc::clone(fs),
                    leakscan_binary,
                    rules_path,
                    Some(Arc::clone(interrupt_checker)),
                    non_interactive,
                    Some(Arc::new(DeterministicCompactionStrategy)),
                ));
            (
                Some(leakscan_prepare) as Option<Arc<dyn PrepareSessionForSensitiveCheck>>,
                true,
            )
        } else {
            (None, false)
        };

    let agent_state_storage = Arc::new(FileAgentStateStorage::new(Arc::clone(fs)));
    let agent_state_saver: Arc<dyn AgentStateSaver> =
        Arc::clone(&agent_state_storage) as Arc<dyn AgentStateSaver>;
    let agent_state_loader: Arc<dyn AgentStateLoader> =
        Arc::clone(&agent_state_storage) as Arc<dyn AgentStateLoader>;

    let resolve_memory_dir = Arc::new(StdResolveMemoryDir::new(Arc::clone(env_resolver)));

    // v0.3: addons = 変更ファイル + memory + grep（env で enable/disable）
    let mut selectors: Vec<Arc<dyn ContextAddonSelector>> = vec![Arc::new(
        ChangedFilesSnippetSelector::new(Arc::clone(fs), 8, 200, 16_000),
    )];
    let memory_enabled = std::env::var("AISH_CONTEXT_ADDONS_MEMORY_ENABLED")
        .map(|v| v != "0")
        .unwrap_or(true);
    if memory_enabled {
        let memory_limit = std::env::var("AISH_CONTEXT_ADDONS_MEMORY_LIMIT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3);
        selectors.push(Arc::new(MemorySelector::new(
            Arc::clone(&resolve_memory_dir) as Arc<dyn crate::ports::outbound::ResolveMemoryDir>,
            memory_limit,
            1200,
        )));
    }
    let grep_enabled = std::env::var("AISH_CONTEXT_ADDONS_GREP_ENABLED")
        .map(|v| v != "0")
        .unwrap_or(true);
    if grep_enabled {
        let grep_max_files = std::env::var("AISH_CONTEXT_ADDONS_GREP_MAX_FILES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(200);
        let grep_max_hits = std::env::var("AISH_CONTEXT_ADDONS_GREP_MAX_HITS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(40);
        let grep_max_bytes = std::env::var("AISH_CONTEXT_ADDONS_GREP_MAX_BYTES_PER_FILE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(200_000u64);
        let grep_token_min_len = std::env::var("AISH_CONTEXT_ADDONS_GREP_TOKEN_MIN_LEN")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3);
        let grep_ignore_dirs = vec![
            ".git".to_string(),
            "target".to_string(),
            "node_modules".to_string(),
            ".aish".to_string(),
            "dist".to_string(),
            "build".to_string(),
        ];
        selectors.push(Arc::new(GrepHitsSelector::new(
            Arc::clone(fs),
            grep_max_files,
            grep_max_hits,
            grep_max_bytes,
            grep_token_min_len,
            grep_ignore_dirs,
        )));
    }

    let addons_max_messages = std::env::var("AISH_CONTEXT_ADDONS_MAX_MESSAGES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let addons_max_chars = std::env::var("AISH_CONTEXT_ADDONS_MAX_CHARS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8_000);
    let addons_budget = ContextBudget {
        max_messages: addons_max_messages,
        max_chars: addons_max_chars,
    };

    let policy_cfg = match config_provider.policy_config() {
        Ok(c) => c,
        // current_dir などの環境依存エラー時は defaults にフォールバックする
        Err(_) => PolicyConfig::defaults(),
    };

    let sensitive_filter: Option<Arc<dyn crate::ports::outbound::SensitiveTextFilter>> =
        if let Some((leakscan_binary, rules_path)) = resolve_leakscan_paths(fs, env_resolver) {
            let action = SensitiveAction::from_str_or_default(
                &policy_cfg.addons_sensitive_action.value,
                non_interactive,
            );
            Some(Arc::new(LeakscanTextFilter::new(
                leakscan_binary,
                rules_path,
                action,
            )))
        } else {
            None
        };

    let context_pack_builder: Arc<dyn ContextPackBuilder> =
        Arc::new(StdContextPackBuilderWithAddons::new(
            reducer,
            budget,
            selectors,
            addons_budget,
            project_root,
            sensitive_filter,
        ));

    let artifact_store: Arc<dyn ContextArtifactStore> =
        Arc::new(StdContextArtifactStore::new(Arc::clone(fs)));

    // egress / run_shell: ConfigProvider の policy_cfg のみから組み立て（env 直読みなし）
    let egress_action = SensitiveAction::from_str_or_default(
        &policy_cfg.egress_sensitive_action.value,
        non_interactive,
    );
    let egress_sensitive_filter: Option<Arc<dyn crate::ports::outbound::SensitiveTextFilter>> =
        if let Some((leakscan_binary, rules_path)) = resolve_leakscan_paths(fs, env_resolver) {
            Some(Arc::new(LeakscanTextFilter::new(
                leakscan_binary,
                rules_path,
                egress_action,
            )))
        } else {
            None
        };

    let hard_cap_chars = policy_cfg.egress_hard_cap_chars.value;
    let policy_cfg = Arc::new(policy_cfg);
    let shell_allowlist = policy_cfg.run_shell_allowlist.value.clone();
    let tool_profiles: Arc<dyn ToolProfileProvider> = Arc::new(
        ConfigurableToolProfileProvider::new(Arc::clone(&policy_cfg), shell_allowlist),
    );

    // PolicyChain: egress 1) hard_cap 2) sensitive, tool 1) shell_allowlist 2) tool_mode
    let egress_rules: Vec<Arc<dyn crate::domain::EgressPolicyRule>> = vec![
        Arc::new(EgressBudgetHardCapRule { hard_cap_chars }),
        Arc::new(EgressSensitiveRule {
            filter: egress_sensitive_filter,
            action: egress_action,
        }),
    ];
    let tool_rules: Vec<Arc<dyn crate::domain::ToolPolicyRule>> = vec![
        Arc::new(ShellAllowlistRule {
            shell_tool_name: "run_shell",
        }),
        Arc::new(ToolModeRule),
    ];
    let chain = PolicyChain {
        egress_rules,
        tool_rules,
    };

    let policy_engine: Arc<dyn PolicyEngine> = Arc::new(StdPolicyEngine::new(
        chain.clone(),
        Arc::clone(&tool_profiles),
    ));

    let resolved = serde_json::to_value(&*policy_cfg).unwrap_or_else(|_| serde_json::json!({}));
    let egress_rule_names: Vec<String> = chain
        .egress_rules
        .iter()
        .map(|r| r.name().to_string())
        .collect();
    let tool_rule_names: Vec<String> = chain
        .tool_rules
        .iter()
        .map(|r| r.name().to_string())
        .collect();
    let examples = vec![
        PolicyExplainExample {
            title: "run_shell allowlisted cmd".to_string(),
            input: serde_json::json!({"tool": "run_shell", "args": {"command": "ls -la"}}),
            outcome: serde_json::json!({"status": "allowed", "reason": "shell_allowlist"}),
            decisions: vec![PolicyDecision {
                v: 1,
                scope: "tool".to_string(),
                subject: "run_shell".to_string(),
                status: "allowed".to_string(),
                reason: "shell_allowlist".to_string(),
                details: serde_json::json!({}),
            }],
        },
        PolicyExplainExample {
            title:
                "run_shell not allowlisted (interactive: RequireApproval, non_interactive: Deny)"
                    .to_string(),
            input: serde_json::json!({"tool": "run_shell", "args": {"command": "rm -rf /"}}),
            outcome: serde_json::json!({"status": "require_approval_or_deny", "reason": "shell_approval_required_or_shell_not_allowlisted_non_interactive"}),
            decisions: vec![PolicyDecision {
                v: 1,
                scope: "tool".to_string(),
                subject: "run_shell".to_string(),
                status: "warn".to_string(),
                reason: "shell_approval_required".to_string(),
                details: serde_json::json!({}),
            }],
        },
        PolicyExplainExample {
            title: "egress sensitive (SECRET) mask or deny".to_string(),
            input: serde_json::json!({"scope": "egress", "text_contains": "SECRET"}),
            outcome: serde_json::json!({"status": "mask_or_deny", "reason": "sensitive_masked_or_sensitive_deny"}),
            decisions: vec![PolicyDecision {
                v: 1,
                scope: "egress".to_string(),
                subject: "context_pack".to_string(),
                status: "warn".to_string(),
                reason: "sensitive_masked".to_string(),
                details: serde_json::json!({}),
            }],
        },
    ];
    let policy_explain_provider: Arc<dyn PolicyExplainProvider> = Arc::new(
        StdPolicyExplainProvider::new(resolved, egress_rule_names, tool_rule_names, examples),
    );

    let config_explain_provider: Arc<dyn ConfigExplainProvider> =
        Arc::new(StdConfigExplainProvider::new(Arc::clone(config_provider)));

    let clock: Arc<dyn Clock> = Arc::new(StdClock);
    let session_event_store: Arc<dyn SessionEventStore> =
        Arc::new(NdjsonSessionEventStore::new(Arc::clone(&fs)));
    let derived_applier: Arc<DerivedApplier> = Arc::new(storage::DerivedApplier::new(
        Arc::clone(&session_event_store),
        Arc::clone(&fs),
    ));
    let session_derived_builder: Arc<dyn SessionDerivedBuilder> =
        Arc::new(StdSessionDerivedBuilder::new(derived_applier));
    let local_appender: Arc<dyn EventAppender> =
        Arc::new(LocalEventAppender::new(Arc::clone(&session_event_store)));
    let event_appender: Arc<dyn EventAppender> = {
        let mode = std::env::var("AISH_DAEMON").unwrap_or_else(|_| "off".to_string());
        let socket_path = daemon_api::default_socket_path();
        match mode.as_str() {
            "on" => Arc::new(DaemonEventAppender::new(socket_path)),
            "auto" => Arc::new(FallbackEventAppender::new(
                socket_path,
                Arc::clone(&local_appender),
            )),
            _ => local_appender,
        }
    };

    (
        SessionDeps {
            fs: Arc::clone(fs),
            history_loader,
            context_pack_builder,
            artifact_store,
            policy_engine,
            response_saver,
            agent_state_saver,
            agent_state_loader,
            prepare_session_for_sensitive_check,
            leakscan_enabled,
            session_event_store,
            session_derived_builder,
            clock,
            event_appender,
        },
        policy_explain_provider,
        config_explain_provider,
        policy_cfg.run_shell_allowlist.value.clone(),
    )
}

fn build_policy_deps(
    env_resolver: &Arc<dyn EnvResolver>,
    interrupt_checker: &Arc<dyn crate::ports::outbound::InterruptChecker>,
    non_interactive: bool,
    run_shell_allowlist: Vec<String>,
) -> PolicyDeps {
    let command_allow_rules_loader = Arc::new(StdCommandAllowRulesLoader);
    let approver: Arc<dyn crate::ports::outbound::ToolApproval> = if non_interactive {
        Arc::new(NonInteractiveToolApproval::new())
    } else {
        Arc::new(CliToolApproval::new(Some(Arc::clone(interrupt_checker))))
    };
    let continue_prompt: Arc<dyn crate::ports::outbound::ContinueAfterLimitPrompt> =
        if non_interactive {
            Arc::new(NoContinuePrompt::new())
        } else {
            Arc::new(CliContinuePrompt::new())
        };

    PolicyDeps {
        continue_prompt,
        env_resolver: Arc::clone(env_resolver),
        resolve_memory_dir: Arc::new(StdResolveMemoryDir::new(Arc::clone(env_resolver))),
        command_allow_rules_loader,
        run_shell_allowlist,
        approver,
        interrupt_checker: Arc::clone(interrupt_checker),
    }
}

/// OpenAI API の tools[].function.name は ^[a-zA-Z0-9_-]+$ のみ許可。
/// canonical id（例: namespace.tool_name）のドット等をアンダースコアに置換する。
fn sanitize_openai_tool_name(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn build_tooling_deps(
    verbose: bool,
    fs: &Arc<dyn FileSystem>,
    env_resolver: &Arc<dyn EnvResolver>,
    event_hub: Option<EventHubHandle>,
) -> ToolingDeps {
    let sink_factory = Arc::new(StdEventSinkFactory::new(verbose));
    let mut tools: Vec<Arc<dyn common::tool::Tool>> = vec![
        Arc::new(EchoTool::new()),
        Arc::new(ShellTool::new()),
        Arc::new(QueueShellSuggestionTool::new()),
        Arc::new(ReadFileTool::new()),
        Arc::new(WriteFileTool::new()),
        Arc::new(ReplaceFileTool::new()),
        Arc::new(GrepTool::new()),
        Arc::new(HistoryGetTool::new()),
        Arc::new(HistorySearchTool::new()),
        Arc::new(SaveMemoryTool::new()),
        Arc::new(SearchMemoryTool::new()),
        Arc::new(GetMemoryContentTool::new()),
    ];
    // 現状は host 側で std::env/std::fs を使う（段階移行）。未使用引数の警告回避。
    let _ = (fs, env_resolver, event_hub);

    // Phase9: 外部拡張の唯一の入口を McpHost に集約（stdio JSON-RPC ブリッジ）
    // OpenAI の tools[].function.name は ^[a-zA-Z0-9_-]+$ のみ許可。canonical id (例: namespace.tool) をサニタイズする。
    let host: Arc<dyn McpHost> = Arc::new(StdioJsonRpcMcpBridgeHost::new());
    if let Ok(servers) = host.discover() {
        for s in servers.into_iter().filter(|s| s.enabled) {
            if let Ok(tool_descs) = host.list_tools(&s.id) {
                for td in tool_descs {
                    let sanitized = sanitize_openai_tool_name(&td.id.0);
                    let name_static: &'static str = Box::leak(sanitized.into_boxed_str());
                    let desc_static: &'static str =
                        Box::leak(format!("[external] {}", td.display_name).into_boxed_str());
                    let proxy = crate::adapter::McpToolProxy::new(
                        name_static,
                        desc_static,
                        td.schema.clone(),
                        td.id.clone(),
                        Arc::clone(&host),
                    );
                    tools.push(Arc::new(proxy));
                }
            }
        }
    }

    ToolingDeps {
        sink_factory,
        tools,
    }
}

fn build_model_deps(fs: &Arc<dyn FileSystem>, env_resolver: &Arc<dyn EnvResolver>) -> ModelDeps {
    let profile_lister: Arc<dyn crate::ports::outbound::ProfileLister> = Arc::new(
        StdProfileLister::new(Arc::clone(fs), Arc::clone(env_resolver)),
    );
    let resolve_profile_and_model: Arc<dyn crate::ports::outbound::ResolveProfileAndModel> =
        Arc::new(StdResolveProfileAndModel::new(
            Arc::clone(fs),
            Arc::clone(env_resolver),
        ));
    let llm_stream_factory: Arc<dyn crate::ports::outbound::LlmEventStreamFactory> = Arc::new(
        StdLlmEventStreamFactory::new(Arc::clone(fs), Arc::clone(env_resolver)),
    );

    let llm_completion: Arc<dyn crate::ports::outbound::LlmCompletion> =
        Arc::new(StdLlmCompletion::new(Arc::clone(&llm_stream_factory)));

    ModelDeps {
        profile_lister,
        resolve_profile_and_model,
        llm_stream_factory,
        llm_completion,
    }
}

fn build_system_deps(process: &Arc<dyn Process>) -> SystemDeps {
    SystemDeps {
        process: Arc::clone(process),
    }
}

fn build_task_runner(fs: &Arc<dyn FileSystem>, process: &Arc<dyn Process>) -> Arc<dyn TaskRunner> {
    Arc::new(StdTaskRunner::new(Arc::clone(fs), Arc::clone(process)))
}

fn build_obs_deps(logger: &Arc<dyn Log>) -> ObsDeps {
    ObsDeps {
        log: Arc::clone(logger),
    }
}

/// ライフサイクルフックを組み立てる。AISH_SELF_IMPROVE=0 または false のときは自己改善ハンドラを登録しない。
fn build_lifecycle_hooks(
    llm_stream_factory: &Arc<dyn crate::ports::outbound::LlmEventStreamFactory>,
    logger: &Arc<dyn Log>,
) -> Arc<dyn LifecycleHooks> {
    let disabled = std::env::var("AISH_SELF_IMPROVE")
        .map(|v| v == "0" || v.eq_ignore_ascii_case("false"))
        .unwrap_or(false);
    let handlers: Vec<Arc<dyn LifecycleHandler>> = if disabled {
        vec![]
    } else {
        let llm_completion: Arc<dyn LlmCompletion> =
            Arc::new(StdLlmCompletion::new(Arc::clone(llm_stream_factory)));
        vec![Arc::new(SelfImproveHandler::new(
            llm_completion,
            Arc::clone(logger),
        ))]
    };
    Arc::new(CompositeLifecycleHooks::new(handlers))
}

fn build_resolve_mode_config(
    env_resolver: &Arc<dyn EnvResolver>,
    fs: &Arc<dyn FileSystem>,
) -> Arc<dyn ResolveModeConfig> {
    Arc::new(StdResolveModeConfig::new(
        Arc::clone(env_resolver),
        Arc::clone(fs),
    ))
}

/// 配線: 標準アダプタで AiUseCase / TaskUseCase を組み立て、App を返す。
///
/// `non_interactive`: true のとき確認プロンプトを出さない（ツール承認は常に拒否・続行はしない・leakscan ヒットは拒否）。CI 向け。
/// `verbose`: true のとき不具合調査用の冗長ログを stderr 等に出力する。
pub fn wire_ai(non_interactive: bool, verbose: bool) -> App {
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let env_resolver: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
    let logger: Arc<dyn Log> = env_resolver
        .resolve_log_file_path()
        .map(|path| Arc::new(FileJsonLog::new(Arc::clone(&fs), path)) as Arc<dyn Log>)
        .unwrap_or_else(|_| Arc::new(NoopLog));
    let interrupt_checker: Arc<dyn crate::ports::outbound::InterruptChecker> = SigintChecker::new()
        .map(Arc::new)
        .map(|a| a as Arc<dyn crate::ports::outbound::InterruptChecker>)
        .unwrap_or_else(|_| Arc::new(NoopInterruptChecker::new()));
    let process: Arc<dyn Process> = Arc::new(StdProcess);
    let project_root = env_resolver
        .current_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let cli_overrides = CliPolicyOverrides::default();
    let raw_config_provider = StdConfigProvider::new(
        Arc::clone(&env_resolver),
        Arc::clone(&fs),
        project_root.clone(),
        cli_overrides,
    );
    let config_provider: Arc<dyn ConfigProvider> = Arc::new(raw_config_provider);
    let (session, policy_explain_provider, config_explain_provider, run_shell_allowlist) =
        build_session_deps(
            &fs,
            &env_resolver,
            &interrupt_checker,
            non_interactive,
            &config_provider,
            project_root,
        );
    let session_event_store = session.session_event_store.clone();
    let session_derived_builder = session.session_derived_builder.clone();
    let policy = build_policy_deps(
        &env_resolver,
        &interrupt_checker,
        non_interactive,
        run_shell_allowlist,
    );
    let tooling = build_tooling_deps(verbose, &fs, &env_resolver, None);
    let model = build_model_deps(&fs, &env_resolver);
    let system = build_system_deps(&process);
    let obs = build_obs_deps(&logger);
    let lifecycle_hooks = build_lifecycle_hooks(&model.llm_stream_factory, &logger);

    let dry_run_report_sink: Arc<dyn DryRunReportSink> = Arc::new(StdoutDryRunReportSink::new());
    let ai_use_case = Arc::new(AiUseCase::new(AiDeps {
        session,
        policy,
        tooling,
        model,
        system,
        obs,
        lifecycle_hooks,
        dry_run_report_sink,
        non_interactive,
    }));
    let run_query: Arc<dyn RunQuery> = Arc::new(AiRunQuery(Arc::clone(&ai_use_case)));
    let task_runner: Arc<dyn TaskRunner> = build_task_runner(&fs, &process);
    let resolve_mode_config = build_resolve_mode_config(&env_resolver, &fs);
    let resolve_system_prompt_from_hooks: Arc<dyn ResolveSystemPromptFromHooks> = Arc::new(
        StdResolveSystemPromptFromHooks::new(Arc::clone(&env_resolver), Arc::clone(&fs)),
    );
    let task_use_case = TaskUseCase::new(task_runner, Arc::clone(&run_query));
    let policy_use_case = PolicyUseCase::new(policy_explain_provider);
    let config_use_case = ConfigUseCase::new(config_explain_provider);
    let session_use_case = SessionUseCase::new(session_event_store, session_derived_builder);
    App {
        env_resolver,
        fs,
        task_use_case,
        run_query,
        resolve_mode_config,
        resolve_system_prompt_from_hooks,
        logger,
        ai_use_case,
        policy_use_case,
        config_use_case,
        session_use_case,
    }
}

#[cfg(test)]
mod tests {
    use super::{context_strategy_from_env, sanitize_openai_tool_name};
    use common::llm::provider::Message as LlmMessage;
    use std::env;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_sanitize_openai_tool_name() {
        assert_eq!(
            sanitize_openai_tool_name("acme.read_file"),
            "acme_read_file"
        );
        assert_eq!(sanitize_openai_tool_name("run_shell"), "run_shell");
        assert_eq!(sanitize_openai_tool_name("a-b_c"), "a-b_c");
        assert_eq!(
            sanitize_openai_tool_name("ns.tool.with.dots"),
            "ns_tool_with_dots"
        );
    }

    #[test]
    fn test_context_strategy_tail_with_overrides() {
        let _guard = env_lock().lock().expect("lock poisoned");
        let old_strategy = env::var("AISH_CONTEXT_STRATEGY").ok();
        let old_max_messages = env::var("AISH_CONTEXT_MAX_MESSAGES").ok();
        let old_max_chars = env::var("AISH_CONTEXT_MAX_CHARS").ok();

        env::set_var("AISH_CONTEXT_STRATEGY", "tail");
        env::set_var("AISH_CONTEXT_MAX_MESSAGES", "2");
        env::set_var("AISH_CONTEXT_MAX_CHARS", "100");

        let (reducer, budget) = context_strategy_from_env();
        assert_eq!(budget.max_messages, 2);
        assert_eq!(budget.max_chars, 100);

        let messages = vec![
            LlmMessage::user("a"),
            LlmMessage::user("b"),
            LlmMessage::user("c"),
        ];
        let reduced = reducer.reduce(&messages, budget);
        assert_eq!(reduced.len(), 2);
        assert_eq!(reduced[0].content, "b");
        assert_eq!(reduced[1].content, "c");

        if let Some(v) = old_strategy {
            env::set_var("AISH_CONTEXT_STRATEGY", v);
        } else {
            env::remove_var("AISH_CONTEXT_STRATEGY");
        }
        if let Some(v) = old_max_messages {
            env::set_var("AISH_CONTEXT_MAX_MESSAGES", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_MESSAGES");
        }
        if let Some(v) = old_max_chars {
            env::set_var("AISH_CONTEXT_MAX_CHARS", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_CHARS");
        }
    }

    #[test]
    fn test_context_strategy_unknown_falls_back_to_tail() {
        let _guard = env_lock().lock().expect("lock poisoned");
        let old_strategy = env::var("AISH_CONTEXT_STRATEGY").ok();
        let old_max_messages = env::var("AISH_CONTEXT_MAX_MESSAGES").ok();
        let old_max_chars = env::var("AISH_CONTEXT_MAX_CHARS").ok();

        env::set_var("AISH_CONTEXT_STRATEGY", "unknown");
        env::remove_var("AISH_CONTEXT_MAX_MESSAGES");
        env::remove_var("AISH_CONTEXT_MAX_CHARS");

        let (reducer, budget) = context_strategy_from_env();
        assert_eq!(budget.max_messages, 40);
        assert_eq!(budget.max_chars, 40_000);

        let messages = vec![
            LlmMessage::user("a"),
            LlmMessage::user("b"),
            LlmMessage::user("c"),
        ];
        let reduced = reducer.reduce(&messages, budget);
        assert_eq!(reduced.len(), 3);

        if let Some(v) = old_strategy {
            env::set_var("AISH_CONTEXT_STRATEGY", v);
        } else {
            env::remove_var("AISH_CONTEXT_STRATEGY");
        }
        if let Some(v) = old_max_messages {
            env::set_var("AISH_CONTEXT_MAX_MESSAGES", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_MESSAGES");
        }
        if let Some(v) = old_max_chars {
            env::set_var("AISH_CONTEXT_MAX_CHARS", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_CHARS");
        }
    }

    #[test]
    fn test_context_strategy_legacy_uses_legacy_budget() {
        let _guard = env_lock().lock().expect("lock poisoned");
        let old_strategy = env::var("AISH_CONTEXT_STRATEGY").ok();
        let old_max_messages = env::var("AISH_CONTEXT_MAX_MESSAGES").ok();
        let old_max_chars = env::var("AISH_CONTEXT_MAX_CHARS").ok();

        env::set_var("AISH_CONTEXT_STRATEGY", "legacy");
        env::remove_var("AISH_CONTEXT_MAX_MESSAGES");
        env::remove_var("AISH_CONTEXT_MAX_CHARS");

        let (reducer, budget) = context_strategy_from_env();
        assert_eq!(budget.max_messages, 10_000);
        assert_eq!(budget.max_chars, 10_000_000);

        let messages = vec![
            LlmMessage::user("a"),
            LlmMessage::user("b"),
            LlmMessage::user("c"),
        ];
        let reduced = reducer.reduce(&messages, budget);
        assert_eq!(reduced.len(), 3);

        if let Some(v) = old_strategy {
            env::set_var("AISH_CONTEXT_STRATEGY", v);
        } else {
            env::remove_var("AISH_CONTEXT_STRATEGY");
        }
        if let Some(v) = old_max_messages {
            env::set_var("AISH_CONTEXT_MAX_MESSAGES", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_MESSAGES");
        }
        if let Some(v) = old_max_chars {
            env::set_var("AISH_CONTEXT_MAX_CHARS", v);
        } else {
            env::remove_var("AISH_CONTEXT_MAX_CHARS");
        }
    }
}
