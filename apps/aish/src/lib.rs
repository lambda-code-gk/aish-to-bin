//! Library entry for aish: run from env args or from given args (used by bins/aish-cli).

mod adapter;
mod cli;
mod daemon_access;
mod daemon_bridge;
mod daemon_cli;
mod daemon_handler;
mod daemon_server;
mod domain;
mod ports;
mod usecase;
mod wiring;

// Re-export run and run_with_args from main. The binary and the library share the same code;
// the library target does not compile main.rs, so we must provide run_with_args here.
// We duplicate the minimal dispatch: parse_args_from_os + run_with_outcome logic.

use common::error::Error;
use common::ports::outbound::PathResolverInput;
use ports::inbound::UseCaseRunner;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Run aish with arguments from the environment (binary entry point).
pub fn run() -> Result<i32, Error> {
    let outcome = cli::parse_args()?;
    run_with_outcome(&outcome)
}

/// Run aish with the given argument iterator (used by bins/aish-cli for `aish shell` etc.).
#[cfg(unix)]
pub fn run_with_args(
    args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
) -> Result<i32, Error> {
    let outcome = cli::parse_args_from_os(args)?;
    run_with_outcome(&outcome)
}

#[cfg(not(unix))]
pub fn run_with_args(
    _args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
) -> Result<i32, Error> {
    Err(Error::system("aish is only supported on Unix"))
}

#[cfg(unix)]
fn run_with_outcome(outcome: &cli::ParseOutcome) -> Result<i32, Error> {
    let config = match outcome {
        cli::ParseOutcome::Config(c) => c.clone(),
        cli::ParseOutcome::GenerateCompletion(shell) => {
            cli::print_completion(*shell);
            return Ok(0);
        }
    };
    if let Some(ref h) = config.home_dir {
        std::env::set_var("AISH_HOME", h);
    }
    let app = wiring::wire_aish();
    let runner = AishRunner { app };
    runner.run(config)
}

#[cfg(unix)]
struct AishRunner {
    app: wiring::App,
}

#[cfg(unix)]
fn ai_process_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(unix)]
struct AiEnvGuard {
    _guard: MutexGuard<'static, ()>,
    prev_session: Option<String>,
    prev_home: Option<String>,
    prev_cwd: PathBuf,
}

#[cfg(unix)]
impl AiEnvGuard {
    fn apply(session_dir: Option<&str>, home_dir: Option<&str>) -> Result<Self, Error> {
        let guard = ai_process_lock()
            .lock()
            .map_err(|_| Error::system("ai env lock poisoned"))?;
        let prev_session = std::env::var("AISH_SESSION").ok();
        let prev_home = std::env::var("AISH_HOME").ok();
        let prev_cwd = std::env::current_dir().map_err(|e| Error::io_msg(e.to_string()))?;
        match session_dir {
            Some(value) => std::env::set_var("AISH_SESSION", value),
            None => std::env::remove_var("AISH_SESSION"),
        }
        match home_dir {
            Some(value) => std::env::set_var("AISH_HOME", value),
            None => std::env::remove_var("AISH_HOME"),
        }
        Ok(Self {
            _guard: guard,
            prev_session,
            prev_home,
            prev_cwd,
        })
    }
}

#[cfg(unix)]
impl Drop for AiEnvGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev_cwd);
        match &self.prev_session {
            Some(value) => std::env::set_var("AISH_SESSION", value),
            None => std::env::remove_var("AISH_SESSION"),
        }
        match &self.prev_home {
            Some(value) => std::env::set_var("AISH_HOME", value),
            None => std::env::remove_var("AISH_HOME"),
        }
    }
}

#[cfg(unix)]
impl UseCaseRunner for AishRunner {
    fn run(&self, config: cli::Config) -> Result<i32, Error> {
        use domain::command::Command;

        let command = cli::config_to_command(&config);
        let path_input = PathResolverInput {
            home_dir: config.home_dir.clone(),
            session_dir: config.session_dir.clone(),
        };

        match command {
            Command::Help => {
                entry_print_help();
                Ok(0)
            }
            Command::Shell => self.app.shell_use_case.run(&path_input),
            Command::ShellStatus => {
                let snapshot = self.app.shell_status_use_case.get(&path_input)?;
                entry_print_shell_status(&snapshot);
                Ok(0)
            }
            Command::TruncateConsoleLog => self.app.truncate_console_log_use_case.run(&path_input),
            Command::Rollout => self.app.rollout_use_case.run(&path_input),
            Command::Mute => self.app.mute_use_case.run(&path_input),
            Command::Unmute => self.app.unmute_use_case.run(&path_input),
            Command::Clear => {
                let session_explicitly_specified = entry_is_session_explicitly_specified(&config);
                self.app
                    .clear_use_case
                    .run(&path_input, session_explicitly_specified)
            }
            Command::Resume { id } => self.app.resume_use_case.run(&path_input, id.as_deref()),
            Command::Sessions => {
                let ids = self.app.sessions_use_case.list(&path_input)?;
                for id in ids {
                    println!("{}", id);
                }
                Ok(0)
            }
            Command::SessionsRebuildDerived { session_id } => {
                entry_run_ai_sessions_rebuild_derived(
                    &path_input,
                    session_id.as_deref(),
                    &self.app.path_resolver,
                )
            }
            Command::Init {
                force,
                dry_run,
                defaults_dir: defaults_dir_opt,
            } => {
                let defaults_dir = defaults_dir_opt
                    .or_else(|| std::env::var("AISH_DEFAULTS_DIR").ok())
                    .ok_or_else(|| {
                        Error::invalid_argument(
                            "Defaults directory required: set AISH_DEFAULTS_DIR or use --defaults-dir"
                                .to_string(),
                        )
                    })?;
                let input = usecase::InitInput {
                    defaults_dir: std::path::PathBuf::from(&defaults_dir),
                    force,
                    dry_run,
                };
                let result = self.app.init_use_case.run(&input)?;
                if result.dry_run {
                    for p in &result.copied_paths {
                        println!("  {}", p.display());
                    }
                    println!(
                        "Would copy {} file(s) to {}",
                        result.copied_count,
                        result.config_dir.display()
                    );
                } else {
                    println!(
                        "Initialized config at {} ({} file(s))",
                        result.config_dir.display(),
                        result.copied_count
                    );
                }
                Ok(0)
            }
            Command::MemoryList => {
                let entries: Vec<domain::MemoryListEntry> =
                    daemon_access::run_aish_read_with_fallback(
                        daemon_api::AishReadRequest::MemoryList {
                            context: daemon_access::backend_context(&config),
                        },
                        || self.app.memory_use_case.list(),
                    )?;
                entry_print_memory_list(&entries);
                Ok(0)
            }
            Command::MemoryGet { ids } => {
                if ids.is_empty() {
                    return Err(Error::invalid_argument(
                        "memory get requires at least one id".to_string(),
                    ));
                }
                let ids_for_fallback = ids.clone();
                let entries: Vec<domain::MemoryEntry> = daemon_access::run_aish_read_with_fallback(
                    daemon_api::AishReadRequest::MemoryGet {
                        context: daemon_access::backend_context(&config),
                        ids,
                    },
                    || self.app.memory_use_case.get(&ids_for_fallback),
                )?;
                entry_print_memory_get(&entries);
                Ok(0)
            }
            Command::MemoryRemove { ids } => {
                if ids.is_empty() {
                    return Err(Error::invalid_argument(
                        "memory remove requires at least one id".to_string(),
                    ));
                }
                let ids_for_fallback = ids.clone();
                daemon_access::run_aish_write_with_fallback(
                    daemon_api::AishWriteRequest::MemoryRemove {
                        context: daemon_access::backend_context(&config),
                        ids,
                    },
                    || self.app.memory_use_case.remove(&ids_for_fallback),
                )?;
                Ok(0)
            }
            Command::HistoryLs {
                all,
                user_only,
                assistant_only,
            } => {
                let session_explicitly_specified = entry_is_session_explicitly_specified(&config);
                let entries: Vec<domain::HistoryListEntry> =
                    daemon_access::run_aish_read_with_fallback(
                        daemon_api::AishReadRequest::HistoryList {
                            context: daemon_access::backend_context(&config),
                            session_explicitly_specified,
                            all,
                            user_only,
                            assistant_only,
                        },
                        || {
                            self.app.history_use_case.list(
                                &path_input,
                                session_explicitly_specified,
                                all,
                                user_only,
                                assistant_only,
                            )
                        },
                    )?;
                let width = (self.app.get_terminal_width)();
                entry_print_history_list(&entries, width);
                Ok(0)
            }
            Command::HistoryGet { ids } => {
                let session_explicitly_specified = entry_is_session_explicitly_specified(&config);
                let ids_for_fallback = ids.clone();
                let entries: Vec<domain::HistoryGetEntry> =
                    daemon_access::run_aish_read_with_fallback(
                        daemon_api::AishReadRequest::HistoryGet {
                            context: daemon_access::backend_context(&config),
                            session_explicitly_specified,
                            ids,
                        },
                        || {
                            self.app.history_use_case.get(
                                &path_input,
                                session_explicitly_specified,
                                &ids_for_fallback,
                            )
                        },
                    )?;
                entry_print_history_get(&entries);
                Ok(0)
            }
            Command::PolicyExplain => entry_run_ai_policy_explain(),
            Command::ConfigExplain => entry_run_ai_config_explain(),
            Command::PluginsList => {
                let list = daemon_access::run_aish_read_with_fallback(
                    daemon_api::AishReadRequest::PluginsList {
                        context: daemon_access::backend_context(&config),
                    },
                    || Ok(daemon_access::sort_plugins(self.app.mcp_host.discover()?)),
                )?;
                for p in list {
                    let flag = if p.enabled { "enabled" } else { "disabled" };
                    let src = p.source.unwrap_or_default();
                    println!("{}\t{}\t{}\t{}", flag, p.id.0, p.namespace, src);
                }
                Ok(0)
            }
            #[cfg(unix)]
            Command::DaemonStart { detach } => {
                if detach {
                    daemon_cli::run_start_detached()
                } else {
                    daemon_cli::run_start()
                }
            }
            #[cfg(unix)]
            Command::DaemonPing => daemon_cli::run_ping(),
            #[cfg(unix)]
            Command::DaemonStatus => daemon_cli::run_status(),
            #[cfg(unix)]
            Command::DaemonJobs {
                active_only,
                persisted_only,
            } => daemon_cli::run_jobs(
                &config,
                &self.app,
                &path_input,
                active_only,
                persisted_only,
                entry_is_session_explicitly_specified(&config),
            ),
            #[cfg(unix)]
            Command::DaemonStop => daemon_cli::run_stop(),
            #[cfg(unix)]
            Command::DaemonCancel { job_id } => daemon_cli::run_cancel(&job_id),
            #[cfg(unix)]
            Command::DaemonEnsure => daemon_cli::run_ensure(),
            Command::ToolsList => {
                let result: daemon_api::AishToolsListResult =
                    daemon_access::run_aish_read_with_fallback(
                        daemon_api::AishReadRequest::ToolsList {
                            context: daemon_access::backend_context(&config),
                        },
                        || daemon_access::list_tools_locally(&self.app),
                    )?;
                for warning in result.warnings {
                    eprintln!("{}", warning);
                }
                for id in result.tool_ids {
                    println!("{}", id);
                }
                Ok(0)
            }
            Command::Unknown(name) => Err(Error::invalid_argument(format!(
                "Command '{}' is not implemented.",
                name
            ))),
        }
    }
}

#[cfg(unix)]
fn entry_is_session_explicitly_specified(config: &cli::Config) -> bool {
    if config.session_dir.is_some() || config.home_dir.is_some() {
        return true;
    }
    if let Ok(env_session) = std::env::var("AISH_SESSION") {
        if !env_session.is_empty() {
            return true;
        }
    }
    if let Ok(env_home) = std::env::var("AISH_HOME") {
        if !env_home.is_empty() {
            return true;
        }
    }
    false
}

#[cfg(unix)]
fn entry_run_ai_policy_explain() -> Result<i32, Error> {
    ai::run_with_args([
        std::ffi::OsString::from("ai"),
        std::ffi::OsString::from("--policy-explain"),
    ])
}

#[cfg(unix)]
fn entry_run_ai_config_explain() -> Result<i32, Error> {
    ai::run_with_args([
        std::ffi::OsString::from("ai"),
        std::ffi::OsString::from("--config-explain"),
    ])
}

#[cfg(unix)]
fn entry_print_shell_status(snapshot: &domain::ShellStatusSnapshot) {
    println!("[session]");
    println!("session_id\t{}", snapshot.session_id);
    println!("persisted_job_count\t{}", snapshot.persisted_job_count);
    if let Some(job) = &snapshot.latest_job {
        println!("latest_job_id\t{}", job.job_id);
        println!("latest_job_state\t{}", job.state);
        if let Some(exit_code) = job.exit_code {
            println!("latest_job_exit_code\t{}", exit_code);
        }
    }
    println!();
    println!("[shell attachment]");
    match &snapshot.attachment {
        Some(attachment) => {
            let status = match attachment.status {
                domain::ShellAttachmentStatus::Attached => "attached",
                domain::ShellAttachmentStatus::Detached => "detached",
            };
            println!("status\t{}", status);
            if let Some(pid) = attachment.pid {
                println!("pid\t{}", pid);
            }
            println!("muted\t{}", attachment.muted);
            println!("console_path\t{}", attachment.console_path);
            println!("pending_input_path\t{}", attachment.pending_input_path);
            println!(
                "prompt_suggestion_path\t{}",
                attachment.prompt_suggestion_path
            );
            println!("mute_flag_path\t{}", attachment.mute_flag_path);
            println!("part_file_prefix\t{}", attachment.part_file_prefix);
            let job_link_mode = match attachment.job_link_mode {
                domain::ShellJobLinkMode::SessionEvents => "session_events",
            };
            println!("job_link_mode\t{}", job_link_mode);
            let part_file_tracking_mode = match attachment.part_file_tracking_mode {
                domain::ShellPartTrackingMode::LayoutOnly => "layout_only",
            };
            println!("part_file_tracking_mode\t{}", part_file_tracking_mode);
        }
        None => println!("status\tnone"),
    }
    println!();
    println!("[console log]");
    println!("console_exists\t{}", snapshot.console_exists);
    if let Some(bytes) = snapshot.console_bytes {
        println!("console_bytes\t{}", bytes);
    }
    println!("part_file_count\t{}", snapshot.part_file_count);
    if let Some(part_file) = &snapshot.latest_part_file {
        println!("latest_part_file\t{}", part_file);
    }
    println!("mute_flag_exists\t{}", snapshot.mute_flag_exists);
    println!("pending_input_exists\t{}", snapshot.pending_input_exists);
    println!(
        "prompt_suggestion_exists\t{}",
        snapshot.prompt_suggestion_exists
    );
}

#[cfg(unix)]
fn entry_run_ai_sessions_rebuild_derived(
    path_input: &common::ports::outbound::PathResolverInput,
    session_id: Option<&str>,
    path_resolver: &std::sync::Arc<dyn common::ports::outbound::PathResolver>,
) -> Result<i32, Error> {
    let home_dir = path_resolver.resolve_home_dir(path_input)?;
    let default_session = path_resolver.resolve_session_dir(path_input, &home_dir)?;
    let session_path = if let Some(id) = session_id {
        let parent = std::path::Path::new(&default_session)
            .parent()
            .ok_or_else(|| Error::system("Failed to resolve sessions root"))?;
        parent.join(id).to_string_lossy().to_string()
    } else {
        default_session
    };
    let _env = AiEnvGuard::apply(Some(&session_path), Some(&home_dir))?;
    ai::run_with_args([
        std::ffi::OsString::from("ai"),
        std::ffi::OsString::from("--sessions-rebuild-derived"),
    ])
}

#[cfg(unix)]
fn entry_print_help() {
    println!("Usage: aish [-h] [-s|--session-dir directory] [-d|--home-dir directory] [<command> [args...]]");
    println!("  -h, --help            Display this help message.");
    println!("  -d, --home-dir        Home directory (sets AISH_HOME).");
    println!("  -s, --session-dir     Session dir (resume). Omit for new session each run.");
    println!("  -v, --verbose         Verbose debug logs.");
    println!("  --generate <shell>    Emit completion (bash, zsh, fish). Source to enable.");
    println!("  <command>             Command to run. Omit to start interactive shell.");
    println!("  [args...]             Command arguments.");
}

#[cfg(unix)]
fn entry_print_memory_list(entries: &[domain::MemoryListEntry]) {
    if entries.is_empty() {
        println!("(no memories)");
        return;
    }
    println!("{:18} {:<16} {}", "ID", "CATEGORY", "SUBJECT");
    for e in entries {
        let subject = if e.subject.len() > 50 {
            format!("{}...", &e.subject[..e.subject.floor_char_boundary(47)])
        } else {
            e.subject.clone()
        };
        println!("{:18} {:<16} {}", e.id, e.category, subject);
    }
}

#[cfg(unix)]
fn entry_print_memory_get(entries: &[domain::MemoryEntry]) {
    for (i, e) in entries.iter().enumerate() {
        if entries.len() > 1 {
            println!("--- {} (id={}) ---", e.subject, e.id);
        }
        println!("{}", e.content);
        if i + 1 < entries.len() {
            println!();
        }
    }
}

#[cfg(unix)]
fn entry_truncate_by_display_width(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    const ELLIPSIS_WIDTH: usize = 3;
    let content_max = max_width.saturating_sub(ELLIPSIS_WIDTH);
    let mut w = 0usize;
    let mut last_end = 0;
    for (i, c) in s.char_indices() {
        let cw = c.width().unwrap_or(1);
        if w + cw > content_max {
            return format!("{}...", &s[..last_end]);
        }
        w += cw;
        last_end = i + c.len_utf8();
    }
    s.to_string()
}

#[cfg(unix)]
fn entry_print_history_list(entries: &[domain::HistoryListEntry], width: usize) {
    use regex::Regex;
    const ID_WIDTH: usize = 9;
    const DATETIME_WIDTH: usize = 16;
    const SEP: &str = " ";
    const MIN_FIRST_LINE_WIDTH: usize = 50;
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\(aish:[^)]*\)[^$]*\$ ").unwrap());
    let reserved = ID_WIDTH + SEP.len() + DATETIME_WIDTH + SEP.len();
    let first_line_max = width.saturating_sub(reserved).max(MIN_FIRST_LINE_WIDTH);
    for e in entries {
        let id = format!("{:<width$}", e.id, width = ID_WIDTH);
        let dt_show = if e.datetime.len() <= DATETIME_WIDTH {
            e.datetime.clone()
        } else {
            format!(
                "{}...",
                &e.datetime[..e
                    .datetime
                    .floor_char_boundary(DATETIME_WIDTH.saturating_sub(3))]
            )
        };
        let dt = format!("{:16}", dt_show);
        let first_line = re.replace(&e.first_line, "...$ ");
        let first_line = entry_truncate_by_display_width(first_line.as_ref(), first_line_max);
        println!("{}{}{}{}{}", id, SEP, dt, SEP, first_line);
    }
}

#[cfg(unix)]
fn entry_print_history_get(entries: &[domain::HistoryGetEntry]) {
    for (i, e) in entries.iter().enumerate() {
        if entries.len() > 1 {
            println!("--- id={} ---", e.id);
        }
        print!("{}", e.content);
        if !e.content.ends_with('\n') {
            println!();
        }
        if i + 1 < entries.len() {
            println!();
        }
    }
}
