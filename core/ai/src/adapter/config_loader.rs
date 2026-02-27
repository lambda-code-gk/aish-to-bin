use crate::adapter::config_toml::{parse_policy_toml, ParsedPolicyToml};
use crate::domain::{ConfigSource, ConfigSourceKind, PolicyConfig, Resolved};
use crate::ports::outbound::ConfigProvider;
use common::error::Error;
use common::ports::outbound::{EnvResolver, FileSystem};
use std::path::PathBuf;
use std::sync::Arc;

/// CLI からの policy 関連の上書き値（v0.6 ではまだフラグ未導入だが、設計だけ先に用意）
#[derive(Debug, Clone, Default)]
pub struct CliPolicyOverrides {
    pub egress_sensitive_action: Option<String>,
    pub egress_hard_cap_chars: Option<usize>,
    pub addons_sensitive_action: Option<String>,
    pub run_shell_mode: Option<String>,
    pub run_shell_allowlist_add: Vec<String>,
}

pub struct StdConfigProvider {
    env: Arc<dyn EnvResolver>,
    fs: Arc<dyn FileSystem>,
    cli: CliPolicyOverrides,
}

impl StdConfigProvider {
    pub fn new(env: Arc<dyn EnvResolver>, fs: Arc<dyn FileSystem>, cli: CliPolicyOverrides) -> Self {
        Self { env, fs, cli }
    }

    fn load_toml(&self, path: &PathBuf) -> Option<ParsedPolicyToml> {
        if !self.fs.exists(path) {
            return None;
        }
        let s = match self.fs.read_to_string(path.as_path()) {
            Ok(s) => s,
            Err(_) => return None,
        };
        parse_policy_toml(&s).ok()
    }

    fn schema_version_from(parsed: &ParsedPolicyToml) -> Option<u32> {
        parsed.schema_version
    }

    fn apply_parsed(
        cfg: &mut PolicyConfig,
        parsed: &ParsedPolicyToml,
        kind: ConfigSourceKind,
        ref_id: String,
    ) {
        let src = |suffix: &str| ConfigSource {
            kind: kind.clone(),
            ref_id: format!("{}:{}", ref_id, suffix),
        };
        if let Some(policy) = &parsed.policy {
            if let Some(v) = &policy.egress_sensitive_action {
                cfg.egress_sensitive_action = Resolved::new(v.to_lowercase(), src("policy.egress_sensitive_action"));
            }
            if let Some(v) = policy.egress_hard_cap_chars {
                cfg.egress_hard_cap_chars = Resolved::new(v, src("policy.egress_hard_cap_chars"));
            }
            if let Some(v) = &policy.addons_sensitive_action {
                cfg.addons_sensitive_action = Resolved::new(v.to_lowercase(), src("policy.addons_sensitive_action"));
            }
            if let Some(tools) = &policy.tools {
                if let Some(run) = &tools.run_shell {
                    if let Some(m) = &run.mode {
                        cfg.run_shell_mode = Resolved::new(m.to_lowercase(), src("policy.tools.run_shell.mode"));
                    }
                    if let Some(list) = &run.allowlist {
                        cfg.run_shell_allowlist =
                            Resolved::new(list.clone(), src("policy.tools.run_shell.allowlist"));
                    }
                }
            }
        }
    }

    fn apply_env(&self, cfg: &mut PolicyConfig) -> Result<(), Error> {
        // env: AISH_EGRESS_SENSITIVE_ACTION
        if let Ok(v) = std::env::var("AISH_EGRESS_SENSITIVE_ACTION") {
            let v = v.trim();
            if !v.is_empty() {
                cfg.egress_sensitive_action = Resolved::new(
                    v.to_lowercase(),
                    ConfigSource {
                        kind: ConfigSourceKind::Env,
                        ref_id: "AISH_EGRESS_SENSITIVE_ACTION".to_string(),
                    },
                );
            }
        }

        // env: AISH_EGRESS_HARD_CAP_CHARS
        if let Ok(v) = std::env::var("AISH_EGRESS_HARD_CAP_CHARS") {
            let v = v.trim();
            if !v.is_empty() {
                let parsed = v
                    .parse::<usize>()
                    .map_err(|e| Error::InvalidArgument(format!("AISH_EGRESS_HARD_CAP_CHARS: {}", e)))?;
                cfg.egress_hard_cap_chars = Resolved::new(
                    parsed,
                    ConfigSource {
                        kind: ConfigSourceKind::Env,
                        ref_id: "AISH_EGRESS_HARD_CAP_CHARS".to_string(),
                    },
                );
            }
        }

        // env: AISH_ADDONS_SENSITIVE_ACTION
        if let Ok(v) = std::env::var("AISH_ADDONS_SENSITIVE_ACTION") {
            let v = v.trim();
            if !v.is_empty() {
                cfg.addons_sensitive_action = Resolved::new(
                    v.to_lowercase(),
                    ConfigSource {
                        kind: ConfigSourceKind::Env,
                        ref_id: "AISH_ADDONS_SENSITIVE_ACTION".to_string(),
                    },
                );
            }
        }

        // env: AISH_RUN_SHELL_MODE
        if let Ok(v) = std::env::var("AISH_RUN_SHELL_MODE") {
            let v = v.trim();
            if !v.is_empty() {
                cfg.run_shell_mode = Resolved::new(
                    v.to_lowercase(),
                    ConfigSource {
                        kind: ConfigSourceKind::Env,
                        ref_id: "AISH_RUN_SHELL_MODE".to_string(),
                    },
                );
            }
        }

        // env: AISH_RUN_SHELL_ALLOWLIST (":" or "," 区切り)
        if let Ok(v) = std::env::var("AISH_RUN_SHELL_ALLOWLIST") {
            let v = v.trim();
            if !v.is_empty() {
                let norm = v.replace(':', ",");
                let items: Vec<String> = norm
                    .split(',')
                    .filter_map(|s| {
                        let t = s.trim();
                        if t.is_empty() {
                            None
                        } else {
                            Some(t.to_string())
                        }
                    })
                    .collect();
                if !items.is_empty() {
                    cfg.run_shell_allowlist = Resolved::new(
                        items,
                        ConfigSource {
                            kind: ConfigSourceKind::Env,
                            ref_id: "AISH_RUN_SHELL_ALLOWLIST".to_string(),
                        },
                    );
                }
            }
        }

        Ok(())
    }

    fn apply_cli(&self, cfg: &mut PolicyConfig) {
        let src = |flag: &str| ConfigSource {
            kind: ConfigSourceKind::CliFlag,
            ref_id: flag.to_string(),
        };

        if let Some(v) = &self.cli.egress_sensitive_action {
            cfg.egress_sensitive_action =
                Resolved::new(v.to_lowercase(), src("--policy.egress-sensitive-action"));
        }
        if let Some(v) = self.cli.egress_hard_cap_chars {
            cfg.egress_hard_cap_chars =
                Resolved::new(v, src("--policy.egress-hard-cap-chars"));
        }
        if let Some(v) = &self.cli.addons_sensitive_action {
            cfg.addons_sensitive_action =
                Resolved::new(v.to_lowercase(), src("--policy.addons-sensitive-action"));
        }
        if let Some(v) = &self.cli.run_shell_mode {
            cfg.run_shell_mode = Resolved::new(v.to_lowercase(), src("--policy.run-shell-mode"));
        }
        if !self.cli.run_shell_allowlist_add.is_empty() {
            let mut list = cfg.run_shell_allowlist.value.clone();
            for item in &self.cli.run_shell_allowlist_add {
                if !list.contains(item) {
                    list.push(item.clone());
                }
            }
            cfg.run_shell_allowlist =
                Resolved::new(list, src("--policy.run-shell-allowlist-add"));
        }
    }
}

impl ConfigProvider for StdConfigProvider {
    fn policy_config(&self) -> Result<PolicyConfig, Error> {
        let mut cfg = PolicyConfig::defaults();

        // user & project TOML
        let dirs = self.env.resolve_dirs()?;
        let user_path = dirs.config_dir.join("config.toml");
        let project_path = self
            .env
            .current_dir()?
            .join(".aish")
            .join("config.toml");

        let mut versions = Vec::new();
        if let Some(p) = self.load_toml(&user_path) {
            if let Some(v) = Self::schema_version_from(&p) {
                versions.push(v);
            }
            Self::apply_parsed(
                &mut cfg,
                &p,
                ConfigSourceKind::UserFile,
                user_path.to_string_lossy().into_owned(),
            );
        }
        if let Some(p) = self.load_toml(&project_path) {
            if let Some(v) = Self::schema_version_from(&p) {
                versions.push(v);
            }
            Self::apply_parsed(
                &mut cfg,
                &p,
                ConfigSourceKind::ProjectFile,
                project_path.to_string_lossy().into_owned(),
            );
        }

        // schema_version: 1 以外が混ざっていないかチェック
        if versions.iter().any(|v| *v != 1) {
            return Err(Error::InvalidArgument(
                "Unsupported policy config schema_version (only 1 is allowed)".to_string(),
            ));
        }

        // env
        self.apply_env(&mut cfg)?;
        // cli
        self.apply_cli(&mut cfg);

        Ok(cfg)
    }
}

