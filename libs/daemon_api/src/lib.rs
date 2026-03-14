//! aishd 用 RPC プロトコル型と length-delimited JSON フレーミング
//!
//! - 1 request 1 response、1接続で複数 request を送ってよい
//! - `run_ai` だけは request 後に server/client 双方向の frame 往復へ入る
//! - メッセージは u32 LE length-prefix + JSON bytes
//! - `run_ai` の状態遷移:
//!   1. client -> `RequestOp::RunAi`
//!   2. server -> `AiStreamFrame::{Lifecycle,Event,TextOutput,InteractionRequest}*`
//!   3. client は `InteractionRequest` ごとに同じ `job_id` の
//!      `AiClientFrame::InteractionResponse` を返す
//!   4. server -> `AiStreamFrame::{Completed|Failed}`

use common::domain::{EventEnvelope, EventEnvelopeWithoutSeq};
use common::sink::AgentEvent;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// プロトコルバージョン
pub const PROTOCOL_VERSION: u32 = 4;

/// フレームの最大バイト数（安全のため上限を設ける）
pub const MAX_FRAME_BYTES: usize = 256 * 1024;

/// RPC リクエスト（op ごとの payload を含む）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// プロトコルバージョン
    pub v: u32,
    /// クライアント生成の一意 ID（レスポンスと紐付ける）
    pub id: String,
    #[serde(flatten)]
    pub op: RequestOp,
}

/// RPC リクエストのペイロード
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum RequestOp {
    /// 生存確認
    #[serde(rename = "ping")]
    Ping {},
    /// 停止要求
    #[serde(rename = "stop")]
    Stop {},
    /// ai job のキャンセル要求
    #[serde(rename = "cancel_ai")]
    CancelAi { job_id: String },
    /// daemon が保持している active job 一覧
    #[serde(rename = "list_active_jobs")]
    ListActiveJobs {},
    /// events append
    #[serde(rename = "append")]
    Append {
        /// セッションディレクトリ（絶対パス想定）
        session_dir: String,
        /// EventEnvelope.session_id と一致させるセッション識別子
        session_id: String,
        /// seq 未採番のエンベロープ
        envelope: EventEnvelopeWithoutSeq,
    },
    /// derived 全再構築（index/snapshots）
    #[serde(rename = "rebuild_derived")]
    RebuildDerived {
        /// セッションディレクトリ（絶対パス想定）
        session_dir: String,
        /// セッション識別子（表示用）
        session_id: String,
    },
    /// derived 状態（last_applied_seq / dirty）
    #[serde(rename = "derived_status")]
    DerivedStatus {
        /// セッションディレクトリ（絶対パス想定）
        session_dir: String,
    },
    /// ai query/task/resume/dry-run を backend で実行
    #[serde(rename = "run_ai")]
    RunAi { request: AiRunRequest },
    /// aish read 系コマンドを backend で実行
    #[serde(rename = "run_aish_read")]
    RunAishRead { request: AishReadRequest },
    /// aish の短命な副作用コマンドを backend で実行
    #[serde(rename = "run_aish_write")]
    RunAishWrite { request: AishWriteRequest },
}

/// ai backend 実行リクエスト
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRunRequest {
    /// backend job 識別子
    pub job_id: String,
    /// 親 job 識別子（nested ai 用）
    pub parent_job_id: Option<String>,
    /// 親から見た nested 深さ（root=0）
    pub nesting_depth: u32,
    /// nested 深さ上限（未指定なら daemon default）
    pub max_nesting_depth: Option<u32>,
    /// "ai" を含む argv
    pub argv: Vec<String>,
    /// セッションディレクトリ（AISH_SESSION 相当）
    pub session_dir: Option<String>,
    /// AISH_HOME の明示値
    pub aish_home: Option<String>,
    /// frontend が直接対話できる TTY パス（nested prompt 用）
    pub frontend_tty: Option<String>,
    /// クライアントのカレントディレクトリ
    pub cwd: Option<String>,
}

/// aish backend 実行で必要なプロセス文脈
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AishBackendContext {
    pub cwd: Option<String>,
    pub aish_home: Option<String>,
    pub aish_session: Option<String>,
    pub home_dir: Option<String>,
    pub session_dir: Option<String>,
}

/// aish read 系コマンドを backend で実行するリクエスト
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AishReadRequest {
    MemoryList {
        context: AishBackendContext,
    },
    MemoryGet {
        context: AishBackendContext,
        ids: Vec<String>,
    },
    HistoryList {
        context: AishBackendContext,
        session_explicitly_specified: bool,
        all: bool,
        user_only: bool,
        assistant_only: bool,
    },
    HistoryGet {
        context: AishBackendContext,
        session_explicitly_specified: bool,
        ids: Vec<String>,
    },
    PluginsList {
        context: AishBackendContext,
    },
    ToolsList {
        context: AishBackendContext,
    },
}

/// aish の短命な副作用コマンドを backend で実行するリクエスト
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AishWriteRequest {
    MemoryRemove {
        context: AishBackendContext,
        ids: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AishToolsListResult {
    pub tool_ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiJobState {
    Queued,
    Running,
    WaitingInteraction,
    Cancelled,
    Completed,
    Failed,
}

/// ai backend から frontend へ送るストリームフレーム
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AiStreamFrame {
    Lifecycle {
        job_id: String,
        parent_job_id: Option<String>,
        state: AiJobState,
    },
    Event {
        job_id: String,
        event: AgentEvent,
    },
    TextOutput {
        job_id: String,
        stream: OutputStream,
        text: String,
    },
    InteractionRequest {
        job_id: String,
        request: AiInteractionRequest,
    },
    Completed {
        job_id: String,
        exit_code: i32,
    },
    Failed {
        job_id: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AiInteractionRequest {
    Approval { prompt: String },
    Continue { prompt: String },
    SensitivePrompt { verbose_output: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveChoiceValue {
    Allow,
    Deny,
    Mask,
}

/// ai frontend から backend へ返す対話応答フレーム
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AiClientFrame {
    InteractionResponse {
        job_id: String,
        response: AiInteractionResponse,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AiInteractionResponse {
    Approval { approved: bool },
    Continue { continue_: bool },
    SensitivePrompt { choice: SensitiveChoiceValue },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// RPC レスポンス（エラー / 成功をラップ）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response<T> {
    pub v: u32,
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorPayload>,
}

/// エラー詳細
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

/// ping レスポンスの result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {
    pub pid: u32,
}

/// stop レスポンスの result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopResult {
    pub pid: u32,
    pub stopping: bool,
}

/// cancel_ai レスポンスの result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelAiResult {
    pub job_id: String,
    pub found: bool,
    pub signaled: bool,
}

/// daemon が保持している active job の概要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveJobInfo {
    pub job_id: String,
    pub parent_job_id: Option<String>,
    pub state: AiJobState,
    pub session_dir: Option<String>,
}

/// append レスポンスの result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendResult {
    /// seq 採番済みのエンベロープ
    pub envelope: EventEnvelope,
}

/// rebuild_derived レスポンスの result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildDerivedResult {
    pub duration_ms: u64,
}

/// derived_status レスポンスの result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedStatusResult {
    pub last_applied_seq: u64,
    pub dirty: bool,
}

/// 1フレーム（length-prefix 付き JSON）を書き込む
pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let bytes =
        serde_json::to_vec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "frame too large: {} bytes (max {})",
                bytes.len(),
                MAX_FRAME_BYTES
            ),
        ));
    }
    let len = bytes.len() as u32;
    writer.write_u32_le(len).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

/// 1フレームを読み込み JSON にデコードする
pub async fn read_frame<R, T>(reader: &mut R) -> io::Result<T>
where
    R: AsyncRead + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let len = reader.read_u32_le().await? as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid frame length: {}", len),
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    let value =
        serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(value)
}

/// 1フレーム（length-prefix 付き JSON）を書き込む。同期待ちクライアント向け。
pub fn write_frame_sync<W, T>(writer: &mut W, value: &T) -> io::Result<()>
where
    W: Write,
    T: Serialize,
{
    let bytes =
        serde_json::to_vec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "frame too large: {} bytes (max {})",
                bytes.len(),
                MAX_FRAME_BYTES
            ),
        ));
    }
    let len = bytes.len() as u32;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

/// 1フレームを読み込み JSON にデコードする。同期待ちクライアント向け。
pub fn read_frame_sync<R, T>(reader: &mut R) -> io::Result<T>
where
    R: Read,
    T: for<'de> Deserialize<'de>,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid frame length: {}", len),
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    let value =
        serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(value)
}

/// デーモンソケットのデフォルトパス（AISH_DAEMON_SOCK または $XDG_RUNTIME_DIR/aish/aishd.sock / $TMPDIR）
pub fn default_socket_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("AISH_DAEMON_SOCK") {
        return std::path::PathBuf::from(p);
    }
    let base = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .or_else(|| std::env::var("TMPDIR").ok())
        .unwrap_or_else(|| "/tmp".to_string());
    std::path::PathBuf::from(base)
        .join("aish")
        .join("aishd.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_frame_roundtrip_for_ai_stream() {
        let frame = AiStreamFrame::Lifecycle {
            job_id: "job-1".to_string(),
            parent_job_id: Some("parent-1".to_string()),
            state: AiJobState::Running,
        };
        let mut buf = Vec::new();
        write_frame_sync(&mut buf, &frame).expect("write");
        let decoded: AiStreamFrame = read_frame_sync(&mut std::io::Cursor::new(buf)).expect("read");
        match decoded {
            AiStreamFrame::Lifecycle {
                job_id,
                parent_job_id,
                state,
            } => {
                assert_eq!(job_id, "job-1");
                assert_eq!(parent_job_id.as_deref(), Some("parent-1"));
                assert!(matches!(state, AiJobState::Running));
            }
            other => panic!("unexpected frame: {:?}", other),
        }
    }

    #[test]
    fn sync_frame_roundtrip_for_ai_client_frame() {
        let frame = AiClientFrame::InteractionResponse {
            job_id: "job-2".to_string(),
            response: AiInteractionResponse::Approval { approved: true },
        };
        let mut buf = Vec::new();
        write_frame_sync(&mut buf, &frame).expect("write");
        let decoded: AiClientFrame = read_frame_sync(&mut std::io::Cursor::new(buf)).expect("read");
        match decoded {
            AiClientFrame::InteractionResponse { job_id, response } => match response {
                AiInteractionResponse::Approval { approved } => {
                    assert_eq!(job_id, "job-2");
                    assert!(approved);
                }
                AiInteractionResponse::Continue { .. }
                | AiInteractionResponse::SensitivePrompt { .. } => panic!("unexpected frame"),
            },
        }
    }
}
