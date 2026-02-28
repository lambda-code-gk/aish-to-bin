//! Library entry for aish: run from env args or from given args (used by crates/aish).

mod adapter;
mod cli;
mod domain;
mod ports;
mod usecase;
mod wiring;

#[cfg(unix)]
mod daemon;

// Re-export run and run_with_args from main. The binary and the library share the same code;
// the library target does not compile main.rs, so we must provide run_with_args here.
// We duplicate the minimal dispatch: parse_args_from_os + run_with_outcome logic.

use common::error::Error;
use common::ports::outbound::PathResolverInput;
use ports::inbound::UseCaseRunner;

/// Run aish with arguments from the environment (binary entry point).
pub fn run() -> Result<i32, Error> {
    let outcome = cli::parse_args()?;
    run_with_outcome(&outcome)
}

/// Run aish with the given argument iterator (used by crates/aish for `aish shell` etc.).
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
            Command::SessionsRebuildDerived { session_id } => entry_run_ai_sessions_rebuild_derived(
                &path_input,
                session_id.as_deref(),
                &self.app.path_resolver,
            ),
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
                let entries = self.app.memory_use_case.list()?;
                entry_print_memory_list(&entries);
                Ok(0)
            }
            Command::MemoryGet { ids } => {
                if ids.is_empty() {
                    return Err(Error::invalid_argument(
                        "memory get requires at least one id".to_string(),
                    ));
                }
                let entries = self.app.memory_use_case.get(&ids)?;
                entry_print_memory_get(&entries);
                Ok(0)
            }
            Command::MemoryRemove { ids } => {
                if ids.is_empty() {
                    return Err(Error::invalid_argument(
                        "memory remove requires at least one id".to_string(),
                    ));
                }
                self.app.memory_use_case.remove(&ids)?;
                Ok(0)
            }
            Command::HistoryLs {
                all,
                user_only,
                assistant_only,
            } => {
                let session_explicitly_specified = entry_is_session_explicitly_specified(&config);
                let entries = self.app.history_use_case.list(
                    &path_input,
                    session_explicitly_specified,
                    all,
                    user_only,
                    assistant_only,
                )?;
                let width = (self.app.get_terminal_width)();
                entry_print_history_list(&entries, width);
                Ok(0)
            }
            Command::HistoryGet { ids } => {
                let session_explicitly_specified = entry_is_session_explicitly_specified(&config);
                let entries = self.app.history_use_case.get(
                    &path_input,
                    session_explicitly_specified,
                    &ids,
                )?;
                entry_print_history_get(&entries);
                Ok(0)
            }
            Command::PolicyExplain => entry_run_ai_policy_explain(),
            Command::ConfigExplain => entry_run_ai_config_explain(),
            Command::PluginsList => {
                let mut list = self.app.mcp_host.discover()?;
                list.sort_by(|a, b| a.id.0.cmp(&b.id.0));
                for p in list {
                    let flag = if p.enabled { "enabled" } else { "disabled" };
                    let src = p.source.unwrap_or_default();
                    println!("{}\t{}\t{}\t{}", flag, p.id.0, p.namespace, src);
                }
                Ok(0)
            }
            #[cfg(unix)]
            Command::DaemonStart => {
                let path = daemon::default_socket_path();
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| Error::invalid_argument(e.to_string()))?;
                rt.block_on(daemon::run_server(path))
                    .map_err(|e: Box<dyn std::error::Error + Send + Sync>| Error::invalid_argument(e.to_string()))?;
                Ok(0)
            }
            #[cfg(unix)]
            Command::DaemonPing => {
                let path = daemon::default_socket_path();
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| Error::invalid_argument(e.to_string()))?;
                let ok = rt.block_on(daemon::run_ping(&path))
                    .map_err(|e: Box<dyn std::error::Error + Send + Sync>| Error::invalid_argument(e.to_string()))?;
                Ok(if ok { 0 } else { 1 })
            }
            #[cfg(unix)]
            Command::DaemonStatus => {
                let path = daemon::default_socket_path();
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| Error::invalid_argument(e.to_string()))?;
                rt.block_on(daemon::run_status(&path))
                    .map_err(|e: Box<dyn std::error::Error + Send + Sync>| Error::invalid_argument(e.to_string()))?;
                Ok(0)
            }
            Command::ToolsList => {
                let mut tool_ids: Vec<String> = Vec::new();
                let servers = self.app.mcp_host.discover()?;
                for s in servers.into_iter().filter(|s| s.enabled) {
                    match self.app.mcp_host.list_tools(&s.id) {
                        Ok(tools) => {
                            for t in tools {
                                tool_ids.push(t.id.0);
                            }
                        }
                        Err(e) => {
                            eprintln!("tools list failed for {}: {}", s.id.0, e);
                        }
                    }
                }
                tool_ids.sort();
                tool_ids.dedup();
                for id in tool_ids {
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
    let status = std::process::Command::new("ai")
        .arg("--policy-explain")
        .status()
        .map_err(|e| Error::io_msg(format!("Failed to run ai: {}", e)))?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(unix)]
fn entry_run_ai_config_explain() -> Result<i32, Error> {
    let status = std::process::Command::new("ai")
        .arg("--config-explain")
        .status()
        .map_err(|e| Error::io_msg(format!("Failed to run ai: {}", e)))?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(unix)]
fn entry_run_ai_sessions_rebuild_derived(
    path_input: &common::ports::outbound::PathResolverInput,
    session_id: Option<&str>,
    path_resolver: &std::sync::Arc<dyn common::ports::outbound::PathResolver>,
) -> Result<i32, Error> {
    use common::ports::outbound::PathResolver;
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
    let status = std::process::Command::new("ai")
        .arg("-s")
        .arg(&session_path)
        .arg("--sessions-rebuild-derived")
        .status()
        .map_err(|e| Error::io_msg(format!("Failed to run ai: {}", e)))?;
    Ok(status.code().unwrap_or(1))
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
                &e.datetime[..e.datetime.floor_char_boundary(DATETIME_WIDTH.saturating_sub(3))]
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
