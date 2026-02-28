//! Unix ドメインソケットでの aishd サーバ・クライアント
//!
//! Phase 10.1: append 成功後に derived 増分適用。rebuild_derived / derived_status RPC 対応。

use common::domain::{EventEnvelopeWithoutSeq, SessionDir};
use common::ports::outbound::EventAppender;
use daemon_api::{
    read_frame, write_frame, AppendResult, DerivedStatusResult, ErrorPayload, RebuildDerivedResult,
    MAX_FRAME_BYTES, PROTOCOL_VERSION, Request, RequestOp, Response,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use storage::{
    read_last_applied_seq, DerivedApplier, DerivedRebuilder, LocalEventAppender,
    NdjsonSessionEventStore,
};
use tokio::io::AsyncReadExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

/// ソケットパス（daemon_api と共通）
pub fn default_socket_path() -> PathBuf {
    daemon_api::default_socket_path()
}

/// サーバを起動し、接続ごとにリクエストを処理する（foreground、Ctrl-C で終了）
pub async fn run_server(socket_path: PathBuf) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let fs: Arc<dyn common::ports::outbound::FileSystem> = Arc::new(common::adapter::StdFileSystem);
    let store: Arc<dyn common::ports::outbound::SessionEventStore> =
        Arc::new(NdjsonSessionEventStore::new(Arc::clone(&fs)));
    let event_appender: Arc<dyn EventAppender> = Arc::new(LocalEventAppender::new(Arc::clone(&store)));
    let derived_applier: Arc<DerivedApplier> =
        Arc::new(DerivedApplier::new(Arc::clone(&store), Arc::clone(&fs)));
    let derived_rebuilder: Arc<DerivedRebuilder> =
        Arc::new(DerivedRebuilder::new(Arc::clone(&store), Arc::clone(&fs)));
    let apply_mutex: Arc<Mutex<()>> = Arc::new(Mutex::new(()));

    let parent = socket_path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent dir"))?;
    std::fs::create_dir_all(parent)?;
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path)?;
    let pid = std::process::id();

    loop {
        let (stream, _) = listener.accept().await?;
        let appender = Arc::clone(&event_appender);
        let applier = Arc::clone(&derived_applier);
        let rebuilder = Arc::clone(&derived_rebuilder);
        let mu = Arc::clone(&apply_mutex);
        tokio::spawn(async move {
            let _ = handle_connection(stream, appender, applier, rebuilder, mu, pid).await;
        });
    }
}

async fn handle_connection(
    mut stream: UnixStream,
    appender: Arc<dyn EventAppender>,
    derived_applier: Arc<DerivedApplier>,
    derived_rebuilder: Arc<DerivedRebuilder>,
    apply_mutex: Arc<Mutex<()>>,
    pid: u32,
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

        let resp: Response<serde_json::Value> = match &req.op {
            RequestOp::Ping {} => Response {
                v: PROTOCOL_VERSION,
                id: req.id.clone(),
                ok: true,
                result: Some(serde_json::json!({ "pid": pid })),
                error: None,
            },
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
                            result: Some(serde_json::to_value(AppendResult { envelope: with_seq })?),
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

        if let Err(e) = write_frame(&mut stream, &resp).await {
            let _ = e;
            break;
        }
    }
    Ok(())
}

/// ping を送り、応答があれば true
pub async fn run_ping(socket_path: &std::path::Path) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = UnixStream::connect(socket_path).await?;
    let id = format!("ping-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let req = Request {
        v: PROTOCOL_VERSION,
        id: id.clone(),
        op: RequestOp::Ping {},
    };
    write_frame(&mut stream, &req).await?;
    let len = stream.read_u32_le().await? as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Ok(false);
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let resp: Response<serde_json::Value> = serde_json::from_slice(&buf)?;
    Ok(resp.ok && resp.id == id)
}

/// status: ping と同様で、表示は CLI 側
pub async fn run_status(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ok = run_ping(socket_path).await?;
    if ok {
        println!("daemon is running");
    } else {
        println!("daemon is not responding");
    }
    Ok(())
}

/// rebuild_derived RPC を送り、成功時は結果を返す（main.rs の sessions サブコマンドで使用）
#[allow(dead_code)]
pub async fn run_rebuild_derived(
    socket_path: &std::path::Path,
    session_dir: &str,
    session_id: &str,
) -> Result<daemon_api::RebuildDerivedResult, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = UnixStream::connect(socket_path).await?;
    let id = format!(
        "rebuild-derived-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let req = Request {
        v: PROTOCOL_VERSION,
        id: id.clone(),
        op: RequestOp::RebuildDerived {
            session_dir: session_dir.to_string(),
            session_id: session_id.to_string(),
        },
    };
    write_frame(&mut stream, &req).await?;
    let len = stream.read_u32_le().await? as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid frame length",
        )
        .into());
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let resp: Response<daemon_api::RebuildDerivedResult> = serde_json::from_slice(&buf)?;
    if !resp.ok {
        let msg = resp
            .error
            .as_ref()
            .map(|e| format!("{}: {}", e.code, e.message))
            .unwrap_or_else(|| "unknown error".to_string());
        return Err(std::io::Error::new(std::io::ErrorKind::Other, msg).into());
    }
    resp.result
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no result").into())
}
