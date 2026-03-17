//! ai backend 実行コア: daemon worker から受けた ai 実行要求を処理する。

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use common::error::Error;
use common::ports::outbound::{ProcessOutputObserver, ProcessOutputStream};
use common::sink::{AgentEvent, EventSink};
use daemon_api::{AiJobState, AiRunRequest, AiStreamFrame, OutputStream, SensitiveChoiceValue};

use crate::adapter::dry_run_report_sink::render_dry_run_report;
use crate::adapter::SensitivePromptChoice;
use crate::cli::{config_to_command, parse_args_from_os, ParseOutcome};
use crate::domain::AiCommand;
use crate::ports::outbound::{
    Approval, ContinueAfterLimitPrompt, DryRunReportSink, EventSinkFactory, ToolApproval,
};
use crate::wiring::wire_ai_with_overrides;

pub const DEFAULT_MAX_BACKEND_JOB_DEPTH: u32 = 8;

pub trait BackendFrameEmitter: Send + Sync {
    fn emit(&self, frame: AiStreamFrame) -> Result<(), Error>;
}

pub trait BackendApprovalHandler: Send + Sync {
    fn request_approval(&self, prompt: &str) -> Result<bool, Error>;
    fn request_continue(&self, prompt: &str) -> Result<bool, Error>;
    fn request_sensitive_choice(
        &self,
        verbose_output: &str,
    ) -> Result<SensitiveChoiceValue, Error>;
}

pub fn emit_lifecycle(
    emitter: &Arc<dyn BackendFrameEmitter>,
    request: &AiRunRequest,
    state: AiJobState,
) -> Result<(), Error> {
    emitter.emit(AiStreamFrame::Lifecycle {
        job_id: request.job_id.clone(),
        parent_job_id: request.parent_job_id.clone(),
        state,
    })
}

fn request_is_verbose(request: &AiRunRequest) -> bool {
    request
        .argv
        .iter()
        .any(|arg| arg == "-v" || arg == "--verbose")
}

fn format_lifecycle_text(request: &AiRunRequest, state: AiJobState) -> String {
    let state = match state {
        AiJobState::Queued => "queued",
        AiJobState::Running => "running",
        AiJobState::WaitingInteraction => "waiting_interaction",
        AiJobState::Cancelled => "cancelled",
        AiJobState::Completed => "completed",
        AiJobState::Failed => "failed",
    };
    match request.parent_job_id.as_deref() {
        Some(parent_job_id) => format!(
            "[backend job] state={} job_id={} parent_job_id={}\n",
            state, request.job_id, parent_job_id
        ),
        None => format!("[backend job] state={} job_id={}\n", state, request.job_id),
    }
}

pub fn emit_lifecycle_text(
    emitter: &Arc<dyn BackendFrameEmitter>,
    request: &AiRunRequest,
    state: AiJobState,
) -> Result<(), Error> {
    if !request_is_verbose(request) {
        return Ok(());
    }
    emitter.emit(AiStreamFrame::TextOutput {
        job_id: request.job_id.clone(),
        stream: OutputStream::Stderr,
        text: format_lifecycle_text(request, state),
    })
}

fn process_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct ProcessContextGuard {
    _guard: MutexGuard<'static, ()>,
    prev_cwd: PathBuf,
    prev_aish_home: Option<String>,
    prev_aish_session: Option<String>,
    prev_aish_job_id: Option<String>,
    prev_aish_job_depth: Option<String>,
    prev_aish_max_job_depth: Option<String>,
    prev_aish_frontend_tty: Option<String>,
}

impl ProcessContextGuard {
    fn apply(request: &AiRunRequest) -> Result<Self, Error> {
        let guard = process_lock()
            .lock()
            .map_err(|_| Error::system("backend process lock poisoned"))?;
        let prev_cwd = std::env::current_dir().map_err(|e| Error::io_msg(e.to_string()))?;
        let prev_aish_home = std::env::var("AISH_HOME").ok();
        let prev_aish_session = std::env::var("AISH_SESSION").ok();
        let prev_aish_job_id = std::env::var("AISH_JOB_ID").ok();
        let prev_aish_job_depth = std::env::var("AISH_JOB_DEPTH").ok();
        let prev_aish_max_job_depth = std::env::var("AISH_MAX_BACKEND_JOB_DEPTH").ok();
        let prev_aish_frontend_tty = std::env::var("AISH_FRONTEND_TTY").ok();

        if let Some(ref cwd) = request.cwd {
            std::env::set_current_dir(cwd).map_err(|e| Error::io_msg(e.to_string()))?;
        }

        match &request.aish_home {
            Some(v) => std::env::set_var("AISH_HOME", v),
            None => std::env::remove_var("AISH_HOME"),
        }
        match &request.session_dir {
            Some(v) => std::env::set_var("AISH_SESSION", v),
            None => std::env::remove_var("AISH_SESSION"),
        }
        std::env::set_var("AISH_JOB_ID", &request.job_id);
        std::env::set_var("AISH_JOB_DEPTH", (request.nesting_depth + 1).to_string());
        match request.max_nesting_depth {
            Some(value) => std::env::set_var("AISH_MAX_BACKEND_JOB_DEPTH", value.to_string()),
            None => std::env::remove_var("AISH_MAX_BACKEND_JOB_DEPTH"),
        }
        match &request.frontend_tty {
            Some(value) => std::env::set_var("AISH_FRONTEND_TTY", value),
            None => std::env::remove_var("AISH_FRONTEND_TTY"),
        }

        Ok(Self {
            _guard: guard,
            prev_cwd,
            prev_aish_home,
            prev_aish_session,
            prev_aish_job_id,
            prev_aish_job_depth,
            prev_aish_max_job_depth,
            prev_aish_frontend_tty,
        })
    }
}

impl Drop for ProcessContextGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev_cwd);
        match &self.prev_aish_home {
            Some(v) => std::env::set_var("AISH_HOME", v),
            None => std::env::remove_var("AISH_HOME"),
        }
        match &self.prev_aish_session {
            Some(v) => std::env::set_var("AISH_SESSION", v),
            None => std::env::remove_var("AISH_SESSION"),
        }
        match &self.prev_aish_job_id {
            Some(v) => std::env::set_var("AISH_JOB_ID", v),
            None => std::env::remove_var("AISH_JOB_ID"),
        }
        match &self.prev_aish_job_depth {
            Some(v) => std::env::set_var("AISH_JOB_DEPTH", v),
            None => std::env::remove_var("AISH_JOB_DEPTH"),
        }
        match &self.prev_aish_max_job_depth {
            Some(v) => std::env::set_var("AISH_MAX_BACKEND_JOB_DEPTH", v),
            None => std::env::remove_var("AISH_MAX_BACKEND_JOB_DEPTH"),
        }
        match &self.prev_aish_frontend_tty {
            Some(v) => std::env::set_var("AISH_FRONTEND_TTY", v),
            None => std::env::remove_var("AISH_FRONTEND_TTY"),
        }
    }
}

pub fn max_backend_job_depth(request: &AiRunRequest) -> u32 {
    request
        .max_nesting_depth
        .unwrap_or(DEFAULT_MAX_BACKEND_JOB_DEPTH)
}

struct StreamingEventSinkFactory {
    emitter: Arc<dyn BackendFrameEmitter>,
    job_id: String,
}

impl StreamingEventSinkFactory {
    fn new(emitter: Arc<dyn BackendFrameEmitter>, job_id: String) -> Self {
        Self { emitter, job_id }
    }
}

impl EventSinkFactory for StreamingEventSinkFactory {
    fn create_sinks(&self) -> Vec<Box<dyn EventSink>> {
        vec![Box::new(StreamingEventSink {
            emitter: Arc::clone(&self.emitter),
            job_id: self.job_id.clone(),
        })]
    }
}

struct StreamingEventSink {
    emitter: Arc<dyn BackendFrameEmitter>,
    job_id: String,
}

impl EventSink for StreamingEventSink {
    fn on_event(&mut self, ev: &AgentEvent) -> Result<(), Error> {
        self.emitter.emit(AiStreamFrame::Event {
            job_id: self.job_id.clone(),
            event: ev.clone(),
        })
    }
}

struct StreamingDryRunReportSink {
    emitter: Arc<dyn BackendFrameEmitter>,
    job_id: String,
}

struct StreamingToolApproval {
    handler: Arc<dyn BackendApprovalHandler>,
}

impl StreamingToolApproval {
    fn new(handler: Arc<dyn BackendApprovalHandler>) -> Self {
        Self { handler }
    }
}

struct StreamingContinuePrompt {
    handler: Arc<dyn BackendApprovalHandler>,
}

impl StreamingContinuePrompt {
    fn new(handler: Arc<dyn BackendApprovalHandler>) -> Self {
        Self { handler }
    }
}

impl ContinueAfterLimitPrompt for StreamingContinuePrompt {
    fn ask_continue(&self) -> Result<bool, Error> {
        self.handler
            .request_continue("Query loop reached the turn limit. Continue? [y/N]: ")
    }
}

struct StreamingSensitivePrompt {
    handler: Arc<dyn BackendApprovalHandler>,
}

impl StreamingSensitivePrompt {
    fn new(handler: Arc<dyn BackendApprovalHandler>) -> Self {
        Self { handler }
    }
}

impl crate::adapter::SensitiveContentPrompt for StreamingSensitivePrompt {
    fn choose(&self, verbose_output: &str) -> Result<SensitivePromptChoice, Error> {
        Ok(
            match self.handler.request_sensitive_choice(verbose_output)? {
                SensitiveChoiceValue::Allow => SensitivePromptChoice::Allow,
                SensitiveChoiceValue::Deny => SensitivePromptChoice::Deny,
                SensitiveChoiceValue::Mask => SensitivePromptChoice::Mask,
            },
        )
    }
}

impl ToolApproval for StreamingToolApproval {
    fn approve_unsafe_shell(&self, command: &str) -> Result<Approval, Error> {
        if self.handler.request_approval(command)? {
            Ok(Approval::Approved)
        } else {
            Ok(Approval::Denied)
        }
    }
}

impl StreamingDryRunReportSink {
    fn new(emitter: Arc<dyn BackendFrameEmitter>, job_id: String) -> Self {
        Self { emitter, job_id }
    }
}

impl DryRunReportSink for StreamingDryRunReportSink {
    fn report(&self, info: &crate::domain::DryRunInfo) -> Result<(), Error> {
        self.emitter.emit(AiStreamFrame::TextOutput {
            job_id: self.job_id.clone(),
            stream: OutputStream::Stdout,
            text: render_dry_run_report(info),
        })
    }
}

struct StreamingProcessOutputObserver {
    emitter: Arc<dyn BackendFrameEmitter>,
    job_id: String,
}

impl StreamingProcessOutputObserver {
    fn new(emitter: Arc<dyn BackendFrameEmitter>, job_id: String) -> Self {
        Self { emitter, job_id }
    }
}

impl ProcessOutputObserver for StreamingProcessOutputObserver {
    fn on_output(&self, stream: ProcessOutputStream, text: &str) -> Result<(), Error> {
        let stream = match stream {
            ProcessOutputStream::Stdout => OutputStream::Stdout,
            ProcessOutputStream::Stderr => OutputStream::Stderr,
        };
        self.emitter.emit(AiStreamFrame::TextOutput {
            job_id: self.job_id.clone(),
            stream,
            text: text.to_string(),
        })
    }
}

fn os_argv(argv: &[String]) -> Vec<OsString> {
    argv.iter().cloned().map(OsString::from).collect()
}

fn emit_stdout(
    emitter: &Arc<dyn BackendFrameEmitter>,
    job_id: &str,
    text: String,
) -> Result<(), Error> {
    emitter.emit(AiStreamFrame::TextOutput {
        job_id: job_id.to_string(),
        stream: OutputStream::Stdout,
        text,
    })
}

fn command_supported(cmd: &AiCommand) -> bool {
    !matches!(cmd, AiCommand::Help)
}

pub fn run_request(
    request: AiRunRequest,
    emitter: Arc<dyn BackendFrameEmitter>,
    approval_handler: Arc<dyn BackendApprovalHandler>,
) -> Result<i32, Error> {
    let max_depth = max_backend_job_depth(&request);
    if request.nesting_depth > max_depth {
        return Err(Error::invalid_argument(format!(
            "nested ai depth {} exceeds limit {}",
            request.nesting_depth, max_depth
        )));
    }
    let _context = ProcessContextGuard::apply(&request)?;
    let job_id = request.job_id.clone();
    let argv = if request.argv.is_empty() {
        vec![OsString::from("ai")]
    } else {
        os_argv(&request.argv)
    };
    let outcome = parse_args_from_os(argv)?;
    let config = match outcome {
        ParseOutcome::Config(c) => c,
        _ => {
            return Err(Error::invalid_argument(
                "backend run_ai only supports ai config commands",
            ))
        }
    };
    let cmd = config_to_command(config.clone());
    if !command_supported(&cmd) {
        return Err(Error::invalid_argument(
            "backend run_ai only supports ai config commands",
        ));
    }
    emit_lifecycle(&emitter, &request, AiJobState::Running)?;
    emit_lifecycle_text(&emitter, &request, AiJobState::Running)?;

    if !matches!(
        cmd,
        AiCommand::Task { .. } | AiCommand::Resume { .. } | AiCommand::Query { .. }
    ) {
        let app = wire_ai_with_overrides(
            config.non_interactive,
            config.verbose,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        match cmd {
            AiCommand::ListProfiles => {
                let (names, default) = app.ai_use_case.list_profiles()?;
                let text = names
                    .into_iter()
                    .map(|name| {
                        if default.as_deref() == Some(name.as_str()) {
                            format!("{name} (default)")
                        } else {
                            name
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let text = if text.is_empty() {
                    text
                } else {
                    format!("{text}\n")
                };
                emit_stdout(&emitter, &job_id, text)?;
                return Ok(0);
            }
            AiCommand::ListTools { profile } => {
                const DESC_MAX_LEN: usize = 52;
                let mut lines = Vec::new();
                if let Some(ref p) = profile {
                    lines.push(format!("Tools enabled for profile '{}':", p.as_ref()));
                } else {
                    lines.push("Tools:".to_string());
                }
                for (name, desc) in app.ai_use_case.list_tools() {
                    if desc.is_empty() {
                        lines.push(format!("  {}", name));
                    } else {
                        let short = if desc.chars().count() <= DESC_MAX_LEN {
                            desc
                        } else {
                            format!("{}...", desc.chars().take(DESC_MAX_LEN).collect::<String>())
                        };
                        lines.push(format!("  {}  {}", name, short));
                    }
                }
                emit_stdout(&emitter, &job_id, format!("{}\n", lines.join("\n")))?;
                return Ok(0);
            }
            AiCommand::PolicyExplain => {
                let info = app.policy_use_case.explain()?;
                let json =
                    serde_json::to_string_pretty(&info).map_err(|e| Error::json(e.to_string()))?;
                emit_stdout(&emitter, &job_id, format!("{json}\n"))?;
                return Ok(0);
            }
            AiCommand::ConfigExplain => {
                let info = app.config_use_case.explain()?;
                let json =
                    serde_json::to_string_pretty(&info).map_err(|e| Error::json(e.to_string()))?;
                emit_stdout(&emitter, &job_id, format!("{json}\n"))?;
                return Ok(0);
            }
            AiCommand::SessionsRebuildDerived => {
                let session_dir = app.env_resolver.session_dir_from_env().ok_or_else(|| {
                    Error::invalid_argument(
                        "sessions-rebuild-derived requires a session. Set AISH_SESSION or use aish -s/--session-dir.".to_string(),
                    )
                })?;
                app.session_use_case.rebuild_derived(&session_dir)?;
                return Ok(0);
            }
            AiCommand::Help
            | AiCommand::Task { .. }
            | AiCommand::Resume { .. }
            | AiCommand::Query { .. } => {}
        }
    }

    let sink_factory: Arc<dyn EventSinkFactory> = Arc::new(StreamingEventSinkFactory::new(
        Arc::clone(&emitter),
        job_id.clone(),
    ));
    let dry_run_report_sink: Arc<dyn DryRunReportSink> = Arc::new(StreamingDryRunReportSink::new(
        Arc::clone(&emitter),
        job_id.clone(),
    ));
    let approver: Arc<dyn ToolApproval> =
        Arc::new(StreamingToolApproval::new(Arc::clone(&approval_handler)));
    let continue_prompt: Arc<dyn ContinueAfterLimitPrompt> =
        Arc::new(StreamingContinuePrompt::new(Arc::clone(&approval_handler)));
    let sensitive_prompt: Arc<dyn crate::adapter::SensitiveContentPrompt> =
        Arc::new(StreamingSensitivePrompt::new(approval_handler));
    let process_output_observer: Arc<dyn ProcessOutputObserver> =
        Arc::new(StreamingProcessOutputObserver::new(Arc::clone(&emitter), job_id));
    let app = wire_ai_with_overrides(
        config.non_interactive,
        config.verbose,
        Some(sink_factory),
        Some(dry_run_report_sink),
        Some(approver),
        Some(continue_prompt),
        Some(sensitive_prompt),
        Some(process_output_observer),
    );
    let runner = crate::cli_entry::Runner { app };
    crate::ports::inbound::UseCaseRunner::run(&runner, config)
}

#[cfg(test)]
mod tests {
    use super::{max_backend_job_depth, DEFAULT_MAX_BACKEND_JOB_DEPTH};
    use daemon_api::AiRunRequest;

    #[test]
    fn max_backend_job_depth_uses_request_override() {
        let depth = max_backend_job_depth(&AiRunRequest {
            job_id: "job-1".to_string(),
            parent_job_id: None,
            nesting_depth: 0,
            max_nesting_depth: Some(3),
            argv: vec!["ai".to_string()],
            session_dir: None,
            aish_home: None,
            frontend_tty: None,
            cwd: None,
        });
        assert_eq!(depth, 3);
    }

    #[test]
    fn max_backend_job_depth_defaults_when_request_has_no_override() {
        let depth = max_backend_job_depth(&AiRunRequest {
            job_id: "job-1".to_string(),
            parent_job_id: None,
            nesting_depth: 0,
            max_nesting_depth: None,
            argv: vec!["ai".to_string()],
            session_dir: None,
            aish_home: None,
            frontend_tty: None,
            cwd: None,
        });
        assert_eq!(depth, DEFAULT_MAX_BACKEND_JOB_DEPTH);
    }
}

