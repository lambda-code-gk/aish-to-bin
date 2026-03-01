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
    project_root: PathBuf,
    cli: CliPolicyOverrides,
}

impl StdConfigProvider {
    pub fn new(
        env: Arc<dyn EnvResolver>,
        fs: Arc<dyn FileSystem>,
        project_root: PathBuf,
        cli: CliPolicyOverrides,
    ) -> Self {
        Self {
            env,
            fs,
            project_root,
            cli,
        }
    }

    /// ファイルを読み、schema_version が 1 以外なら Err（fail-closed）。OK なら Parsed を返す。
    fn load_toml(&self, path: &PathBuf) -> Result<Option<ParsedPolicyToml>, Error> {
        if !self.fs.exists(path) {
            return Ok(None);
        }
        let s = match self.fs.read_to_string(path.as_path()) {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };
        let parsed = parse_policy_toml(&s)?;
        if let Some(v) = parsed.schema_version {
            if v != 1 {
                return Err(Error::InvalidArgument(format!(
                    "Unsupported policy config schema_version (only 1 is allowed, got {})",
                    v
                )));
            }
        }
        Ok(Some(parsed))
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
        if let Some(v) = parsed.schema_version {
            cfg.schema_version = Resolved::new(v, src("schema_version"));
        }
        if let Some(policy) = &parsed.policy {
            if let Some(v) = &policy.egress_sensitive_action {
                cfg.egress_sensitive_action =
                    Resolved::new(v.to_lowercase(), src("policy.egress_sensitive_action"));
            }
            if let Some(v) = policy.egress_hard_cap_chars {
                cfg.egress_hard_cap_chars = Resolved::new(v, src("policy.egress_hard_cap_chars"));
            }
            if let Some(v) = &policy.addons_sensitive_action {
                cfg.addons_sensitive_action =
                    Resolved::new(v.to_lowercase(), src("policy.addons_sensitive_action"));
            }
            if let Some(v) = &policy.tool_default_mode {
                cfg.tool_default_mode =
                    Resolved::new(v.to_lowercase(), src("policy.tool_default_mode"));
            }
            if let Some(map) = &policy.tool_mode_by_capability {
                let normalized: std::collections::HashMap<String, String> = map
                    .iter()
                    .map(|(k, v)| (k.to_lowercase(), v.to_lowercase()))
                    .collect();
                cfg.tool_mode_by_capability =
                    Resolved::new(normalized, src("policy.tool_mode_by_capability"));
            }
            if let Some(tools) = &policy.tools {
                let mut tool_modes = std::collections::HashMap::new();
                for (name, entry) in tools {
                    if let Some(m) = &entry.mode {
                        tool_modes.insert(name.clone(), m.to_lowercase());
                    }
                    if name == "run_shell" {
                        if let Some(m) = &entry.mode {
                            cfg.run_shell_mode =
                                Resolved::new(m.to_lowercase(), src("policy.tools.run_shell.mode"));
                        }
                        if let Some(list) = &entry.allowlist {
                            cfg.run_shell_allowlist =
                                Resolved::new(list.clone(), src("policy.tools.run_shell.allowlist"));
                        }
                    }
                }
                if !tool_modes.is_empty() {
                    cfg.tool_modes = Resolved::new(tool_modes, src("policy.tools"));
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
                let parsed = v.parse::<usize>().map_err(|e| {
                    Error::InvalidArgument(format!("AISH_EGRESS_HARD_CAP_CHARS: {}", e))
                })?;
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

        // env: AISH_TOOL_DEFAULT_MODE
        if let Ok(v) = std::env::var("AISH_TOOL_DEFAULT_MODE") {
            let v = v.trim().to_lowercase();
            if !v.is_empty() {
                cfg.tool_default_mode = Resolved::new(
                    v,
                    ConfigSource {
                        kind: ConfigSourceKind::Env,
                        ref_id: "AISH_TOOL_DEFAULT_MODE".to_string(),
                    },
                );
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
            cfg.egress_hard_cap_chars = Resolved::new(v, src("--policy.egress-hard-cap-chars"));
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
            cfg.run_shell_allowlist = Resolved::new(list, src("--policy.run-shell-allowlist-add"));
        }
    }

    fn validate_policy_values(cfg: &PolicyConfig) -> Result<(), Error> {
        let valid_sensitive = ["deny", "mask", "allow"];
        if !valid_sensitive.contains(&cfg.egress_sensitive_action.value.as_str()) {
            return Err(Error::InvalidArgument(format!(
                "policy.egress_sensitive_action must be one of deny|mask|allow, got '{}'",
                cfg.egress_sensitive_action.value
            )));
        }
        if !valid_sensitive.contains(&cfg.addons_sensitive_action.value.as_str()) {
            return Err(Error::InvalidArgument(format!(
                "policy.addons_sensitive_action must be one of deny|mask|allow, got '{}'",
                cfg.addons_sensitive_action.value
            )));
        }
        let valid_tool_mode = ["allow", "require_approval", "deny"];
        if !valid_tool_mode.contains(&cfg.run_shell_mode.value.as_str()) {
            return Err(Error::InvalidArgument(format!(
                "policy.tools.run_shell.mode must be one of allow|require_approval|deny, got '{}'",
                cfg.run_shell_mode.value
            )));
        }
        if !valid_tool_mode.contains(&cfg.tool_default_mode.value.as_str()) {
            return Err(Error::InvalidArgument(format!(
                "policy.tool_default_mode must be one of allow|require_approval|deny, got '{}'",
                cfg.tool_default_mode.value
            )));
        }
        for (cap, mode) in &cfg.tool_mode_by_capability.value {
            if !valid_tool_mode.contains(&mode.as_str()) {
                return Err(Error::InvalidArgument(format!(
                    "policy.tool_mode_by_capability.{} must be one of allow|require_approval|deny, got '{}'",
                    cap, mode
                )));
            }
        }
        for (name, mode) in &cfg.tool_modes.value {
            if !valid_tool_mode.contains(&mode.as_str()) {
                return Err(Error::InvalidArgument(format!(
                    "policy.tools.{}.mode must be one of allow|require_approval|deny, got '{}'",
                    name, mode
                )));
            }
        }
        Ok(())
    }
}

impl ConfigProvider for StdConfigProvider {
    fn policy_config(&self) -> Result<PolicyConfig, Error> {
        let mut cfg = PolicyConfig::defaults();

        // user & project TOML (schema_version は load_toml 内で fail-closed 検証済み)
        let dirs = self.env.resolve_dirs()?;
        let user_path = dirs.config_dir.join("config.toml");
        let project_path = self.project_root.join(".aish").join("config.toml");

        if let Some(p) = self.load_toml(&user_path)? {
            Self::apply_parsed(
                &mut cfg,
                &p,
                ConfigSourceKind::UserFile,
                user_path.to_string_lossy().into_owned(),
            );
        }
        if let Some(p) = self.load_toml(&project_path)? {
            Self::apply_parsed(
                &mut cfg,
                &p,
                ConfigSourceKind::ProjectFile,
                project_path.to_string_lossy().into_owned(),
            );
        }

        // env
        self.apply_env(&mut cfg)?;
        // cli
        self.apply_cli(&mut cfg);

        Self::validate_policy_values(&cfg)?;
        Ok(cfg)
    }
}
