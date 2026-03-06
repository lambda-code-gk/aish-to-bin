use crate::adapter::config_loader::{CliPolicyOverrides, StdConfigProvider};
use crate::domain::{ConfigSourceKind, PolicyConfig};
use crate::ports::outbound::ConfigProvider;
use common::adapter::{StdEnvResolver, StdFileSystem};

fn make_env(
    home: Option<&str>,
    aish_home: Option<&str>,
    current_dir: &std::path::Path,
) -> StdEnvResolver {
    use std::env;
    let _ = env::set_current_dir(current_dir);
    match home {
        Some(v) => env::set_var("HOME", v),
        None => env::remove_var("HOME"),
    }
    match aish_home {
        Some(v) => env::set_var("AISH_HOME", v),
        None => env::remove_var("AISH_HOME"),
    }
    StdEnvResolver
}

fn write_file(path: &std::path::Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// defaults -> user toml -> project toml -> env -> cli の優先順位が守られること
#[test]
fn test_policy_config_layering_precedence() {
    let old_home = std::env::var("HOME").ok();
    let old_aish = std::env::var("AISH_HOME").ok();
    let old_cwd = std::env::current_dir().unwrap();

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let aish_home = root.join("home");
    let project_dir = root.join("project");
    std::fs::create_dir_all(&aish_home).unwrap();
    std::fs::create_dir_all(&project_dir).unwrap();

    // user config.toml (under AISH_HOME/config/config.toml)
    let user_toml = r#"
schema_version = 1

[policy]
egress_sensitive_action = "mask"
"#;
    let user_path = aish_home.join("config").join("config.toml");
    write_file(&user_path, user_toml);

    // project .aish/config.toml
    let project_toml = r#"
schema_version = 1

[policy]
egress_sensitive_action = "deny"
"#;
    let project_cfg = project_dir.join(".aish").join("config.toml");
    write_file(&project_cfg, project_toml);

    // env
    std::env::set_var("AISH_EGRESS_SENSITIVE_ACTION", "allow");

    // CLI override
    let mut cli = CliPolicyOverrides::default();
    cli.egress_sensitive_action = Some("mask".to_string());

    let env_resolver = make_env(
        Some("/tmp/aish_test_home"),
        Some(aish_home.to_str().unwrap()),
        &project_dir,
    );
    let fs = StdFileSystem;
    let provider = StdConfigProvider::new(
        std::sync::Arc::new(env_resolver),
        std::sync::Arc::new(fs),
        project_dir.to_path_buf(),
        cli,
    );

    let cfg: PolicyConfig = provider
        .policy_config()
        .expect("policy_config must succeed");
    assert_eq!(cfg.egress_sensitive_action.value, "mask");
    assert_eq!(
        cfg.egress_sensitive_action.source.kind,
        ConfigSourceKind::CliFlag
    );

    // restore env
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
