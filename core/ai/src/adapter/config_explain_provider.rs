use crate::domain::{ConfigExplainInfo, ConfigKeySource, PolicyConfig};
use crate::ports::outbound::{ConfigExplainProvider, ConfigProvider};
use common::error::Error;
use std::sync::Arc;

pub struct StdConfigExplainProvider {
    provider: Arc<dyn ConfigProvider>,
}

impl StdConfigExplainProvider {
    pub fn new(provider: Arc<dyn ConfigProvider>) -> Self {
        Self { provider }
    }

    fn build_sources(&self, cfg: &PolicyConfig) -> Vec<ConfigKeySource> {
        let mut out = Vec::new();

        out.push(ConfigKeySource {
            key: "schema_version".to_string(),
            value_preview: cfg.schema_version.value.to_string(),
            source: cfg.schema_version.source.clone(),
        });
        out.push(ConfigKeySource {
            key: "policy.egress_sensitive_action".to_string(),
            value_preview: cfg.egress_sensitive_action.value.clone(),
            source: cfg.egress_sensitive_action.source.clone(),
        });
        out.push(ConfigKeySource {
            key: "policy.egress_hard_cap_chars".to_string(),
            value_preview: cfg.egress_hard_cap_chars.value.to_string(),
            source: cfg.egress_hard_cap_chars.source.clone(),
        });
        out.push(ConfigKeySource {
            key: "policy.addons_sensitive_action".to_string(),
            value_preview: cfg.addons_sensitive_action.value.clone(),
            source: cfg.addons_sensitive_action.source.clone(),
        });
        out.push(ConfigKeySource {
            key: "policy.tools.run_shell.mode".to_string(),
            value_preview: cfg.run_shell_mode.value.clone(),
            source: cfg.run_shell_mode.source.clone(),
        });
        out.push(ConfigKeySource {
            key: "policy.tools.run_shell.allowlist".to_string(),
            value_preview: cfg.run_shell_allowlist.value.join(","),
            source: cfg.run_shell_allowlist.source.clone(),
        });
        out.push(ConfigKeySource {
            key: "policy.tool_default_mode".to_string(),
            value_preview: cfg.tool_default_mode.value.clone(),
            source: cfg.tool_default_mode.source.clone(),
        });
        if !cfg.tool_mode_by_capability.value.is_empty() {
            let preview: Vec<String> = cfg
                .tool_mode_by_capability
                .value
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            out.push(ConfigKeySource {
                key: "policy.tool_mode_by_capability".to_string(),
                value_preview: preview.join(", "),
                source: cfg.tool_mode_by_capability.source.clone(),
            });
        }
        if !cfg.tool_modes.value.is_empty() {
            let preview: Vec<String> = cfg
                .tool_modes
                .value
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            out.push(ConfigKeySource {
                key: "policy.tools".to_string(),
                value_preview: format!("tool_modes: {}", preview.join(", ")),
                source: cfg.tool_modes.source.clone(),
            });
        }

        out
    }
}

impl ConfigExplainProvider for StdConfigExplainProvider {
    fn explain(&self) -> Result<ConfigExplainInfo, Error> {
        let cfg = self.provider.policy_config()?;
        let resolved = serde_json::to_value(&cfg).map_err(|e| Error::Json(e.to_string()))?;
        let sources = self.build_sources(&cfg);
        let notes = vec![
            "Precedence: CLI flags > env > project (.aish/config.toml) > user (config.toml) > defaults"
                .to_string(),
            format!("schema_version: {}", cfg.schema_version.value),
        ];

        Ok(ConfigExplainInfo {
            v: 1,
            resolved,
            sources,
            notes,
        })
    }
}
