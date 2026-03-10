//! 外部プラグイン loader の結合テスト（Tool 名重複・list_tools 失敗時の register 防止）

use crate::adapter::plugin::external_plugin_loader::load_external_plugins;
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

/// Test A: 同一 Tool 名が複数プラグインから返ってきた場合は先勝ち、後続は登録されない
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

    // 2 つの plugin.toml を用意し、どちらも list_tools で "only_tool" を返す。
    // loader は Tool 名重複を検出し、最初のプラグインの Tool だけを登録する。
    // StdRuntimeCatalog の plugins 探索では config/plugins を UserConfig スコープとして参照するため、
    // そこに配置する。
    let plugin_dir = temp.join("config").join("plugins");
    let _ = std::fs::create_dir_all(&plugin_dir);
    let toml1 = format!(
        r#"id = "p1"
namespace = "p1"
command = "python3"
args = ["-u", "{}"]
enabled = true
"#,
        script_str
    );
    let toml2 = format!(
        r#"id = "p2"
namespace = "p2"
command = "python3"
args = ["-u", "{}"]
enabled = true
"#,
        script_str
    );
    std::fs::write(plugin_dir.join("01_p1.toml"), &toml1).unwrap();
    std::fs::write(plugin_dir.join("02_p2.toml"), &toml2).unwrap();

    let collected = Arc::new(Mutex::new(Vec::<EventRecord>::new()));
    let hub = EventHub::new(vec![Box::new(CollectingSink(Arc::clone(&collected)))]);
    let handle = EventHubHandle(Arc::new(Mutex::new(hub)));

    let old_home = std::env::var("HOME").ok();
    let old_aish_home = std::env::var("AISH_HOME").ok();
    std::env::set_var("HOME", temp.as_os_str());
    std::env::set_var("AISH_HOME", temp.as_os_str());
    // XDG_CONFIG_HOME は未設定として扱い、StdRuntimeCatalog の plugins 探索順は
    // project (.aish/plugins) のみになる。

    let tools = load_external_plugins(Some(handle));

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
        1,
        "duplicate tool name: only first plugin's tool is registered"
    );
    let kinds: Vec<String> = collected
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.kind.clone())
        .collect();
    assert!(
        kinds
            .iter()
            .any(|k| k == "external_plugin.tool_name_conflict"),
        "expected tool_name_conflict event, got: {:?}",
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
    let tools = load_external_plugins(None);
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
