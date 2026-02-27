//! LeakscanTextFilter のテスト

use crate::adapter::leakscan_text_filter::{LeakscanTextFilter, SensitiveAction};
use crate::domain::SensitiveFilterOutcome;
use crate::ports::outbound::SensitiveTextFilter;

#[cfg(unix)]
fn write_dummy_leakscan(path: &std::path::Path) {
    use std::io::Write;
    let script = r#"#!/bin/sh
case "$1" in
  -v)
    cat > /dev/null
    echo "HIT: sensitive data found"
    ;;
  --mask)
    cat > /dev/null
    echo "***MASKED***"
    ;;
  *)
    cat > /dev/null
    ;;
esac
"#;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(script.as_bytes()).unwrap();
    f.flush().unwrap();
    f.sync_all().unwrap();
    drop(f);
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

#[test]
#[cfg(unix)]
fn test_deny_action_returns_deny() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("leakscan");
    let rules = tmp.path().join("rules.json");
    std::fs::write(&rules, "{}").unwrap();
    write_dummy_leakscan(&script);

    let filter = LeakscanTextFilter::new(script, rules, SensitiveAction::Deny);
    let result = filter.filter("secret data").unwrap();
    match result {
        SensitiveFilterOutcome::Deny { verbose } => {
            assert!(verbose.contains("HIT"));
        }
        other => panic!("expected Deny, got {:?}", other),
    }
}

#[test]
#[cfg(unix)]
fn test_mask_action_returns_masked() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("leakscan");
    let rules = tmp.path().join("rules.json");
    std::fs::write(&rules, "{}").unwrap();
    write_dummy_leakscan(&script);

    let filter = LeakscanTextFilter::new(script, rules, SensitiveAction::Mask);
    let result = filter.filter("secret data").unwrap();
    match result {
        SensitiveFilterOutcome::Masked { masked, verbose } => {
            assert!(masked.contains("***MASKED***"));
            assert!(verbose.contains("HIT"));
        }
        other => panic!("expected Masked, got {:?}", other),
    }
}

#[test]
#[cfg(unix)]
fn test_allow_action_returns_clean_even_on_hit() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("leakscan");
    let rules = tmp.path().join("rules.json");
    std::fs::write(&rules, "{}").unwrap();
    write_dummy_leakscan(&script);

    let filter = LeakscanTextFilter::new(script, rules, SensitiveAction::Allow);
    let result = filter.filter("secret data").unwrap();
    assert_eq!(result, SensitiveFilterOutcome::Clean);
}
