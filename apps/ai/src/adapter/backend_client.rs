//! daemon backend へ ai の実行要求を委譲するクライアント

use std::fs::{self, OpenOptions};
use std::io::IsTerminal;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use common::error::Error;
use daemon_api::{
    read_frame_sync, write_frame_sync, AiClientFrame, AiInteractionRequest, AiInteractionResponse,
    AiRunRequest, AiStreamFrame, Request, RequestOp, PROTOCOL_VERSION,
};

use crate::adapter::{CliToolApproval, StdEventSinkFactory};
use crate::ports::outbound::{Approval, EventSinkFactory, ToolApproval};

fn backend_required_error(detail: impl Into<String>) -> Error {
    Error::invalid_argument(format!(
        "AISH backend is required for this ai command. Start it with 'aish daemon start'. {}",
        detail.into()
    ))
}

fn ensure_backend_available() -> Result<(), Error> {
    let socket_path = daemon_api::default_socket_path();
    // Fast path: daemon is already accepting connections.
    if UnixStream::connect(&socket_path).is_ok() {
        return Ok(());
    }
    if std::env::var_os("AISH_NO_AUTOSTART").is_some() {
        return Err(backend_required_error(format!(
            "daemon is not running and autostart is disabled by AISH_NO_AUTOSTART (socket: {})",
            socket_path.display()
        )));
    }

    // Slow path: try to ask aish to ensure the daemon.
    let mut child = Command::new("aish")
        .arg("daemon")
        .arg("ensure")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| backend_required_error(format!("spawn 'aish daemon ensure': {}", e)))?;

    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err(backend_required_error(format!(
                        "'aish daemon ensure' failed with status {}",
                        status
                    )));
                }
                break;
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    return Err(backend_required_error(
                        "'aish daemon ensure' timed out waiting for daemon to become ready"
                            .to_string(),
                    ));
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(backend_required_error(format!(
                    "wait for 'aish daemon ensure': {}",
                    e
                )));
            }
        }
    }

    // Final verification: daemon should now accept connections.
    UnixStream::connect(&socket_path)
        .map(|_| ())
        .map_err(|e| {
            backend_required_error(format!(
                "connect {} after 'aish daemon ensure': {}",
                socket_path.display(),
                e
            ))
        })
}

fn prompt_continue(prompt: &str) -> Result<bool, Error> {
    let mut line = String::new();
    prompt_line(prompt, &mut line)?;
    let input = line.trim().to_lowercase();
    Ok(input == "y" || input == "yes")
}

fn prompt_sensitive_choice(
    verbose_output: &str,
) -> Result<daemon_api::SensitiveChoiceValue, Error> {
    let mut line = String::new();
    write_prompt_text("\x1b[1;33mSECURITY: Sensitive content matched\x1b[0m\n")?;
    write_prompt_text("----------------------------------------\n")?;
    write_prompt_text(verbose_output)?;
    if !verbose_output.ends_with('\n') {
        write_prompt_text("\n")?;
    }
    write_prompt_text("----------------------------------------\n")?;
    prompt_line("Send to LLM? [y]es / [n]o (deny) / [m]ask: ", &mut line)?;
    Ok(match line.trim().to_lowercase().as_str() {
        "y" | "yes" | "" => daemon_api::SensitiveChoiceValue::Allow,
        "m" | "mask" => daemon_api::SensitiveChoiceValue::Mask,
        _ => daemon_api::SensitiveChoiceValue::Deny,
    })
}

fn prompt_approval(command: &str) -> Result<bool, Error> {
    write_prompt_text("============ Approval =============\n")?;
    write_prompt_text(&format!("  {}\n", command))?;
    let mut line = String::new();
    prompt_line("Execute? [Enter/No(other)]: ", &mut line)?;
    Ok(line.trim().is_empty())
}

fn resolve_frontend_tty_path() -> Option<String> {
    if let Ok(path) = std::env::var("AISH_FRONTEND_TTY") {
        if !path.trim().is_empty() {
            return Some(path);
        }
    }
    resolve_terminal_fd_path(0).or_else(|| resolve_terminal_fd_path(2))
}

fn resolve_terminal_fd_path(fd: i32) -> Option<String> {
    let is_terminal = match fd {
        0 => io::stdin().is_terminal(),
        2 => io::stderr().is_terminal(),
        _ => false,
    };
    if !is_terminal {
        return None;
    }
    fs::read_link(format!("/proc/self/fd/{}", fd))
        .ok()
        .and_then(|path| path.into_os_string().into_string().ok())
}

fn write_prompt_text(text: &str) -> Result<(), Error> {
    if let Some(tty_path) = resolve_frontend_tty_path().filter(|_| !io::stdin().is_terminal()) {
        let mut writer = OpenOptions::new()
            .write(true)
            .open(&tty_path)
            .map_err(|e| {
                Error::io_msg(format!("open frontend tty for write {}: {}", tty_path, e))
            })?;
        writer
            .write_all(text.as_bytes())
            .map_err(|e| Error::io_msg(format!("write frontend tty {}: {}", tty_path, e)))?;
        writer
            .flush()
            .map_err(|e| Error::io_msg(format!("flush frontend tty {}: {}", tty_path, e)))?;
        return Ok(());
    }
    eprint!("{}", text);
    io::stderr()
        .flush()
        .map_err(|e| Error::io_msg(format!("flush stderr: {}", e)))
}

fn prompt_line(prompt: &str, out: &mut String) -> Result<(), Error> {
    write_prompt_text(prompt)?;
    if let Some(tty_path) = resolve_frontend_tty_path().filter(|_| !io::stdin().is_terminal()) {
        let reader_file = OpenOptions::new().read(true).open(&tty_path).map_err(|e| {
            Error::io_msg(format!("open frontend tty for read {}: {}", tty_path, e))
        })?;
        let mut reader = BufReader::new(reader_file);
        reader
            .read_line(out)
            .map_err(|e| Error::io_msg(format!("read frontend tty {}: {}", tty_path, e)))?;
        return Ok(());
    }
    let stdin = io::stdin();
    stdin
        .lock()
        .read_line(out)
        .map_err(|e| Error::io_msg(e.to_string()))?;
    Ok(())
}

fn interrupt_flag() -> Arc<AtomicBool> {
    static FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    FLAG.get_or_init(|| {
        let flag = Arc::new(AtomicBool::new(false));
        let flag_for_handler = Arc::clone(&flag);
        let _ = ctrlc::set_handler(move || {
            flag_for_handler.store(true, Ordering::Relaxed);
        });
        flag
    })
    .clone()
}

fn send_cancel_request(job_id: &str) -> Result<(), Error> {
    let socket_path = daemon_api::default_socket_path();
    let mut stream = UnixStream::connect(&socket_path).map_err(|e| {
        Error::io_msg(format!(
            "connect {} for cancel: {}",
            socket_path.display(),
            e
        ))
    })?;
    let id = format!("cancel-ai-{}", job_id);
    let req = Request {
        v: PROTOCOL_VERSION,
        id: id.clone(),
        op: RequestOp::CancelAi {
            job_id: job_id.to_string(),
        },
    };
    write_frame_sync(&mut stream, &req).map_err(|e| Error::io_msg(e.to_string()))?;
    let response: daemon_api::Response<daemon_api::CancelAiResult> =
        read_frame_sync(&mut stream).map_err(|e| Error::io_msg(e.to_string()))?;
    if !response.ok {
        let msg = response
            .error
            .map(|e| format!("{}: {}", e.code, e.message))
            .unwrap_or_else(|| "cancel failed".to_string());
        return Err(Error::io_msg(msg));
    }
    Ok(())
}

fn generate_job_id() -> String {
    format!(
        "job-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

fn resolve_job_id() -> String {
    std::env::var("AISH_BACKEND_JOB_ID").unwrap_or_else(|_| generate_job_id())
}

fn resolve_nesting_depth() -> Result<u32, Error> {
    match std::env::var("AISH_JOB_DEPTH") {
        Ok(value) => value.parse::<u32>().map_err(|_| {
            Error::invalid_argument(format!("invalid AISH_JOB_DEPTH value: {}", value))
        }),
        Err(_) => Ok(0),
    }
}

fn resolve_max_nesting_depth() -> Result<Option<u32>, Error> {
    match std::env::var("AISH_MAX_BACKEND_JOB_DEPTH") {
        Ok(value) => value.parse::<u32>().map(Some).map_err(|_| {
            Error::invalid_argument(format!(
                "invalid AISH_MAX_BACKEND_JOB_DEPTH value: {}",
                value
            ))
        }),
        Err(_) => Ok(None),
    }
}

fn nested_non_interactive_response(
    request: &AiInteractionRequest,
    nested_job_depth_present: bool,
    stdin_is_terminal: bool,
) -> Option<AiInteractionResponse> {
    if !nested_job_depth_present || stdin_is_terminal {
        return None;
    }
    Some(match request {
        AiInteractionRequest::Approval { .. } => {
            AiInteractionResponse::Approval { approved: false }
        }
        AiInteractionRequest::Continue { .. } => {
            AiInteractionResponse::Continue { continue_: false }
        }
        AiInteractionRequest::SensitivePrompt { .. } => AiInteractionResponse::SensitivePrompt {
            choice: daemon_api::SensitiveChoiceValue::Deny,
        },
    })
}

fn nested_lifecycle_notice(
    job_id: &str,
    parent_job_id: Option<&str>,
    state: &daemon_api::AiJobState,
) -> Option<String> {
    match state {
        daemon_api::AiJobState::Queued => Some(match parent_job_id {
            Some(parent_job_id) => format!(
                "[backend job] started nested job_id={} parent_job_id={}\n",
                job_id, parent_job_id
            ),
            None => format!("[backend job] started nested job_id={}\n", job_id),
        }),
        daemon_api::AiJobState::WaitingInteraction => Some(match parent_job_id {
            Some(parent_job_id) => format!(
                "[backend job] nested job_id={} parent_job_id={} waiting for interaction\n",
                job_id, parent_job_id
            ),
            None => format!(
                "[backend job] nested job_id={} waiting for interaction\n",
                job_id
            ),
        }),
        daemon_api::AiJobState::Cancelled => Some(match parent_job_id {
            Some(parent_job_id) => format!(
                "[backend job] nested job_id={} parent_job_id={} cancelled\n",
                job_id, parent_job_id
            ),
            None => format!("[backend job] nested job_id={} cancelled\n", job_id),
        }),
        daemon_api::AiJobState::Running
        | daemon_api::AiJobState::Completed
        | daemon_api::AiJobState::Failed => None,
    }
}

fn nested_completion_notice(job_id: &str, exit_code: i32) -> Option<String> {
    if std::env::var_os("AISH_JOB_DEPTH").is_none() {
        return None;
    }
    Some(format!(
        "[backend job] finished nested job_id={} exit_code={}\n",
        job_id, exit_code
    ))
}

fn nested_failure_notice(job_id: &str, message: &str) -> Option<String> {
    if std::env::var_os("AISH_JOB_DEPTH").is_none() {
        return None;
    }
    Some(format!(
        "[backend job] nested job_id={} failed: {}\n",
        job_id, message
    ))
}

fn trim_for_notice(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out.replace('\n', "\\n")
}

fn nested_interaction_notice(job_id: &str, request: &AiInteractionRequest) -> Option<String> {
    if std::env::var_os("AISH_JOB_DEPTH").is_none() {
        return None;
    }
    Some(match request {
        AiInteractionRequest::Approval { prompt } => format!(
            "[backend job] nested job_id={} requests approval: {}\n",
            job_id,
            trim_for_notice(prompt, 80)
        ),
        AiInteractionRequest::Continue { prompt } => format!(
            "[backend job] nested job_id={} requests continue confirmation: {}\n",
            job_id,
            trim_for_notice(prompt, 80)
        ),
        AiInteractionRequest::SensitivePrompt { verbose_output } => format!(
            "[backend job] nested job_id={} requests sensitive-content decision: {}\n",
            job_id,
            trim_for_notice(verbose_output, 80)
        ),
    })
}

fn nested_auto_response_notice(job_id: &str, response: &AiInteractionResponse) -> Option<String> {
    if std::env::var_os("AISH_JOB_DEPTH").is_none() {
        return None;
    }
    Some(match response {
        AiInteractionResponse::Approval { approved } => format!(
            "[backend job] auto-resolved nested job_id={} approval={} because stdin is not a terminal\n",
            job_id, approved
        ),
        AiInteractionResponse::Continue { continue_ } => format!(
            "[backend job] auto-resolved nested job_id={} continue={} because stdin is not a terminal\n",
            job_id, continue_
        ),
        AiInteractionResponse::SensitivePrompt { choice } => format!(
            "[backend job] auto-resolved nested job_id={} sensitive_choice={:?} because stdin is not a terminal\n",
            job_id, choice
        ),
    })
}

#[cfg(test)]
fn format_lifecycle(
    job_id: &str,
    parent_job_id: Option<&str>,
    state: &daemon_api::AiJobState,
) -> String {
    let state = match state {
        daemon_api::AiJobState::Queued => "queued",
        daemon_api::AiJobState::Running => "running",
        daemon_api::AiJobState::WaitingInteraction => "waiting_interaction",
        daemon_api::AiJobState::Cancelled => "cancelled",
        daemon_api::AiJobState::Completed => "completed",
        daemon_api::AiJobState::Failed => "failed",
    };
    match parent_job_id {
        Some(parent_job_id) => format!(
            "[backend job] state={} job_id={} parent_job_id={}\n",
            state, job_id, parent_job_id
        ),
        None => format!("[backend job] state={} job_id={}\n", state, job_id),
    }
}

pub(crate) fn build_run_request(argv: &[std::ffi::OsString]) -> Result<AiRunRequest, Error> {
    let argv = argv
        .iter()
        .map(|arg| {
            arg.clone()
                .into_string()
                .map_err(|_| Error::invalid_argument("ai backend does not support non-UTF-8 argv"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.display().to_string());
    Ok(AiRunRequest {
        job_id: resolve_job_id(),
        parent_job_id: std::env::var("AISH_JOB_ID").ok(),
        nesting_depth: resolve_nesting_depth()?,
        max_nesting_depth: resolve_max_nesting_depth()?,
        argv,
        session_dir: std::env::var("AISH_SESSION").ok(),
        aish_home: std::env::var("AISH_HOME").ok(),
        frontend_tty: resolve_frontend_tty_path(),
        cwd,
    })
}

pub(crate) fn run_via_backend(request: AiRunRequest, verbose: bool) -> Result<i32, Error> {
    ensure_backend_available()?;
    let socket_path = daemon_api::default_socket_path();
    let mut stream = UnixStream::connect(&socket_path)
        .map_err(|e| backend_required_error(format!("connect {}: {}", socket_path.display(), e)))?;
    let expected_job_id = request.job_id.clone();
    let id = format!("run-ai-{}", expected_job_id);
    let req = Request {
        v: PROTOCOL_VERSION,
        id: id.clone(),
        op: RequestOp::RunAi { request },
    };
    write_frame_sync(&mut stream, &req).map_err(|e| backend_required_error(e.to_string()))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| Error::io_msg(format!("set backend read timeout: {}", e)))?;

    let mut sinks = StdEventSinkFactory::new(verbose).create_sinks();
    let interrupt = interrupt_flag();
    interrupt.store(false, Ordering::Relaxed);
    let mut cancel_sent = false;
    loop {
        if interrupt.load(Ordering::Relaxed) && !cancel_sent {
            let _ = send_cancel_request(&expected_job_id);
            cancel_sent = true;
        }
        let frame: AiStreamFrame = match read_frame_sync(&mut stream) {
            Ok(frame) => frame,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if cancel_sent {
                    continue;
                }
                continue;
            }
            Err(e) => return Err(backend_required_error(e.to_string())),
        };
        match frame {
            AiStreamFrame::Lifecycle {
                job_id,
                parent_job_id,
                state,
            } => {
                if job_id != expected_job_id {
                    return Err(backend_required_error(format!(
                        "received frame for unexpected job_id {}",
                        job_id
                    )));
                }
                if let Some(text) =
                    nested_lifecycle_notice(&job_id, parent_job_id.as_deref(), &state)
                {
                    eprint!("{}", text);
                    io::stderr()
                        .flush()
                        .map_err(|e| Error::io_msg(format!("flush stderr: {}", e)))?;
                }
            }
            AiStreamFrame::Event { job_id, event } => {
                if job_id != expected_job_id {
                    return Err(backend_required_error(format!(
                        "received frame for unexpected job_id {}",
                        job_id
                    )));
                }
                for sink in &mut sinks {
                    sink.on_event(&event)?;
                }
            }
            AiStreamFrame::TextOutput {
                job_id,
                stream,
                text,
            } => {
                if job_id != expected_job_id {
                    return Err(backend_required_error(format!(
                        "received frame for unexpected job_id {}",
                        job_id
                    )));
                }
                match stream {
                    daemon_api::OutputStream::Stdout => {
                        print!("{}", text);
                        io::stdout()
                            .flush()
                            .map_err(|e| Error::io_msg(format!("flush stdout: {}", e)))?;
                    }
                    daemon_api::OutputStream::Stderr => {
                        eprint!("{}", text);
                        io::stderr()
                            .flush()
                            .map_err(|e| Error::io_msg(format!("flush stderr: {}", e)))?;
                    }
                }
            }
            AiStreamFrame::InteractionRequest { job_id, request } => {
                if job_id != expected_job_id {
                    return Err(backend_required_error(format!(
                        "received frame for unexpected job_id {}",
                        job_id
                    )));
                }
                if let Some(text) = nested_interaction_notice(&job_id, &request) {
                    eprint!("{}", text);
                    io::stderr()
                        .flush()
                        .map_err(|e| Error::io_msg(format!("flush stderr: {}", e)))?;
                }
                let response = if let Some(response) = nested_non_interactive_response(
                    &request,
                    std::env::var_os("AISH_JOB_DEPTH").is_some(),
                    io::stdin().is_terminal(),
                ) {
                    if let Some(text) = nested_auto_response_notice(&job_id, &response) {
                        eprint!("{}", text);
                        io::stderr()
                            .flush()
                            .map_err(|e| Error::io_msg(format!("flush stderr: {}", e)))?;
                    }
                    response
                } else {
                    match request {
                        AiInteractionRequest::Approval { prompt } => {
                            let approved = if io::stdin().is_terminal() {
                                let approval =
                                    CliToolApproval::default().approve_unsafe_shell(&prompt)?;
                                matches!(approval, Approval::Approved)
                            } else {
                                prompt_approval(&prompt)?
                            };
                            AiInteractionResponse::Approval { approved }
                        }
                        AiInteractionRequest::Continue { prompt } => {
                            let continue_ = prompt_continue(&prompt)?;
                            AiInteractionResponse::Continue { continue_ }
                        }
                        AiInteractionRequest::SensitivePrompt { verbose_output } => {
                            let choice = prompt_sensitive_choice(&verbose_output)?;
                            AiInteractionResponse::SensitivePrompt { choice }
                        }
                    }
                };
                write_frame_sync(
                    &mut stream,
                    &AiClientFrame::InteractionResponse { job_id, response },
                )
                .map_err(|e| Error::io_msg(format!("interaction response: {}", e)))?;
            }
            AiStreamFrame::Completed { job_id, exit_code } => {
                if job_id != expected_job_id {
                    return Err(backend_required_error(format!(
                        "received frame for unexpected job_id {}",
                        job_id
                    )));
                }
                if let Some(text) = nested_completion_notice(&job_id, exit_code) {
                    eprint!("{}", text);
                    io::stderr()
                        .flush()
                        .map_err(|e| Error::io_msg(format!("flush stderr: {}", e)))?;
                }
                for sink in &mut sinks {
                    sink.on_end()?;
                }
                return Ok(exit_code);
            }
            AiStreamFrame::Failed { job_id, message } => {
                if job_id != expected_job_id {
                    return Err(backend_required_error(format!(
                        "received frame for unexpected job_id {}",
                        job_id
                    )));
                }
                if let Some(text) = nested_failure_notice(&job_id, &message) {
                    eprint!("{}", text);
                    io::stderr()
                        .flush()
                        .map_err(|e| Error::io_msg(format!("flush stderr: {}", e)))?;
                }
                for sink in &mut sinks {
                    let _ = sink.on_end();
                }
                return Err(Error::invalid_argument(message));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_run_request, ensure_backend_available, format_lifecycle, nested_completion_notice,
        nested_failure_notice, nested_interaction_notice, nested_lifecycle_notice,
        nested_non_interactive_response, trim_for_notice,
    };
    use daemon_api::AiJobState;
    use std::ffi::OsString;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock")
    }

    #[test]
    fn ensure_backend_available_respects_no_autostart_env() {
        let _guard = env_lock();
        // Point default socket to a non-existing path by relying on the actual default (when the
        // daemon is not running) and forcing NO_AUTOSTART so that we do not try to spawn `aish`.
        std::env::set_var("AISH_NO_AUTOSTART", "1");
        let err = ensure_backend_available().expect_err("expected backend_required_error");
        let msg = format!("{}", err);
        assert!(msg.contains("AISH backend is required for this ai command"));
        assert!(msg.contains("AISH_NO_AUTOSTART"));
        std::env::remove_var("AISH_NO_AUTOSTART");
    }

    #[test]
    fn build_run_request_captures_parent_job_id_from_env() {
        let _guard = env_lock();
        std::env::set_var("AISH_JOB_ID", "parent-job-1");
        std::env::set_var("AISH_JOB_DEPTH", "1");
        std::env::remove_var("AISH_MAX_BACKEND_JOB_DEPTH");
        let request =
            build_run_request(&[OsString::from("ai"), OsString::from("hello")]).expect("request");
        std::env::remove_var("AISH_JOB_ID");
        std::env::remove_var("AISH_JOB_DEPTH");

        assert!(!request.job_id.is_empty());
        assert_eq!(request.parent_job_id.as_deref(), Some("parent-job-1"));
        assert_eq!(request.nesting_depth, 1);
    }

    #[test]
    fn build_run_request_uses_explicit_job_id_override() {
        let _guard = env_lock();
        std::env::set_var("AISH_BACKEND_JOB_ID", "job-override-1");
        let request =
            build_run_request(&[OsString::from("ai"), OsString::from("hello")]).expect("request");
        std::env::remove_var("AISH_BACKEND_JOB_ID");
        assert_eq!(request.job_id, "job-override-1");
    }

    #[test]
    fn build_run_request_defaults_nesting_depth_to_zero() {
        let _guard = env_lock();
        std::env::remove_var("AISH_JOB_DEPTH");
        std::env::remove_var("AISH_MAX_BACKEND_JOB_DEPTH");
        let request =
            build_run_request(&[OsString::from("ai"), OsString::from("hello")]).expect("request");
        assert_eq!(request.nesting_depth, 0);
        assert_eq!(request.max_nesting_depth, None);
    }

    #[test]
    fn build_run_request_reads_max_nesting_depth_override() {
        let _guard = env_lock();
        std::env::set_var("AISH_MAX_BACKEND_JOB_DEPTH", "2");
        let request =
            build_run_request(&[OsString::from("ai"), OsString::from("hello")]).expect("request");
        std::env::remove_var("AISH_MAX_BACKEND_JOB_DEPTH");
        assert_eq!(request.max_nesting_depth, Some(2));
    }

    #[test]
    fn build_run_request_uses_frontend_tty_env_override() {
        let _guard = env_lock();
        std::env::set_var("AISH_FRONTEND_TTY", "/tmp/fake-tty");
        let request =
            build_run_request(&[OsString::from("ai"), OsString::from("hello")]).expect("request");
        std::env::remove_var("AISH_FRONTEND_TTY");
        assert_eq!(request.frontend_tty.as_deref(), Some("/tmp/fake-tty"));
    }

    #[test]
    fn format_lifecycle_includes_parent_when_present() {
        let text = format_lifecycle("job-1", Some("parent-1"), &AiJobState::WaitingInteraction);
        assert!(text.contains("job_id=job-1"));
        assert!(text.contains("parent_job_id=parent-1"));
        assert!(text.contains("waiting_interaction"));
    }

    #[test]
    fn nested_non_interactive_response_denies_without_tty() {
        let response = nested_non_interactive_response(
            &daemon_api::AiInteractionRequest::Approval {
                prompt: "approve?".to_string(),
            },
            true,
            false,
        );
        assert!(matches!(
            response,
            Some(daemon_api::AiInteractionResponse::Approval { approved: false })
        ));
    }

    #[test]
    fn nested_non_interactive_response_keeps_interactive_when_tty_exists() {
        let response = nested_non_interactive_response(
            &daemon_api::AiInteractionRequest::Continue {
                prompt: "continue?".to_string(),
            },
            true,
            true,
        );
        assert!(response.is_none());
    }

    #[test]
    fn nested_lifecycle_notice_includes_parent_for_queued() {
        let text = nested_lifecycle_notice("job-2", Some("job-1"), &daemon_api::AiJobState::Queued)
            .expect("queued notice");
        assert!(text.contains("started nested"));
        assert!(text.contains("job_id=job-2"));
        assert!(text.contains("parent_job_id=job-1"));
    }

    #[test]
    fn nested_completion_notice_is_none_without_nested_env() {
        let _guard = env_lock();
        std::env::remove_var("AISH_JOB_DEPTH");
        assert!(nested_completion_notice("job-2", 0).is_none());
        assert!(nested_failure_notice("job-2", "boom").is_none());
    }

    #[test]
    fn trim_for_notice_preserves_utf8_boundaries() {
        assert_eq!(trim_for_notice("こんにちは世界", 4), "こんにち...");
        assert_eq!(trim_for_notice("line1\nline2", 20), "line1\\nline2");
    }

    #[test]
    fn nested_interaction_notice_formats_prompt_preview() {
        let _guard = env_lock();
        std::env::set_var("AISH_JOB_DEPTH", "1");
        let text = nested_interaction_notice(
            "job-2",
            &daemon_api::AiInteractionRequest::Approval {
                prompt: "rm -rf /tmp/demo".to_string(),
            },
        )
        .expect("notice");
        std::env::remove_var("AISH_JOB_DEPTH");
        assert!(text.contains("requests approval"));
        assert!(text.contains("job_id=job-2"));
        assert!(text.contains("rm -rf /tmp/demo"));
    }
}
