use common::error::Error;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const MAX_RESPONSE_LINE_BYTES: usize = 2 * 1024 * 1024;
const STDERR_TAIL_MAX_BYTES: usize = 4096;
const MAX_TOOL_RESULT_BYTES: usize = 256 * 1024;

#[derive(Default)]
struct StderrCapture {
    tail: Mutex<Vec<u8>>,
}

impl StderrCapture {
    fn push(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let mut tail = match self.tail.lock() {
            Ok(t) => t,
            Err(_) => return,
        };
        tail.extend_from_slice(data);
        if tail.len() > STDERR_TAIL_MAX_BYTES {
            let drop_len = tail.len() - STDERR_TAIL_MAX_BYTES;
            tail.drain(0..drop_len);
        }
    }

    fn tail_string(&self) -> String {
        let tail = self
            .tail
            .lock()
            .ok()
            .map(|g| g.clone())
            .unwrap_or_default();
        String::from_utf8_lossy(&tail).to_string()
    }
}

#[derive(serde::Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(serde::Deserialize)]
struct JsonRpcSuccess {
    id: Value,
    result: Option<Value>,
}

#[derive(serde::Deserialize)]
struct JsonRpcErrorPayload {
    code: i64,
    message: String,
}

#[derive(serde::Deserialize)]
struct JsonRpcErrorResponse {
    id: Value,
    error: JsonRpcErrorPayload,
}

enum JsonRpcParseResult {
    Success(Value),
    RpcError { code: i64, message: String },
}

fn json_rpc_id_to_u64(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse::<u64>().ok(),
        _ => None,
    }
}

fn parse_json_rpc_response_line(line: &str, expected_id: u64) -> Result<JsonRpcParseResult, Error> {
    if let Ok(err_resp) = serde_json::from_str::<JsonRpcErrorResponse>(line) {
        let got = json_rpc_id_to_u64(&err_resp.id);
        if got != Some(expected_id) {
            return Err(Error::json(format!(
                "response id mismatch: expected {}, got {:?}",
                expected_id, err_resp.id
            )));
        }
        return Ok(JsonRpcParseResult::RpcError {
            code: err_resp.error.code,
            message: err_resp.error.message,
        });
    }
    let succ: JsonRpcSuccess =
        serde_json::from_str(line).map_err(|e| Error::json(format!("parse response: {}", e)))?;
    let got = json_rpc_id_to_u64(&succ.id);
    if got != Some(expected_id) {
        return Err(Error::json(format!(
            "response id mismatch: expected {}, got {:?}",
            expected_id, succ.id
        )));
    }
    match succ.result {
        Some(r) => Ok(JsonRpcParseResult::Success(r)),
        None => Err(Error::json("missing result".to_string())),
    }
}

fn cleanup_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub struct StdioJsonRpcClient {
    child: Child,
    stdin: Mutex<Option<ChildStdin>>,
    stdout_rx: Mutex<mpsc::Receiver<Result<String, Error>>>,
    next_id: std::sync::atomic::AtomicU64,
    call_timeout_ms: u64,
    stderr_capture: Arc<StderrCapture>,
}

impl StdioJsonRpcClient {
    pub fn start(
        command: &str,
        args: &[String],
        cwd: Option<&str>,
        env: &[(String, String)],
        startup_timeout_ms: u64,
        call_timeout_ms: u64,
    ) -> Result<Self, Error> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = cwd {
            if !dir.trim().is_empty() {
                cmd.current_dir(dir);
            }
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::system(format!("spawn failed: {}", e)))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::system("stdin not captured".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::system("stdout not captured".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::system("stderr not captured".to_string()))?;

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
                            let _ = tx.send(Err(Error::json("response line too large".to_string())));
                        } else {
                            let _ = tx.send(Ok(l));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(Error::json(format!("stdout read: {}", e))));
                        break;
                    }
                }
            }
        });

        // initialize
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "initialize".to_string(),
            params: Some(serde_json::json!({ "protocolVersion": "0.1" })),
        };
        let line = serde_json::to_string(&req).map_err(|e| Error::json(format!("serialize: {}", e)))?;
        if let Err(e) = stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
        {
            cleanup_child(&mut child);
            return Err(Error::system(format!("initialize write: {}", e)));
        }

        let response = rx.recv_timeout(Duration::from_millis(startup_timeout_ms));
        let line = match response {
            Err(_) => {
                cleanup_child(&mut child);
                return Err(Error::system(format!(
                    "initialize timeout ({} ms)",
                    startup_timeout_ms
                )));
            }
            Ok(Err(e)) => {
                cleanup_child(&mut child);
                return Err(e);
            }
            Ok(Ok(l)) => l,
        };
        match parse_json_rpc_response_line(&line, 1)? {
            JsonRpcParseResult::RpcError { code, message } => {
                cleanup_child(&mut child);
                return Err(Error::system(format!(
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
            call_timeout_ms,
            stderr_capture,
        })
    }

    fn request(&mut self, method: &str, params: Option<Value>, timeout_ms: Option<u64>) -> Result<Value, Error> {
        let call_timeout = timeout_ms.unwrap_or(self.call_timeout_ms);
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };
        let line = serde_json::to_string(&req).map_err(|e| Error::json(format!("serialize: {}", e)))?;
        let mut guard = self.stdin.lock().map_err(|_| Error::system("stdin lock poisoned".to_string()))?;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| Error::system("stdin closed".to_string()))?;
        stdin
            .write_all(line.as_bytes())
            .map_err(|e| Error::system(format!("write: {}", e)))?;
        stdin
            .write_all(b"\n")
            .map_err(|e| Error::system(format!("write newline: {}", e)))?;
        stdin
            .flush()
            .map_err(|e| Error::system(format!("flush: {}", e)))?;

        let start = Instant::now();
        let response = self
            .stdout_rx
            .lock()
            .map_err(|_| Error::system("stdout_rx lock poisoned".to_string()))?
            .recv_timeout(Duration::from_millis(call_timeout));

        let line = match response {
            Ok(r) => r?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // fail-closed: timeout したら子プロセスを kill
                cleanup_child(&mut self.child);
                *guard = None;
                return Err(Error::system(format!("timeout {} ms", call_timeout)));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                cleanup_child(&mut self.child);
                *guard = None;
                return Err(Error::system("stdout closed".to_string()));
            }
        };

        match parse_json_rpc_response_line(&line, id)? {
            JsonRpcParseResult::RpcError { code, message } => Err(Error::system(format!(
                "JSON-RPC error {}: {}",
                code, message
            ))),
            JsonRpcParseResult::Success(v) => {
                let _elapsed = start.elapsed();
                Ok(v)
            }
        }
    }

    pub fn list_tools(&mut self, timeout_ms: Option<u64>) -> Result<Vec<Value>, Error> {
        let value = self.request("list_tools", None, timeout_ms)?;
        let arr = value
            .as_array()
            .ok_or_else(|| Error::json("list_tools result must be array".to_string()))?;
        Ok(arr.to_vec())
    }

    pub fn call_tool(
        &mut self,
        name: &str,
        arguments: Value,
        timeout_ms: Option<u64>,
    ) -> Result<(Value, Option<String>, u64), Error> {
        let start = Instant::now();
        let params = serde_json::json!({ "name": name, "arguments": arguments });
        let value = self.request("call_tool", Some(params), timeout_ms)?;
        let content = value
            .get("content")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let bytes = serde_json::to_vec(&content)
            .map(|v| v.len())
            .unwrap_or(0);
        if bytes > MAX_TOOL_RESULT_BYTES {
            return Err(Error::system(format!(
                "tool output too large: {} bytes (cap {})",
                bytes, MAX_TOOL_RESULT_BYTES
            )));
        }
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let stderr_tail = self.stderr_capture.tail_string();
        let stderr_tail = if stderr_tail.trim().is_empty() {
            None
        } else {
            Some(stderr_tail)
        };
        Ok((content, stderr_tail, elapsed_ms))
    }
}

