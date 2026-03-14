//! 責務: aish 非依存の daemon server runtime を提供する。

use common::domain::{EventEnvelopeWithoutSeq, SessionDir};
use common::error::Error as CommonError;
use common::ports::outbound::EventAppender;
use daemon_api::{
    read_frame, read_frame_sync, write_frame, write_frame_sync, ActiveJobInfo, AiClientFrame,
    AiJobState, AiRunRequest, AiStreamFrame, AppendResult, CancelAiResult, DerivedStatusResult,
    ErrorPayload, RebuildDerivedResult, Request, RequestOp, Response, PROTOCOL_VERSION,
};
use std::collections::HashMap;
use std::io::BufReader;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::Instant;
use storage::{
    read_last_applied_seq, DerivedApplier, DerivedRebuilder, LocalEventAppender,
    NdjsonSessionEventStore,
};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::sync::Mutex;

type JsonHandler =
    Arc<dyn Fn(serde_json::Value) -> Result<serde_json::Value, CommonError> + Send + Sync>;

#[derive(Clone)]
pub struct ServerHandlers {
    pub run_aish_read: JsonHandler,
    pub run_aish_write: JsonHandler,
}

#[derive(Debug, Clone)]
struct ActiveJob {
    pid: u32,
    cancelled: bool,
    parent_job_id: Option<String>,
    session_dir: Option<String>,
    state: AiJobState,
}

type ActiveJobs = Arc<StdMutex<HashMap<String, ActiveJob>>>;

fn session_id_from_dir(session_dir: &SessionDir) -> String {
    session_dir
        .as_ref()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn append_job_lifecycle_event(
    event_appender: &Arc<dyn EventAppender>,
    derived_applier: &Arc<DerivedApplier>,
    apply_mutex: &Arc<Mutex<()>>,
    request: &AiRunRequest,
    state: &str,
    exit_code: Option<i32>,
) -> Result<(), CommonError> {
    let Some(session_dir) = request.session_dir.as_ref() else {
        return Ok(());
    };
    let session_dir = SessionDir::new(session_dir.clone());
    let session_id = session_id_from_dir(&session_dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let mut payload = serde_json::json!({
        "job_id": request.job_id,
        "parent_job_id": request.parent_job_id,
        "state": state,
    });
    if let Some(exit_code) = exit_code {
        payload["exit_code"] = serde_json::json!(exit_code);
    }
    let envelope = EventEnvelopeWithoutSeq {
        v: common::domain::EventEnvelope::SCHEMA_VERSION,
        ts_ms: ts.as_millis() as i64,
        session_id,
        run_id: Some(request.job_id.clone()),
        kind: "job.lifecycle".to_string(),
        payload,
    };
    event_appender.append(&session_dir, envelope)?;
    let _guard = apply_mutex.blocking_lock();
    let from_seq = read_last_applied_seq(&session_dir).saturating_add(1).max(1);
    let _ = derived_applier.apply_from_seq(&session_dir, from_seq);
    Ok(())
}

fn list_active_jobs(active_jobs: &ActiveJobs) -> Result<Vec<ActiveJobInfo>, CommonError> {
    let jobs = active_jobs
        .lock()
        .map_err(|_| CommonError::system("active jobs lock poisoned"))?;
    let mut entries = jobs
        .iter()
        .map(|(job_id, job)| ActiveJobInfo {
            job_id: job_id.clone(),
            parent_job_id: job.parent_job_id.clone(),
            state: job.state.clone(),
            session_dir: job.session_dir.clone(),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.job_id.cmp(&b.job_id));
    Ok(entries)
}

pub async fn run_server(
    socket_path: PathBuf,
    handlers: ServerHandlers,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let fs: Arc<dyn common::ports::outbound::FileSystem> = Arc::new(common::adapter::StdFileSystem);
    let store: Arc<dyn common::ports::outbound::SessionEventStore> =
        Arc::new(NdjsonSessionEventStore::new(Arc::clone(&fs)));
    let event_appender: Arc<dyn EventAppender> =
        Arc::new(LocalEventAppender::new(Arc::clone(&store)));
    let derived_applier: Arc<DerivedApplier> =
        Arc::new(DerivedApplier::new(Arc::clone(&store), Arc::clone(&fs)));
    let derived_rebuilder: Arc<DerivedRebuilder> =
        Arc::new(DerivedRebuilder::new(Arc::clone(&store), Arc::clone(&fs)));
    let apply_mutex: Arc<Mutex<()>> = Arc::new(Mutex::new(()));
    let active_jobs: ActiveJobs = Arc::new(StdMutex::new(HashMap::new()));

    let parent = socket_path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent dir"))?;
    std::fs::create_dir_all(parent)?;
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path)?;
    let pid = std::process::id();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    loop {
        let accept_result = tokio::select! {
            res = listener.accept() => Some(res),
            sig = tokio::signal::ctrl_c() => {
                sig?;
                None
            },
            changed = shutdown_rx.changed() => {
                changed?;
                if *shutdown_rx.borrow() { None } else { continue; }
            },
        };
        let Some((stream, _)) = accept_result.transpose()? else {
            break;
        };
        let appender = Arc::clone(&event_appender);
        let applier = Arc::clone(&derived_applier);
        let rebuilder = Arc::clone(&derived_rebuilder);
        let mu = Arc::clone(&apply_mutex);
        let active = Arc::clone(&active_jobs);
        let shutdown_signal = shutdown_tx.clone();
        let handlers = handlers.clone();
        tokio::spawn(async move {
            let _ = handle_connection(
                stream,
                appender,
                applier,
                rebuilder,
                mu,
                active,
                pid,
                shutdown_signal,
                handlers,
            )
            .await;
        });
    }
    let _ = std::fs::remove_file(&socket_path);
    Ok(())
}

async fn handle_connection(
    mut stream: UnixStream,
    appender: Arc<dyn EventAppender>,
    derived_applier: Arc<DerivedApplier>,
    derived_rebuilder: Arc<DerivedRebuilder>,
    apply_mutex: Arc<Mutex<()>>,
    active_jobs: ActiveJobs,
    pid: u32,
    shutdown: watch::Sender<bool>,
    handlers: ServerHandlers,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
        let req: Request = match read_frame(&mut stream).await {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                let _ = write_frame(
                    &mut stream,
                    &Response::<serde_json::Value> {
                        v: PROTOCOL_VERSION,
                        id: String::new(),
                        ok: false,
                        result: None,
                        error: Some(ErrorPayload {
                            code: "protocol".to_string(),
                            message: e.to_string(),
                        }),
                    },
                )
                .await;
                break;
            }
        };

        match req.op.clone() {
            RequestOp::RunAi { request } => {
                let std_stream = stream.into_std()?;
                std_stream.set_nonblocking(false)?;
                let active = Arc::clone(&active_jobs);
                let appender = Arc::clone(&appender);
                let applier = Arc::clone(&derived_applier);
                let apply_mu = Arc::clone(&apply_mutex);
                tokio::task::spawn_blocking(move || {
                    run_ai_via_worker(std_stream, request, active, appender, applier, apply_mu)
                })
                .await??;
                return Ok(());
            }
            RequestOp::CancelAi { job_id } => {
                let resp = match cancel_active_job(&active_jobs, &job_id) {
                    Ok((found, signaled)) => Response {
                        v: PROTOCOL_VERSION,
                        id: req.id.clone(),
                        ok: true,
                        result: Some(serde_json::to_value(CancelAiResult {
                            job_id,
                            found,
                            signaled,
                        })?),
                        error: None,
                    },
                    Err(e) => Response {
                        v: PROTOCOL_VERSION,
                        id: req.id.clone(),
                        ok: false,
                        result: None,
                        error: Some(ErrorPayload {
                            code: "cancel_ai_failed".to_string(),
                            message: e.to_string(),
                        }),
                    },
                };
                write_frame(&mut stream, &resp).await?;
                continue;
            }
            RequestOp::ListActiveJobs {} => {
                let resp = match list_active_jobs(&active_jobs) {
                    Ok(result) => Response {
                        v: PROTOCOL_VERSION,
                        id: req.id.clone(),
                        ok: true,
                        result: Some(serde_json::to_value(result)?),
                        error: None,
                    },
                    Err(e) => Response {
                        v: PROTOCOL_VERSION,
                        id: req.id.clone(),
                        ok: false,
                        result: None,
                        error: Some(ErrorPayload {
                            code: "list_active_jobs_failed".to_string(),
                            message: e.to_string(),
                        }),
                    },
                };
                write_frame(&mut stream, &resp).await?;
                continue;
            }
            RequestOp::RunAishRead { request } => {
                let handler = Arc::clone(&handlers.run_aish_read);
                let request = serde_json::to_value(request)?;
                let resp = match tokio::task::spawn_blocking(move || handler(request)).await? {
                    Ok(result) => Response {
                        v: PROTOCOL_VERSION,
                        id: req.id.clone(),
                        ok: true,
                        result: Some(result),
                        error: None,
                    },
                    Err(e) => Response {
                        v: PROTOCOL_VERSION,
                        id: req.id.clone(),
                        ok: false,
                        result: None,
                        error: Some(ErrorPayload {
                            code: "run_aish_read_failed".to_string(),
                            message: e.to_string(),
                        }),
                    },
                };
                write_frame(&mut stream, &resp).await?;
                continue;
            }
            RequestOp::RunAishWrite { request } => {
                let handler = Arc::clone(&handlers.run_aish_write);
                let request = serde_json::to_value(request)?;
                let resp = match tokio::task::spawn_blocking(move || handler(request)).await? {
                    Ok(result) => Response {
                        v: PROTOCOL_VERSION,
                        id: req.id.clone(),
                        ok: true,
                        result: Some(result),
                        error: None,
                    },
                    Err(e) => Response {
                        v: PROTOCOL_VERSION,
                        id: req.id.clone(),
                        ok: false,
                        result: None,
                        error: Some(ErrorPayload {
                            code: "run_aish_write_failed".to_string(),
                            message: e.to_string(),
                        }),
                    },
                };
                write_frame(&mut stream, &resp).await?;
                continue;
            }
            _ => {}
        }

        let resp: Response<serde_json::Value> = match &req.op {
            RequestOp::RunAi { .. }
            | RequestOp::CancelAi { .. }
            | RequestOp::ListActiveJobs {}
            | RequestOp::RunAishRead { .. }
            | RequestOp::RunAishWrite { .. } => unreachable!("handled above"),
            RequestOp::Ping {} => Response {
                v: PROTOCOL_VERSION,
                id: req.id.clone(),
                ok: true,
                result: Some(serde_json::json!({ "pid": pid })),
                error: None,
            },
            RequestOp::Stop {} => {
                let _ = shutdown.send(true);
                Response {
                    v: PROTOCOL_VERSION,
                    id: req.id.clone(),
                    ok: true,
                    result: Some(serde_json::json!({ "pid": pid, "stopping": true })),
                    error: None,
                }
            }
            RequestOp::Append {
                session_dir,
                session_id,
                envelope,
            } => {
                let session_dir_path = SessionDir::new(session_dir.clone());
                match appender.append(&session_dir_path, envelope.clone()) {
                    Ok(with_seq) => {
                        let session_dir_for_apply = session_dir_path.clone();
                        let applier = Arc::clone(&derived_applier);
                        let app = Arc::clone(&appender);
                        let sid = session_id.clone();
                        let apply_guard = apply_mutex.lock().await;
                        let from_seq = read_last_applied_seq(&session_dir_for_apply)
                            .saturating_add(1)
                            .max(1);
                        if let Err(e) = applier.apply_from_seq(&session_dir_for_apply, from_seq) {
                            drop(apply_guard);
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default();
                            let fail_envelope = EventEnvelopeWithoutSeq {
                                v: common::domain::EventEnvelope::SCHEMA_VERSION,
                                ts_ms: ts.as_millis() as i64,
                                session_id: sid,
                                run_id: None,
                                kind: "derived.update_failed".to_string(),
                                payload: serde_json::json!({
                                    "error_class": "apply_failed",
                                    "message": e.to_string(),
                                }),
                            };
                            let _ = app.append(&session_dir_for_apply, fail_envelope);
                        }
                        Response {
                            v: PROTOCOL_VERSION,
                            id: req.id.clone(),
                            ok: true,
                            result: Some(serde_json::to_value(AppendResult {
                                envelope: with_seq,
                            })?),
                            error: None,
                        }
                    }
                    Err(e) => Response {
                        v: PROTOCOL_VERSION,
                        id: req.id.clone(),
                        ok: false,
                        result: None,
                        error: Some(ErrorPayload {
                            code: "append_failed".to_string(),
                            message: e.to_string(),
                        }),
                    },
                }
            }
            RequestOp::RebuildDerived {
                session_dir,
                session_id: _,
            } => {
                let session_dir_path = SessionDir::new(session_dir.clone());
                let start = Instant::now();
                match derived_rebuilder.rebuild_all(&session_dir_path) {
                    Ok(()) => Response {
                        v: PROTOCOL_VERSION,
                        id: req.id.clone(),
                        ok: true,
                        result: Some(serde_json::to_value(RebuildDerivedResult {
                            duration_ms: start.elapsed().as_millis() as u64,
                        })?),
                        error: None,
                    },
                    Err(e) => Response {
                        v: PROTOCOL_VERSION,
                        id: req.id.clone(),
                        ok: false,
                        result: None,
                        error: Some(ErrorPayload {
                            code: "rebuild_derived_failed".to_string(),
                            message: e.to_string(),
                        }),
                    },
                }
            }
            RequestOp::DerivedStatus { session_dir } => {
                let session_dir_path = SessionDir::new(session_dir.clone());
                let last_applied_seq = read_last_applied_seq(&session_dir_path);
                Response {
                    v: PROTOCOL_VERSION,
                    id: req.id.clone(),
                    ok: true,
                    result: Some(serde_json::to_value(DerivedStatusResult {
                        last_applied_seq,
                        dirty: false,
                    })?),
                    error: None,
                }
            }
        };

        if let Err(_e) = write_frame(&mut stream, &resp).await {
            break;
        }
        if matches!(req.op, RequestOp::Stop {}) {
            break;
        }
    }
    Ok(())
}

fn run_ai_via_worker(
    mut client_stream: StdUnixStream,
    request: AiRunRequest,
    active_jobs: ActiveJobs,
    event_appender: Arc<dyn EventAppender>,
    derived_applier: Arc<DerivedApplier>,
    apply_mutex: Arc<Mutex<()>>,
) -> Result<(), CommonError> {
    fn write_client_frame(
        writer: &Arc<StdMutex<StdUnixStream>>,
        frame: &AiStreamFrame,
    ) -> Result<(), CommonError> {
        let mut guard = writer
            .lock()
            .map_err(|_| CommonError::system("client stream lock poisoned"))?;
        write_frame_sync(&mut *guard, frame).map_err(|e| CommonError::io_msg(e.to_string()))
    }

    let job_id = request.job_id.clone();
    let parent_job_id = request.parent_job_id.clone();
    let client_writer = Arc::new(StdMutex::new(
        client_stream
            .try_clone()
            .map_err(|e| CommonError::io_msg(e.to_string()))?,
    ));
    write_client_frame(
        &client_writer,
        &AiStreamFrame::Lifecycle {
            job_id: job_id.clone(),
            parent_job_id,
            state: AiJobState::Queued,
        },
    )
    .map_err(|e| CommonError::io_msg(format!("emit queued lifecycle: {}", e)))?;
    append_job_lifecycle_event(
        &event_appender,
        &derived_applier,
        &apply_mutex,
        &request,
        "queued",
        None,
    )?;
    if request
        .argv
        .iter()
        .any(|arg| arg == "-v" || arg == "--verbose")
    {
        let text = match request.parent_job_id.as_deref() {
            Some(parent_job_id) => format!(
                "[backend job] state=queued job_id={} parent_job_id={}\n",
                request.job_id, parent_job_id
            ),
            None => format!("[backend job] state=queued job_id={}\n", request.job_id),
        };
        write_client_frame(
            &client_writer,
            &AiStreamFrame::TextOutput {
                job_id: request.job_id.clone(),
                stream: daemon_api::OutputStream::Stderr,
                text,
            },
        )
        .map_err(|e| CommonError::io_msg(format!("emit queued lifecycle text: {}", e)))?;
    }
    let exe = std::env::current_exe().map_err(|e| CommonError::io_msg(e.to_string()))?;
    let mut child = Command::new(exe)
        .env("AISH_BACKEND_WORKER", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CommonError::io_msg(format!("spawn backend worker: {}", e)))?;
    let child_pid = child.id();
    {
        let mut jobs = active_jobs
            .lock()
            .map_err(|_| CommonError::system("active jobs lock poisoned"))?;
        jobs.insert(
            request.job_id.clone(),
            ActiveJob {
                pid: child_pid,
                cancelled: false,
                parent_job_id: request.parent_job_id.clone(),
                session_dir: request.session_dir.clone(),
                state: AiJobState::Queued,
            },
        );
    }

    let mut worker_writer = child
        .stdin
        .take()
        .ok_or_else(|| CommonError::system("worker stdin unavailable"))?;
    write_frame_sync(&mut worker_writer, &request)
        .map_err(|e| CommonError::io_msg(e.to_string()))?;

    let stderr_stream = child
        .stderr
        .take()
        .ok_or_else(|| CommonError::system("worker stderr unavailable"))?;
    let mut stderr_reader = BufReader::new(stderr_stream);
    let client_stderr = Arc::clone(&client_writer);
    let stderr_job_id = job_id.clone();
    let stderr_thread = thread::spawn(move || -> Result<(), CommonError> {
        loop {
            let mut buf = [0u8; 4096];
            let n = std::io::Read::read(&mut stderr_reader, &mut buf)
                .map_err(|e| CommonError::io_msg(format!("read worker stderr: {}", e)))?;
            if n == 0 {
                return Ok(());
            }
            let text = String::from_utf8_lossy(&buf[..n]).to_string();
            write_client_frame(
                &client_stderr,
                &AiStreamFrame::TextOutput {
                    job_id: stderr_job_id.clone(),
                    stream: daemon_api::OutputStream::Stderr,
                    text,
                },
            )
            .map_err(|e| CommonError::io_msg(format!("forward worker stderr: {}", e)))?;
        }
    });

    let stdout_stream = child
        .stdout
        .take()
        .ok_or_else(|| CommonError::system("worker stdout unavailable"))?;
    let mut worker_reader = BufReader::new(stdout_stream);
    let saw_terminal_frame = loop {
        let frame: AiStreamFrame = match read_frame_sync(&mut worker_reader) {
            Ok(frame) => frame,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break false,
            Err(e) => return Err(CommonError::io_msg(e.to_string())),
        };
        write_client_frame(&client_writer, &frame)
            .map_err(|e| CommonError::io_msg(e.to_string()))?;
        if let AiStreamFrame::Lifecycle { state, .. } = &frame {
            {
                let mut jobs = active_jobs
                    .lock()
                    .map_err(|_| CommonError::system("active jobs lock poisoned"))?;
                if let Some(job) = jobs.get_mut(&request.job_id) {
                    job.state = state.clone();
                }
            }
            let state = match state {
                AiJobState::Queued => Some("queued"),
                AiJobState::Running => Some("running"),
                AiJobState::WaitingInteraction => Some("waiting_interaction"),
                AiJobState::Cancelled => Some("cancelled"),
                AiJobState::Completed | AiJobState::Failed => None,
            };
            if let Some(state) = state {
                append_job_lifecycle_event(
                    &event_appender,
                    &derived_applier,
                    &apply_mutex,
                    &request,
                    state,
                    None,
                )?;
            }
        } else if let AiStreamFrame::Completed { exit_code, .. } = &frame {
            append_job_lifecycle_event(
                &event_appender,
                &derived_applier,
                &apply_mutex,
                &request,
                "completed",
                Some(*exit_code),
            )?;
        } else if let AiStreamFrame::Failed { .. } = &frame {
            append_job_lifecycle_event(
                &event_appender,
                &derived_applier,
                &apply_mutex,
                &request,
                "failed",
                None,
            )?;
        }
        if let AiStreamFrame::InteractionRequest { .. } = frame {
            let response: AiClientFrame = read_frame_sync(&mut client_stream)
                .map_err(|e| CommonError::io_msg(e.to_string()))?;
            write_frame_sync(&mut worker_writer, &response)
                .map_err(|e| CommonError::io_msg(e.to_string()))?;
            continue;
        }
        if matches!(
            frame,
            AiStreamFrame::Completed { .. } | AiStreamFrame::Failed { .. }
        ) {
            break true;
        }
    };

    let status = child
        .wait()
        .map_err(|e| CommonError::io_msg(format!("wait backend worker: {}", e)))?;
    stderr_thread
        .join()
        .map_err(|_| CommonError::system("worker stderr thread panicked"))??;
    let was_cancelled = {
        let mut jobs = active_jobs
            .lock()
            .map_err(|_| CommonError::system("active jobs lock poisoned"))?;
        jobs.remove(&request.job_id)
            .map(|job| job.cancelled)
            .unwrap_or(false)
    };

    if saw_terminal_frame {
        Ok(())
    } else if was_cancelled {
        write_client_frame(
            &client_writer,
            &AiStreamFrame::Lifecycle {
                job_id: request.job_id.clone(),
                parent_job_id: request.parent_job_id.clone(),
                state: AiJobState::Cancelled,
            },
        )
        .map_err(|e| CommonError::io_msg(format!("emit cancelled lifecycle: {}", e)))?;
        append_job_lifecycle_event(
            &event_appender,
            &derived_applier,
            &apply_mutex,
            &request,
            "cancelled",
            Some(130),
        )?;
        write_client_frame(
            &client_writer,
            &AiStreamFrame::Completed {
                job_id: request.job_id.clone(),
                exit_code: 130,
            },
        )
        .map_err(|e| CommonError::io_msg(format!("emit cancelled completion: {}", e)))?;
        Ok(())
    } else {
        Err(CommonError::io_msg(format!(
            "backend worker exited with status {}",
            status
        )))
    }
}

fn collect_descendant_job_ids(jobs: &HashMap<String, ActiveJob>, root_job_id: &str) -> Vec<String> {
    let mut ordered = Vec::new();
    let mut stack = vec![root_job_id.to_string()];
    while let Some(current) = stack.pop() {
        ordered.push(current.clone());
        let mut children = jobs
            .iter()
            .filter_map(|(job_id, job)| {
                (job.parent_job_id.as_deref() == Some(current.as_str())).then_some(job_id.clone())
            })
            .collect::<Vec<_>>();
        children.sort();
        children.reverse();
        stack.extend(children);
    }
    ordered
}

fn cancel_active_job(active_jobs: &ActiveJobs, job_id: &str) -> Result<(bool, bool), CommonError> {
    let targets = {
        let mut jobs = active_jobs
            .lock()
            .map_err(|_| CommonError::system("active jobs lock poisoned"))?;
        if !jobs.contains_key(job_id) {
            return Ok((false, false));
        }
        let ordered_job_ids = collect_descendant_job_ids(&jobs, job_id);
        let mut targets = Vec::new();
        for target_job_id in ordered_job_ids {
            let job = jobs
                .get_mut(&target_job_id)
                .expect("target job should still exist while lock is held");
            job.cancelled = true;
            job.state = AiJobState::Cancelled;
            targets.push((target_job_id, job.pid));
        }
        targets
    };

    let mut any_signal_sent = false;
    for (target_job_id, pid) in targets {
        let rc = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        if rc == 0 {
            any_signal_sent = true;
            continue;
        }
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            continue;
        }
        return Err(CommonError::io_msg(format!(
            "cancel job {} with pid {}: {}",
            target_job_id, pid, err
        )));
    }
    Ok((true, any_signal_sent))
}
