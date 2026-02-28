use crate::adapter::config_explain_provider::StdConfigExplainProvider;
use crate::adapter::config_loader::{CliPolicyOverrides, StdConfigProvider};
use crate::ports::outbound::ConfigExplainProvider;
use common::adapter::{StdEnvResolver, StdFileSystem};

fn write_file(path: &std::path::Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn test_config_explain_includes_sources() {
    let old_home = std::env::var("HOME").ok();
    let old_aish = std::env::var("AISH_HOME").ok();
    let old_cwd = std::env::current_dir().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let aish_home = root.join("home");
    let project_dir = root.join("project");
    std::fs::create_dir_all(&aish_home).unwrap();
    std::fs::create_dir_all(&project_dir).unwrap();

    let toml = r#"
schema_version = 1

[policy]
egress_sensitive_action = "mask"
"#;
    let user_path = aish_home.join("config").join("config.toml");
    write_file(&user_path, toml);

    // env setup
    std::env::set_var("HOME", "/tmp/aish_test_home");
    std::env::set_var("AISH_HOME", aish_home.to_str().unwrap());
    std::env::set_current_dir(&project_dir).unwrap();

    let env_resolver = StdEnvResolver;
    let fs = StdFileSystem;
    let provider = StdConfigProvider::new(
        std::sync::Arc::new(env_resolver),
        std::sync::Arc::new(fs),
        project_dir.to_path_buf(),
        CliPolicyOverrides::default(),
    );
    let explain = StdConfigExplainProvider::new(std::sync::Arc::new(provider));

    let info = explain.explain().expect("config explain must succeed");
    assert!(info.sources.len() >= 1);
    let keys: Vec<String> = info.sources.iter().map(|s| s.key.clone()).collect();
    assert!(
        keys.iter().any(|k| k == "policy.egress_sensitive_action"),
        "sources must contain policy.egress_sensitive_action"
    );

    match old_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
    match old_aish {
        Some(v) => std::env::set_var("AISH_HOME", v),
        None => std::env::remove_var("AISH_HOME"),
    }
    let _ = std::env::set_current_dir(old_cwd);
}
