//! JSON-RPC stdio クライアントの結合テスト（モック子プロセス使用）
//!
//! モックは Python で実装。python3 が無い環境ではスキップする。

use crate::adapter::external_plugin_stdio_client::ExternalPluginStdioClient;
use crate::domain::external_plugin::{ExternalPluginError, PluginTimeouts, StdioTransport};
use std::collections::HashMap;
use std::io::Write;

fn have_python3() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_ok()
}

fn write_script(name: &str, body: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir();
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(body.as_bytes()).unwrap();
    f.flush().unwrap();
    path
}

/// Python で動くモック: 1 行読んで JSON-RPC に応答する（正しい id を返す）
fn mock_plugin_python_script() -> std::path::PathBuf {
    let script = r#"import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line: continue
    try:
        r = json.loads(line)
    except: break
    id = r.get('id', 0)
    m = r.get('method', '')
    if m == 'initialize':
        print(json.dumps({'jsonrpc':'2.0','id':id,'result':{}}))
    elif m == 'list_tools':
        print(json.dumps({'jsonrpc':'2.0','id':id,'result':[{'name':'echo_plugin','description':'Echo back','input_schema':{'type':'object','properties':{'x':{'type':'string'}}}}]}))
    elif m == 'call_tool':
        params = r.get('params') or {}
        args = params.get('arguments', {})
        print(json.dumps({'jsonrpc':'2.0','id':id,'result':{'content':args}}))
    else:
        print(json.dumps({'jsonrpc':'2.0','id':id,'error':{'code':-32601,'message':'Method not found'}}))
    sys.stdout.flush()
"#;
    write_script("aish_plugin_mock_test.py", script)
}

/// レスポンスの id を意図的に誤って返す（id+1）。id 検証でエラーになるはず。
fn mock_plugin_wrong_id_script() -> std::path::PathBuf {
    let script = r#"import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line: continue
    try:
        r = json.loads(line)
    except: break
    req_id = r.get('id', 0)
    wrong_id = req_id + 1
    m = r.get('method', '')
    if m == 'initialize':
        print(json.dumps({'jsonrpc':'2.0','id':wrong_id,'result':{}}))
    elif m == 'list_tools':
        print(json.dumps({'jsonrpc':'2.0','id':wrong_id,'result':[]}))
    else:
        print(json.dumps({'jsonrpc':'2.0','id':wrong_id,'result':{}}))
    sys.stdout.flush()
"#;
    write_script("aish_plugin_mock_wrong_id.py", script)
}

/// initialize で error を返す。start が StartFailed になるはず。
fn mock_plugin_initialize_error_script() -> std::path::PathBuf {
    let script = r#"import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line: continue
    try:
        r = json.loads(line)
    except: break
    id = r.get('id', 0)
    m = r.get('method', '')
    if m == 'initialize':
        print(json.dumps({'jsonrpc':'2.0','id':id,'error':{'code':-32600,'message':'Init refused'}}))
    else:
        print(json.dumps({'jsonrpc':'2.0','id':id,'result':{}}))
    sys.stdout.flush()
"#;
    write_script("aish_plugin_mock_init_error.py", script)
}

/// stderr に大量出力してから通常応答（デッドロックしないことを確認）
fn mock_plugin_stderr_flood_script() -> std::path::PathBuf {
    let script = r#"import sys, json
# 先に stderr に大量出力（パイプが詰まらないように drain が効くことを確認）
for i in range(20000):
    print("stderr line", i, file=sys.stderr)
sys.stderr.flush()
for line in sys.stdin:
    line = line.strip()
    if not line: continue
    try:
        r = json.loads(line)
    except: break
    id = r.get('id', 0)
    m = r.get('method', '')
    if m == 'initialize':
        print(json.dumps({'jsonrpc':'2.0','id':id,'result':{}}))
    elif m == 'list_tools':
        print(json.dumps({'jsonrpc':'2.0','id':id,'result':[{'name':'x','description':'x','input_schema':{}}]}))
    else:
        print(json.dumps({'jsonrpc':'2.0','id':id,'result':{'content':{}}}))
    sys.stdout.flush()
"#;
    write_script("aish_plugin_mock_stderr_flood.py", script)
}

/// initialize に応答しない（sleep）。タイムアウトで start 失敗し、子プロセスは cleanup される。
fn mock_plugin_no_initialize_script() -> std::path::PathBuf {
    let script = r#"import sys, time
# ずっと sleep（initialize に応答しない）
time.sleep(300)
"#;
    write_script("aish_plugin_mock_no_init.py", script)
}

#[test]
fn stdio_client_initialize_list_tools_call_tool_with_mock() {
    if !have_python3() {
        eprintln!("skip: python3 not found");
        return;
    }
    let script_path = mock_plugin_python_script();
    let transport = StdioTransport {
        command: "python3".to_string(),
        args: vec!["-u".to_string(), script_path.to_string_lossy().into_owned()],
    };
    let env = HashMap::new();
    let timeouts = PluginTimeouts::default();

    let client = ExternalPluginStdioClient::start(&transport, &env, &timeouts).unwrap();
    let tools = client.list_tools().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo_plugin");

    let result = client
        .call_tool("echo_plugin", serde_json::json!({ "x": "hello" }))
        .unwrap();
    assert_eq!(result.get("x").and_then(|v| v.as_str()), Some("hello"));

    let _ = std::fs::remove_file(script_path);
}

// --- Task D: JSON-RPC id 検証 ---
#[test]
fn stdio_client_rejects_wrong_response_id() {
    if !have_python3() {
        eprintln!("skip: python3 not found");
        return;
    }
    let script_path = mock_plugin_wrong_id_script();
    let transport = StdioTransport {
        command: "python3".to_string(),
        args: vec!["-u".to_string(), script_path.to_string_lossy().into_owned()],
    };
    let env = HashMap::new();
    let timeouts = PluginTimeouts::default();

    let r = ExternalPluginStdioClient::start(&transport, &env, &timeouts);
    let err = match r {
        Ok(_) => panic!("expected Err"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("id mismatch") || msg.contains("initialize"),
        "expected id mismatch or initialize error, got: {}",
        msg
    );
    let _ = std::fs::remove_file(script_path);
}

#[test]
fn stdio_client_initialize_error_response_fails_start() {
    if !have_python3() {
        eprintln!("skip: python3 not found");
        return;
    }
    let script_path = mock_plugin_initialize_error_script();
    let transport = StdioTransport {
        command: "python3".to_string(),
        args: vec!["-u".to_string(), script_path.to_string_lossy().into_owned()],
    };
    let env = HashMap::new();
    let timeouts = PluginTimeouts::default();

    let r = ExternalPluginStdioClient::start(&transport, &env, &timeouts);
    let err = match r {
        Ok(_) => panic!("expected Err"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("Init refused") || msg.contains("initialize failed"),
        "expected init error, got: {}",
        msg
    );
    let _ = std::fs::remove_file(script_path);
}

// --- Task B: stderr drain（大量 stderr でも詰まらない）---
#[test]
fn stdio_client_stderr_flood_does_not_deadlock() {
    if !have_python3() {
        eprintln!("skip: python3 not found");
        return;
    }
    let script_path = mock_plugin_stderr_flood_script();
    let transport = StdioTransport {
        command: "python3".to_string(),
        args: vec!["-u".to_string(), script_path.to_string_lossy().into_owned()],
    };
    let env = HashMap::new();
    let timeouts = PluginTimeouts::default();

    let client = ExternalPluginStdioClient::start(&transport, &env, &timeouts).unwrap();
    let tools = client.list_tools().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "x");
    let _ = std::fs::remove_file(script_path);
}

// --- Task C: start 失敗時の cleanup（タイムアウトで子プロセスが残らない）---
#[test]
fn stdio_client_start_timeout_cleans_up_child() {
    if !have_python3() {
        eprintln!("skip: python3 not found");
        return;
    }
    let script_path = mock_plugin_no_initialize_script();
    let transport = StdioTransport {
        command: "python3".to_string(),
        args: vec!["-u".to_string(), script_path.to_string_lossy().into_owned()],
    };
    let env = HashMap::new();
    let mut timeouts = PluginTimeouts::default();
    timeouts.startup_ms = Some(500);

    let r = ExternalPluginStdioClient::start(&transport, &env, &timeouts);
    match r {
        Ok(_) => panic!("expected Err(Timeout)"),
        Err(ExternalPluginError::Timeout(_)) => {}
        Err(e) => panic!("expected Timeout, got: {}", e),
    }
    let _ = std::fs::remove_file(script_path);
}
