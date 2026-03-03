//! daemon 経由で events append する EventAppender 実装（AISH_DAEMON=on または auto で使用）

use common::domain::{EventEnvelope, EventEnvelopeWithoutSeq, SessionDir};
use common::error::Error;
use common::ports::outbound::EventAppender;
use daemon_api::{
    read_frame, write_frame, AppendResult, Request, RequestOp, Response, PROTOCOL_VERSION,
};
use std::path::Path;
use tokio::net::UnixStream;

/// 単一ライタ daemon に RPC で append するクライアント
pub struct DaemonEventAppender {
    socket_path: std::path::PathBuf,
}

impl DaemonEventAppender {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    fn append_impl(
        &self,
        session_dir: &SessionDir,
        envelope: EventEnvelopeWithoutSeq,
    ) -> Result<EventEnvelope, Box<dyn std::error::Error + Send + Sync>> {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.append_async(session_dir, envelope))
    }

    async fn append_async(
        &self,
        session_dir: &SessionDir,
        envelope: EventEnvelopeWithoutSeq,
    ) -> Result<EventEnvelope, Box<dyn std::error::Error + Send + Sync>> {
        let mut stream = UnixStream::connect(&self.socket_path).await?;
        let session_dir_str = session_dir.as_ref().display().to_string();
        let id = format!(
            "append-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let req = Request {
            v: PROTOCOL_VERSION,
            id: id.clone(),
            op: RequestOp::Append {
                session_dir: session_dir_str,
                session_id: envelope.session_id.clone(),
                envelope: envelope.clone(),
            },
        };
        write_frame(&mut stream, &req).await?;
        let resp: Response<AppendResult> = read_frame(&mut stream).await?;
        if !resp.ok {
            let msg = resp
                .error
                .as_ref()
                .map(|e| e.message.clone())
                .unwrap_or_else(|| "unknown daemon error".to_string());
            return Err(std::io::Error::new(std::io::ErrorKind::Other, msg).into());
        }
        let result = resp.result.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "missing result")
        })?;
        Ok(result.envelope)
    }
}

impl EventAppender for DaemonEventAppender {
    fn append(
        &self,
        session_dir: &SessionDir,
        envelope: EventEnvelopeWithoutSeq,
    ) -> Result<EventEnvelope, Error> {
        self.append_impl(session_dir, envelope)
            .map_err(|e| Error::invalid_argument(e.to_string()))
    }
}
