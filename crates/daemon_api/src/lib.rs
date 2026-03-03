//! aishd 用 RPC プロトコル型と length-delimited JSON フレーミング
//!
//! - Phase10 最小スコープ: append / ping のみ
//! - 1 request 1 response、1接続で複数 request を送ってよい
//! - メッセージは u32 LE length-prefix + JSON bytes

use common::domain::{EventEnvelope, EventEnvelopeWithoutSeq};
use serde::{Deserialize, Serialize};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// プロトコルバージョン
pub const PROTOCOL_VERSION: u32 = 1;

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
