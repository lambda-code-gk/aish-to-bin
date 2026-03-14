//! 責務: frontend から daemon socket へ接続する client RPC を提供する。

use daemon_api::{
    write_frame, ActiveJobInfo, AishReadRequest, AishWriteRequest, CancelAiResult,
    RebuildDerivedResult, Request, RequestOp, Response, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};
use serde::de::DeserializeOwned;
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;

pub fn default_socket_path() -> std::path::PathBuf {
    daemon_api::default_socket_path()
}

pub async fn run_ping(
    socket_path: &std::path::Path,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = UnixStream::connect(socket_path).await?;
    let id = format!(
        "ping-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
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

pub async fn run_status(
    socket_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ok = run_ping(socket_path).await?;
    if ok {
        println!("daemon is running");
    } else {
        println!("daemon is not responding");
    }
    Ok(())
}

async fn run_request<T>(
    socket_path: &std::path::Path,
    op: RequestOp,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>>
where
    T: DeserializeOwned,
{
    let mut stream = UnixStream::connect(socket_path).await?;
    let id = format!(
        "req-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let req = Request {
        v: PROTOCOL_VERSION,
        id: id.clone(),
        op,
    };
    write_frame(&mut stream, &req).await?;
    let len = stream.read_u32_le().await? as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid frame length").into(),
        );
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let resp: Response<serde_json::Value> = serde_json::from_slice(&buf)?;
    if !resp.ok || resp.id != id {
        let msg = resp
            .error
            .as_ref()
            .map(|e| format!("{}: {}", e.code, e.message))
            .unwrap_or_else(|| "daemon request failed".to_string());
        return Err(std::io::Error::other(msg).into());
    }
    let value = resp
        .result
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "missing result"))?;
    Ok(serde_json::from_value(value)?)
}

pub async fn run_aish_read<T>(
    socket_path: &std::path::Path,
    request: AishReadRequest,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>>
where
    T: DeserializeOwned,
{
    run_request(socket_path, RequestOp::RunAishRead { request }).await
}

pub async fn run_aish_write(
    socket_path: &std::path::Path,
    request: AishWriteRequest,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = UnixStream::connect(socket_path).await?;
    let id = format!(
        "write-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let req = Request {
        v: PROTOCOL_VERSION,
        id: id.clone(),
        op: RequestOp::RunAishWrite { request },
    };
    write_frame(&mut stream, &req).await?;
    let len = stream.read_u32_le().await? as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid frame length").into(),
        );
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let resp: Response<serde_json::Value> = serde_json::from_slice(&buf)?;
    if !resp.ok || resp.id != id {
        let msg = resp
            .error
            .as_ref()
            .map(|e| format!("{}: {}", e.code, e.message))
            .unwrap_or_else(|| "daemon write request failed".to_string());
        return Err(std::io::Error::other(msg).into());
    }
    Ok(())
}

pub async fn run_stop(
    socket_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = UnixStream::connect(socket_path).await?;
    let id = format!(
        "stop-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let req = Request {
        v: PROTOCOL_VERSION,
        id: id.clone(),
        op: RequestOp::Stop {},
    };
    write_frame(&mut stream, &req).await?;
    let len = stream.read_u32_le().await? as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid frame length").into(),
        );
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let resp: Response<serde_json::Value> = serde_json::from_slice(&buf)?;
    if !resp.ok || resp.id != id {
        let msg = resp
            .error
            .as_ref()
            .map(|e| format!("{}: {}", e.code, e.message))
            .unwrap_or_else(|| "daemon stop failed".to_string());
        return Err(std::io::Error::other(msg).into());
    }
    for _ in 0..20 {
        if !socket_path.exists() {
            println!("daemon stopped");
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "daemon stop timed out waiting for socket cleanup",
    )
    .into())
}

pub async fn run_cancel_ai(
    socket_path: &std::path::Path,
    job_id: &str,
) -> Result<CancelAiResult, Box<dyn std::error::Error + Send + Sync>> {
    run_request(
        socket_path,
        RequestOp::CancelAi {
            job_id: job_id.to_string(),
        },
    )
    .await
}

pub async fn run_list_active_jobs(
    socket_path: &std::path::Path,
) -> Result<Vec<ActiveJobInfo>, Box<dyn std::error::Error + Send + Sync>> {
    run_request(socket_path, RequestOp::ListActiveJobs {}).await
}

pub async fn run_rebuild_derived(
    socket_path: &std::path::Path,
    session_dir: &str,
    session_id: &str,
) -> Result<RebuildDerivedResult, Box<dyn std::error::Error + Send + Sync>> {
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
        return Err(
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid frame length").into(),
        );
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let resp: Response<RebuildDerivedResult> = serde_json::from_slice(&buf)?;
    if !resp.ok {
        let msg = resp
            .error
            .as_ref()
            .map(|e| format!("{}: {}", e.code, e.message))
            .unwrap_or_else(|| "unknown error".to_string());
        return Err(std::io::Error::other(msg).into());
    }
    resp.result.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing result").into()
    })
}
