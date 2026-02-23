//! 外部プロセスと stdio で JSON-RPC 2.0 通信するクライアント
//!
//! 1 行 1 JSON。起動タイムアウト・呼び出しタイムアウト・応答サイズ制限・stderr 制限あり。

use crate::domain::external_plugin::{
    ExternalPluginError, ExternalToolCallResponse, ExternalToolDescriptor, PluginTimeouts,
    StdioTransport,
};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// 応答 1 行の最大バイト数（巨大 JSON で固まらないように）
const MAX_RESPONSE_LINE_BYTES: usize = 2 * 1024 * 1024;
/// stderr 収集の先頭・末尾それぞれの最大バイト数（将来イベント用）
const STDERR_HEAD_TAIL_BYTES: usize = 4096;

/// JSON-RPC 2.0 リクエスト
#[derive(serde::Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 成功レスポンス
#[derive(serde::Deserialize)]
struct JsonRpcSuccess {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[allow(dead_code)]
    id: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 エラーレスポンス
#[derive(serde::Deserialize)]
struct JsonRpcErrorPayload {
    code: i64,
    message: String,
}

#[derive(serde::Deserialize)]
struct JsonRpcErrorResponse {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[allow(dead_code)]
    id: Option<serde_json::Value>,
    error: JsonRpcErrorPayload,
}

/// 起動済みプラグインプロセスと stdio で JSON-RPC 通信するクライアント
pub struct ExternalPluginStdioClient {
    #[allow(dead_code)]
    child: Child,
    stdin: Mutex<Option<ChildStdin>>,
    /// Receiver は Sync でないため Mutex でラップして共有可能にする
    stdout_rx: Mutex<mpsc::Receiver<Result<String, ExternalPluginError>>>,
    next_id: std::sync::atomic::AtomicU64,
    call_timeout_ms: u64,
}

impl ExternalPluginStdioClient {
    /// プロセスを起動し、initialize まで完了させる。失敗時は Err、プロセスは kill される。
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
            ExternalPluginError::StartFailed("stdin not captured".to_string())
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ExternalPluginError::StartFailed("stdout not captured".to_string()))?;

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
                        let _ = tx.send(Err(ExternalPluginError::MalformedResponse(
                            format!("stdout read: {}", e),
                        )));
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
        stdin.write_all(line.as_bytes()).map_err(|e| {
            ExternalPluginError::StartFailed(format!("write: {}", e))
        })?;
        stdin.write_all(b"\n").map_err(|e| {
            ExternalPluginError::StartFailed(format!("write newline: {}", e))
        })?;
        stdin.flush().map_err(|e| {
            ExternalPluginError::StartFailed(format!("flush: {}", e))
        })?;

        let response = rx
            .recv_timeout(Duration::from_millis(startup_ms))
            .map_err(|_| {
                ExternalPluginError::Timeout(format!("initialize timeout ({} ms)", startup_ms))
            })?;
        let line = response?;
        let _: JsonRpcSuccess = serde_json::from_str(&line).map_err(|e| {
            ExternalPluginError::StartFailed(format!("initialize response: {}", e))
        })?;

        Ok(Self {
            child,
            stdin: Mutex::new(Some(stdin)),
            stdout_rx: Mutex::new(rx),
            next_id: std::sync::atomic::AtomicU64::new(2),
            call_timeout_ms: call_ms,
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
        let mut guard = self.stdin.lock().map_err(|_| {
            ExternalPluginError::ToolCallFailed("stdin lock poisoned".to_string())
        })?;
        let stdin = guard.as_mut().ok_or_else(|| {
            ExternalPluginError::ProcessExited("stdin closed".to_string())
        })?;
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
            .map_err(|_| ExternalPluginError::ToolCallFailed("stdout_rx lock poisoned".to_string()))?
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

        if let Ok(err_resp) = serde_json::from_str::<JsonRpcErrorResponse>(&line) {
            return Err(ExternalPluginError::ToolCallFailed(format!(
                "JSON-RPC error {}: {}",
                err_resp.error.code, err_resp.error.message
            )));
        }
        let succ: JsonRpcSuccess = serde_json::from_str(&line).map_err(|e| {
            ExternalPluginError::MalformedResponse(format!("parse response: {}", e))
        })?;
        succ.result.ok_or_else(|| {
            ExternalPluginError::MalformedResponse("missing result".to_string())
        })
    }

    /// list_tools を呼び、ツール定義一覧を返す
    pub fn list_tools(&self) -> Result<Vec<ExternalToolDescriptor>, ExternalPluginError> {
        let value = self.request("list_tools", None)?;
        let arr = value.as_array().ok_or_else(|| {
            ExternalPluginError::ListToolsFailed("list_tools result must be array".to_string())
        })?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let desc: ExternalToolDescriptor = serde_json::from_value(item.clone()).map_err(|e| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::external_plugin::PluginTimeouts;

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

