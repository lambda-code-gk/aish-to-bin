//! 外部プロセスと stdio で JSON-RPC 2.0 通信するクライアント
//!
//! 1 行 1 JSON。起動タイムアウト・呼び出しタイムアウト・応答サイズ制限・stderr 制限あり。
//! 外部プラグイン対応用（将来有効化予定）。
#![allow(dead_code)]

use crate::domain::external_plugin::{
    ExternalPluginError, ExternalToolCallResponse, ExternalToolDescriptor, PluginTimeouts,
    StdioTransport,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// 応答 1 行の最大バイト数（巨大 JSON で固まらないように）
const MAX_RESPONSE_LINE_BYTES: usize = 2 * 1024 * 1024;
/// stderr 末尾バッファの最大バイト数（無制限蓄積を防ぐ）
const STDERR_TAIL_MAX_BYTES: usize = 4096;

/// stderr の捨て読み＋末尾のみ保持（デッドロック防止・デバッグ用）。サイズ上限あり。
#[derive(Default)]
struct StderrCapture {
    total_bytes: AtomicUsize,
    tail: Mutex<Vec<u8>>,
}

impl StderrCapture {
    fn push(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.total_bytes
            .fetch_add(data.len(), std::sync::atomic::Ordering::Relaxed);
        let mut tail = match self.tail.lock() {
            Ok(t) => t,
            Err(_) => return,
        };
        tail.extend_from_slice(data);
        let n = tail.len();
        if n > STDERR_TAIL_MAX_BYTES {
            let drop_len = n - STDERR_TAIL_MAX_BYTES;
            tail.drain(0..drop_len);
        }
    }
}

/// JSON-RPC 2.0 リクエスト
#[derive(serde::Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 成功レスポンス（id 検証用に必須）
#[derive(serde::Deserialize)]
struct JsonRpcSuccess {
    id: serde_json::Value,
    result: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 エラーレスポンス（id 検証用に必須）
#[derive(serde::Deserialize)]
struct JsonRpcErrorPayload {
    code: i64,
    message: String,
}

#[derive(serde::Deserialize)]
struct JsonRpcErrorResponse {
    id: serde_json::Value,
    error: JsonRpcErrorPayload,
}

/// 1 行をパースした結果（success または RPC error）
enum JsonRpcParseResult {
    Success(serde_json::Value),
    RpcError { code: i64, message: String },
}

/// JSON-RPC の id を u64 に変換。数値または数値文字列のみ許可。
fn json_rpc_id_to_u64(v: &serde_json::Value) -> Option<u64> {
    match v {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.parse::<u64>().ok(),
        _ => None,
    }
}

/// レスポンス 1 行をパースし、expected_id と一致することを検証。不一致・parse 失敗は Err。
fn parse_json_rpc_response_line(
    line: &str,
    expected_id: u64,
) -> Result<JsonRpcParseResult, ExternalPluginError> {
    if let Ok(err_resp) = serde_json::from_str::<JsonRpcErrorResponse>(line) {
        let got = json_rpc_id_to_u64(&err_resp.id);
        if got != Some(expected_id) {
            return Err(ExternalPluginError::MalformedResponse(format!(
                "response id mismatch: expected {}, got {:?}",
                expected_id, err_resp.id
            )));
        }
        return Ok(JsonRpcParseResult::RpcError {
            code: err_resp.error.code,
            message: err_resp.error.message,
        });
    }
    let succ: JsonRpcSuccess = serde_json::from_str(line)
        .map_err(|e| ExternalPluginError::MalformedResponse(format!("parse response: {}", e)))?;
    let got = json_rpc_id_to_u64(&succ.id);
    if got != Some(expected_id) {
        return Err(ExternalPluginError::MalformedResponse(format!(
            "response id mismatch: expected {}, got {:?}",
            expected_id, succ.id
        )));
    }
    match succ.result {
        Some(r) => Ok(JsonRpcParseResult::Success(r)),
        None => Err(ExternalPluginError::MalformedResponse(
            "missing result".to_string(),
        )),
    }
}

/// spawn 後の子プロセスを kill + wait する。失敗時は主エラーを優先し副次情報として扱う。
fn cleanup_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// 起動済みプラグインプロセスと stdio で JSON-RPC 通信するクライアント
pub struct ExternalPluginStdioClient {
    child: Child,
    stdin: Mutex<Option<ChildStdin>>,
    /// Receiver は Sync でないため Mutex でラップして共有可能にする
    stdout_rx: Mutex<mpsc::Receiver<Result<String, ExternalPluginError>>>,
    next_id: std::sync::atomic::AtomicU64,
    call_timeout_ms: u64,
    /// stderr の捨て読み＋末尾バッファ（デッドロック防止・デバッグ用）
    #[allow(dead_code)]
    stderr_capture: Arc<StderrCapture>,
}

impl ExternalPluginStdioClient {
    /// プロセスを起動し、initialize まで完了させる。失敗時は Err、子プロセスは kill + wait する。
    pub fn start(
        transport: &StdioTransport,
        env: &std::collections::HashMap<String, String>,
        timeouts: &PluginTimeouts,
    ) -> Result<Self, ExternalPluginError> {
        let startup_ms = timeouts.startup_ms.unwrap_or(10_000);
        let call_ms = timeouts.call_ms.unwrap_or(30_000);

        let mut cmd = Command::new(&transport.command);
        cmd.args(&transport.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| ExternalPluginError::StartFailed(format!("spawn failed: {}", e)))?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            cleanup_child(&mut child);
            ExternalPluginError::StartFailed("stdin not captured".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            cleanup_child(&mut child);
            ExternalPluginError::StartFailed("stdout not captured".to_string())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            cleanup_child(&mut child);
            ExternalPluginError::StartFailed("stderr not captured".to_string())
        })?;

        let stderr_capture = Arc::new(StderrCapture::default());
        let cap = Arc::clone(&stderr_capture);
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut r = BufReader::new(stderr);
            while let Ok(n) = r.read(&mut buf) {
                if n == 0 {
                    break;
                }
                cap.push(&buf[..n]);
            }
        });

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if l.len() > MAX_RESPONSE_LINE_BYTES {
                            let _ = tx.send(Err(ExternalPluginError::MalformedResponse(
                                "response line too large".to_string(),
                            )));
                        } else {
                            let _ = tx.send(Ok(l));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(ExternalPluginError::MalformedResponse(format!(
                            "stdout read: {}",
                            e
                        ))));
                        break;
                    }
                }
            }
        });

        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "initialize".to_string(),
            params: Some(serde_json::json!({ "protocolVersion": "0.1" })),
        };
        let line = serde_json::to_string(&req)
            .map_err(|e| ExternalPluginError::StartFailed(format!("serialize: {}", e)))?;
        if let Err(e) = stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
        {
            cleanup_child(&mut child);
            return Err(ExternalPluginError::StartFailed(format!("write: {}", e)));
        }

        let response = rx.recv_timeout(Duration::from_millis(startup_ms));
        let line = match response {
            Err(_) => {
                cleanup_child(&mut child);
                return Err(ExternalPluginError::Timeout(format!(
                    "initialize timeout ({} ms)",
                    startup_ms
                )));
            }
            Ok(Err(e)) => {
                cleanup_child(&mut child);
                return Err(e);
            }
            Ok(Ok(l)) => l,
        };

        let parsed = match parse_json_rpc_response_line(&line, 1) {
            Ok(p) => p,
            Err(e) => {
                cleanup_child(&mut child);
                return Err(if line.is_empty() || line.starts_with('{') {
                    ExternalPluginError::StartFailed(format!("initialize response: {}", e))
                } else {
                    e
                });
            }
        };
        match parsed {
            JsonRpcParseResult::RpcError { code, message } => {
                cleanup_child(&mut child);
                return Err(ExternalPluginError::StartFailed(format!(
                    "initialize failed: JSON-RPC error {}: {}",
                    code, message
                )));
            }
            JsonRpcParseResult::Success(_) => {}
        }

        Ok(Self {
            child,
            stdin: Mutex::new(Some(stdin)),
            stdout_rx: Mutex::new(rx),
            next_id: std::sync::atomic::AtomicU64::new(2),
            call_timeout_ms: call_ms,
            stderr_capture,
        })
    }

    /// 1 リクエスト送信して 1 レスポンス受信（call_timeout_ms でタイムアウト）
    fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ExternalPluginError> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };
        let line = serde_json::to_string(&req)
            .map_err(|e| ExternalPluginError::MalformedResponse(format!("serialize: {}", e)))?;
        let mut guard = self
            .stdin
            .lock()
            .map_err(|_| ExternalPluginError::ToolCallFailed("stdin lock poisoned".to_string()))?;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| ExternalPluginError::ProcessExited("stdin closed".to_string()))?;
        stdin
            .write_all(line.as_bytes())
            .map_err(|e| ExternalPluginError::ToolCallFailed(format!("write: {}", e)))?;
        stdin
            .write_all(b"\n")
            .map_err(|e| ExternalPluginError::ToolCallFailed(format!("write newline: {}", e)))?;
        stdin
            .flush()
            .map_err(|e| ExternalPluginError::ToolCallFailed(format!("flush: {}", e)))?;

        let response = self
            .stdout_rx
            .lock()
            .map_err(|_| {
                ExternalPluginError::ToolCallFailed("stdout_rx lock poisoned".to_string())
            })?
            .recv_timeout(Duration::from_millis(self.call_timeout_ms))
            .map_err(|e| match e {
                mpsc::RecvTimeoutError::Timeout => {
                    ExternalPluginError::Timeout(format!("{} ms", self.call_timeout_ms))
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    ExternalPluginError::ProcessExited("stdout closed".to_string())
                }
            })?;
        let line = response?;

        match parse_json_rpc_response_line(&line, id)? {
            JsonRpcParseResult::RpcError { code, message } => {
                Err(ExternalPluginError::ToolCallFailed(format!(
                    "JSON-RPC error {}: {}",
                    code, message
                )))
            }
            JsonRpcParseResult::Success(v) => Ok(v),
        }
    }

    /// list_tools を呼び、ツール定義一覧を返す
    pub fn list_tools(&self) -> Result<Vec<ExternalToolDescriptor>, ExternalPluginError> {
        let value = self.request("list_tools", None)?;
        let arr = value.as_array().ok_or_else(|| {
            ExternalPluginError::ListToolsFailed("list_tools result must be array".to_string())
        })?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let desc: ExternalToolDescriptor =
                serde_json::from_value(item.clone()).map_err(|e| {
                    ExternalPluginError::ListToolsFailed(format!("tool descriptor: {}", e))
                })?;
            out.push(desc);
        }
        Ok(out)
    }

    /// call_tool を呼び、結果の content を返す
    pub fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, ExternalPluginError> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments,
        });
        let value = self.request("call_tool", Some(params))?;
        let resp: ExternalToolCallResponse = serde_json::from_value(value)
            .map_err(|e| ExternalPluginError::ToolCallFailed(format!("call_tool result: {}", e)))?;
        Ok(resp.content)
    }
}

impl Drop for ExternalPluginStdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_tools_parses_descriptors() {
        let desc = ExternalToolDescriptor {
            name: "browser.open".to_string(),
            description: "Open a URL".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "url": { "type": "string" } }
            }),
        };
        let value = serde_json::to_value(&desc).unwrap();
        let back: ExternalToolDescriptor = serde_json::from_value(value).unwrap();
        assert_eq!(back.name, "browser.open");
    }
}
