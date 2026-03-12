//! 責務: shell コマンドの許可/拒否判定ルールのみ。I/O を知らない。

use common::tool::{is_command_allowed, CommandAllowRule};

/// shell コマンドが allowlist で許可されているかを判定する純関数。
///
/// `profile_allowlist` (policy.toml 由来) と `command_allow_rules` (command_rules.txt 由来) の
/// いずれかに一致すれば許可。
pub fn is_shell_command_allowed(
    command: &str,
    profile_allowlist: &[String],
    command_allow_rules: &[CommandAllowRule],
) -> bool {
    let first_token = command.split_whitespace().next().unwrap_or("");

    let allowed_by_profile = if profile_allowlist.is_empty() {
        false
    } else {
        profile_allowlist
            .iter()
            .any(|prefix| first_token.starts_with(prefix) || first_token == prefix)
    };

    let allowed_by_command_rules = is_command_allowed(command, command_allow_rules);

    allowed_by_profile || allowed_by_command_rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_allowlist_matches_prefix() {
        assert!(is_shell_command_allowed(
            "echo hello",
            &["echo".to_string()],
            &[],
        ));
    }

    #[test]
    fn command_rules_match() {
        assert!(is_shell_command_allowed(
            "find . -maxdepth 1",
            &[],
            &[CommandAllowRule::Prefix("find".to_string())],
        ));
    }

    #[test]
    fn no_match_returns_false() {
        assert!(!is_shell_command_allowed("rm -rf /", &[], &[]));
    }

    #[test]
    fn either_source_suffices() {
        assert!(is_shell_command_allowed(
            "grep foo",
            &[],
            &[CommandAllowRule::Prefix("grep".to_string())],
        ));
        assert!(is_shell_command_allowed(
            "grep foo",
            &["grep".to_string()],
            &[],
        ));
    }
}
