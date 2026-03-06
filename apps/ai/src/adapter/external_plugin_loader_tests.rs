//! 外部プラグイン loader の結合テスト（plugin_id 重複・list_tools 失敗時の register 防止）

use crate::adapter::external_plugin_loader::load_external_plugins;
use common::adapter::{StdEnvResolver, StdFileSystem};
use common::domain::event::EventRecord;
use common::event_hub::{EventHub, EventHubHandle};
use common::ports::outbound::EventRecordSink;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// テスト用: 受け取った EventRecord を蓄積する sink
struct CollectingSink(Arc<Mutex<Vec<EventRecord>>>);

impl EventRecordSink for CollectingSink {
    fn emit(&mut self, rec: &EventRecord) -> anyhow::Result<()> {
        self.0.lock().unwrap().push(rec.clone());
        Ok(())
    }
}

fn have_python3() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_ok()
}

/// 正常応答するモックプラグインの Python スクリプトを書き、パスを返す
fn write_working_mock_plugin(dir: &std::path::Path) -> PathBuf {
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
        print(json.dumps({'jsonrpc':'2.0','id':id,'result':[{'name':'only_tool','description':'x','input_schema':{}}]}))
    else:
        print(json.dumps({'jsonrpc':'2.0','id':id,'result':{'content':{}}}))
    sys.stdout.flush()
"#;
    let path = dir.join("mock_plugin.py");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(script.as_bytes()).unwrap();
    f.flush().unwrap();
    path
}

/// list_tools が配列でないものを返すモック（list_tools 失敗）
fn write_bad_list_tools_plugin(dir: &std::path::Path) -> PathBuf {
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
        print(json.dumps({'jsonrpc':'2.0','id':id,'result':'not-an-array'}))
    else:
        print(json.dumps({'jsonrpc':'2.0','id':id,'result':{}}))
    sys.stdout.flush()
"#;
    let path = dir.join("bad_list_tools_plugin.py");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(script.as_bytes()).unwrap();
    f.flush().unwrap();
    path
}

/// Test A: plugin_id 重複時は先勝ち、後続はスキップされ誤配送されない
#[test]
fn duplicate_plugin_id_skips_second_and_emits_event() {
    if !have_python3() {
        eprintln!("skip: python3 not found");
        return;
    }
    let temp = std::env::temp_dir().join(format!("aish_loader_dup_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(temp.join(".aish").join("plugins.d"));
    let script_path = write_working_mock_plugin(&temp);
    let script_str = script_path.to_string_lossy().into_owned();

    let yaml1 = format!(
        r#"id: same-id
version: "0.1.0"
transport:
  type: stdio
  command: python3
  args: ["-u", "{}"]
"#,
        script_str
    );
    let yaml2 = format!(
        r#"id: same-id
version: "0.1.0"
transport:
  type: stdio
  command: python3
  args: ["-u", "{}"]
"#,
        script_str
    );
    let plugins_d = temp.join(".aish").join("plugins.d");
    std::fs::write(plugins_d.join("01_first.yaml"), &yaml1).unwrap();
    std::fs::write(plugins_d.join("02_second.yaml"), &yaml2).unwrap();

    let collected = Arc::new(Mutex::new(Vec::<EventRecord>::new()));
    let hub = EventHub::new(vec![Box::new(CollectingSink(Arc::clone(&collected)))]);
    let handle = EventHubHandle(Arc::new(Mutex::new(hub)));

    let old_home = std::env::var("HOME").ok();
    let old_aish_home = std::env::var("AISH_HOME").ok();
    std::env::set_var("HOME", temp.as_os_str());
    std::env::set_var("AISH_HOME", temp.as_os_str());
    let config_plugins_d = temp.join("config").join("plugins.d");
    let _ = std::fs::create_dir_all(&config_plugins_d);
    std::fs::write(config_plugins_d.join("01_first.yaml"), &yaml1).unwrap();
    std::fs::write(config_plugins_d.join("02_second.yaml"), &yaml2).unwrap();

    let tools = load_external_plugins(
        Arc::new(StdFileSystem),
        Arc::new(StdEnvResolver),
        Some(handle),
    );

    if let Some(h) = old_aish_home {
        std::env::set_var("AISH_HOME", h);
    } else {
        std::env::remove_var("AISH_HOME");
    }
    if let Some(h) = old_home {
        std::env::set_var("HOME", h);
    } else {
        std::env::remove_var("HOME");
    }

    assert_eq!(tools.len(), 1, "duplicate id: only first plugin's tool");
    let kinds: Vec<String> = collected
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.kind.clone())
        .collect();
    assert!(
        kinds
            .iter()
            .any(|k| k == "external_plugin.skipped_id_conflict"),
        "expected skipped_id_conflict event, got: {:?}",
        kinds
    );

    let _ = std::fs::remove_dir_all(temp);
}

/// Test E: list_tools 失敗時は register されず、ツールが返らない
#[test]
fn list_tools_failure_does_not_register_plugin() {
    if !have_python3() {
        eprintln!("skip: python3 not found");
        return;
    }
    let temp =
        std::env::temp_dir().join(format!("aish_loader_bad_list_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(temp.join("config").join("plugins.d"));
    let script_path = write_bad_list_tools_plugin(&temp);
    let script_str = script_path.to_string_lossy().into_owned();

    let yaml = format!(
        r#"id: bad-list-tools
version: "0.1.0"
transport:
  type: stdio
  command: python3
  args: ["-u", "{}"]
"#,
        script_str
    );
    std::fs::write(
        temp.join("config").join("plugins.d").join("only.yaml"),
        &yaml,
    )
    .unwrap();

    let old_aish_home = std::env::var("AISH_HOME").ok();
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("AISH_HOME", temp.as_os_str());
    // テスト環境の $HOME に既存の ~/.aish/plugins.d があると結果が混ざるため隔離する
    let isolated_home = temp.join("home");
    let _ = std::fs::create_dir_all(&isolated_home);
    std::env::set_var("HOME", isolated_home.as_os_str());
    let tools = load_external_plugins(Arc::new(StdFileSystem), Arc::new(StdEnvResolver), None);
    if let Some(h) = old_aish_home {
        std::env::set_var("AISH_HOME", h);
    } else {
        std::env::remove_var("AISH_HOME");
    }
    if let Some(h) = old_home {
        std::env::set_var("HOME", h);
    } else {
        std::env::remove_var("HOME");
    }

    assert_eq!(
        tools.len(),
        0,
        "list_tools failure must not register any tool"
    );
    let _ = std::fs::remove_dir_all(temp);
}
