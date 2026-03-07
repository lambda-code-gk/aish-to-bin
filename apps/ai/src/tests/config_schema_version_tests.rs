use crate::adapter::{CliPolicyOverrides, StdConfigProvider};
use crate::ports::outbound::ConfigProvider;
use common::adapter::{StdEnvResolver, StdFileSystem};

fn write_file(path: &std::path::Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn test_schema_version_future_fails_closed() {
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
schema_version = 2

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

    let err = provider
        .policy_config()
        .expect_err("schema_version=2 must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("schema_version"),
        "error message should mention schema_version, got: {}",
        msg
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
