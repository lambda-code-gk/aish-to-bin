//! JSON-RPC stdio クライアントの結合テスト（モック子プロセス使用）
//!
//! モックは Python で実装。python3 が無い環境ではスキップする。

use crate::adapter::external_plugin_stdio_client::ExternalPluginStdioClient;
use crate::domain::external_plugin::{PluginTimeouts, StdioTransport};
use std::collections::HashMap;
use std::io::Write;

fn have_python3() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_ok()
}

/// Python で動くモック: 1 行読んで JSON-RPC に応答する（一時ファイルに書き実行）
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
    let dir = std::env::temp_dir();
    let path = dir.join("aish_plugin_mock_test.py");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(script.as_bytes()).unwrap();
    f.flush().unwrap();
    path
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
