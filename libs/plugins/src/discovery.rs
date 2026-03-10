//! Plugin discovery (canonical source of truth for plugins).
//!
//! Responsibilities:
//! - Define how `RuntimeCatalog` locations for `CatalogKind::Plugins` are
//!   interpreted.
//! - Load `plugin.toml` manifests (canonical format).
//! - Maintain minimal compatibility with legacy YAML manifests documented in
//!   `docs/external-tools.md`.
//! - Apply `enabled` flags (deny-by-default for legacy YAML).
//! - Apply duplicate id handling (first match wins, later entries skipped).

use common::adapter::{StdEnvResolver, StdFileSystem, StdRuntimeCatalog};
use common::domain::{CatalogKind, CatalogLocation};
use common::error::Error;
use common::ports::outbound::{EnvResolver, RuntimeCatalog};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PluginPolicyToml {
    #[serde(default)]
    pub default_tool_mode: Option<String>,
    #[serde(default)]
    pub capabilities_hint: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginToml {
    pub id: String,
    pub namespace: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// allowlist した env var だけを子プロセスへ渡す（安全のため default empty）
    #[serde(default)]
    pub env_allowlist: Vec<String>,
    /// 明示 enable が必要（deny-by-default）
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// policy / capability hint（external tool policy 用の補助情報）
    #[serde(default)]
    pub policy: PluginPolicyToml,
}

#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub config: PluginToml,
    pub source: String,
}

fn locations_for_plugins(catalog: &dyn RuntimeCatalog) -> Result<Vec<CatalogLocation>, Error> {
    catalog.locations(CatalogKind::Plugins)
}

fn read_toml_file(path: &Path) -> Result<PluginToml, Error> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| Error::io_msg(format!("read {}: {}", path.display(), e)))?;
    toml::from_str::<PluginToml>(&s).map_err(|e| {
        Error::invalid_argument(format!("invalid plugin.toml {}: {}", path.display(), e))
    })
}

fn read_legacy_yaml_manifest(path: &Path) -> Result<PluginToml, Error> {
    // MVP legacy: docs/external-tools.md の YAML を最小互換で読む。
    // Phase9: deny-by-default のため enabled は欠けていれば false 扱いにする。
    let s = std::fs::read_to_string(path)
        .map_err(|e| Error::io_msg(format!("read {}: {}", path.display(), e)))?;
    let raw: serde_yaml::Value = serde_yaml::from_str(&s)
        .map_err(|e| Error::invalid_argument(format!("invalid yaml {}: {}", path.display(), e)))?;
    let map = raw
        .as_mapping()
        .ok_or_else(|| Error::invalid_argument("manifest must be a YAML object"))?;
    let id = map
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::invalid_argument("missing id"))?
        .to_string();
    let enabled = map
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let transport = map
        .get("transport")
        .and_then(|v| v.as_mapping())
        .ok_or_else(|| Error::invalid_argument("missing transport"))?;
    let ty = transport
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("stdio");
    if ty != "stdio" {
        return Err(Error::invalid_argument(format!(
            "unsupported transport type: {}",
            ty
        )));
    }
    let command = transport
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::invalid_argument("transport.command required"))?
        .to_string();
    let args = transport
        .get("args")
        .and_then(|v| v.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // legacy YAML は namespace を持たないので、id を namespace として扱う
    let namespace = id.clone();
    let timeout_ms = map
        .get("timeouts")
        .and_then(|t| t.get("call_ms"))
        .and_then(|v| v.as_u64());
    // legacy YAML では policy セクションはオプション扱いにする
    let policy = if let Some(p) = map.get("policy").and_then(|v| v.as_mapping()) {
        let default_tool_mode = p
            .get("default_tool_mode")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let capabilities_hint = p
            .get("capabilities_hint")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let notes = p
            .get("notes")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        PluginPolicyToml {
            default_tool_mode,
            capabilities_hint,
            notes,
        }
    } else {
        PluginPolicyToml::default()
    };

    Ok(PluginToml {
        id: id.clone(),
        namespace: namespace.clone(),
        display_name: Some(id),
        command,
        args,
        cwd: None,
        env_allowlist: Vec::new(),
        enabled,
        timeout_ms,
        policy,
    })
}

fn collect_from_dir(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for ent in entries.flatten() {
        let p = ent.path();
        if p.is_dir() {
            let candidate = p.join("plugin.toml");
            if candidate.is_file() {
                out.push(candidate);
            }
        } else if p.is_file() {
            if p.file_name().and_then(|s| s.to_str()) == Some("plugin.toml") {
                out.push(p);
            } else if p.extension().and_then(|s| s.to_str()) == Some("toml") {
                out.push(p);
            } else if matches!(
                p.extension().and_then(|s| s.to_str()),
                Some("yaml") | Some("yml")
            ) {
                // legacy YAML (plugins.d 相当も同ディレクトリに置かれ得るため)
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

pub fn discover_plugins_with_catalog(
    catalog: &dyn RuntimeCatalog,
) -> Result<Vec<DiscoveredPlugin>, Error> {
    let mut files = Vec::new();
    for loc in locations_for_plugins(catalog)? {
        files.extend(collect_from_dir(&loc.path));
    }

    let mut seen: HashMap<String, String> = HashMap::new();
    let mut out = Vec::new();
    for f in files {
        let cfg = if f.extension().and_then(|s| s.to_str()) == Some("toml")
            || f.file_name().and_then(|s| s.to_str()) == Some("plugin.toml")
        {
            read_toml_file(&f)?
        } else {
            read_legacy_yaml_manifest(&f)?
        };
        if let Some(prev) = seen.get(&cfg.id) {
            // 重複は先勝ち（誤配送防止）。衝突は discover 側でエラーにせず list に残す。
            let _ = prev;
            continue;
        }
        seen.insert(cfg.id.clone(), f.to_string_lossy().to_string());
        out.push(DiscoveredPlugin {
            config: cfg,
            source: f.to_string_lossy().to_string(),
        });
    }
    Ok(out)
}

pub fn discover_plugins() -> Result<Vec<DiscoveredPlugin>, Error> {
    let env: std::sync::Arc<dyn EnvResolver> = std::sync::Arc::new(StdEnvResolver);
    let fs = StdFileSystem;
    let fs_arc: std::sync::Arc<dyn common::ports::outbound::FileSystem> = std::sync::Arc::new(fs);
    let catalog =
        StdRuntimeCatalog::new(std::sync::Arc::clone(&env), std::sync::Arc::clone(&fs_arc));
    discover_plugins_with_catalog(&catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::domain::{CatalogKind, CatalogLocation, CatalogScope};
    use common::ports::outbound::RuntimeCatalog;
    use std::fs;
    use std::path::{Path, PathBuf};

    struct TestCatalog {
        dirs: Vec<PathBuf>,
    }

    impl RuntimeCatalog for TestCatalog {
        fn locations(&self, kind: CatalogKind) -> Result<Vec<CatalogLocation>, Error> {
            if kind != CatalogKind::Plugins {
                return Ok(Vec::new());
            }
            Ok(self
                .dirs
                .iter()
                .cloned()
                .map(|path| CatalogLocation {
                    kind: CatalogKind::Plugins,
                    scope: CatalogScope::UserConfig,
                    path,
                })
                .collect())
        }

        fn project_root(&self) -> Result<Option<PathBuf>, Error> {
            let _ = self;
            Ok(None)
        }
    }

    fn tempdir(name: &str) -> PathBuf {
        let base = std::env::temp_dir();
        let dir = base.join(format!("plugins_discovery_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn legacy_yaml_is_parsed_with_deny_by_default_enabled() {
        let tmp = tempdir("legacy_yaml_basic");
        let yaml = r#"
id: yaml-plugin
transport:
  type: stdio
  command: /bin/echo
  args: ["hello"]
timeouts:
  call_ms: 1234
"#;
        let path = tmp.join("legacy.yaml");
        fs::write(&path, yaml).unwrap();

        let catalog = TestCatalog { dirs: vec![tmp] };
        let discovered = discover_plugins_with_catalog(&catalog).unwrap();
        assert_eq!(discovered.len(), 1);
        let p = &discovered[0].config;
        assert_eq!(p.id, "yaml-plugin");
        assert_eq!(p.namespace, "yaml-plugin");
        assert_eq!(p.command, "/bin/echo");
        assert_eq!(p.args, vec!["hello"]);
        // legacy YAML は deny-by-default なので enabled が欠けていれば false
        assert_eq!(p.enabled, false);
        assert_eq!(p.timeout_ms, Some(1234));
        // legacy YAML では policy セクション未指定時はデフォルト値
        assert!(p.policy.default_tool_mode.is_none());
        assert!(p.policy.capabilities_hint.is_empty());
        assert!(p.policy.notes.is_none());
    }

    #[test]
    fn duplicate_ids_prefer_first_and_skip_later() {
        let tmp = tempdir("duplicate_ids");

        let first = r#"
id: same-id
transport:
  type: stdio
  command: cmd-first
"#;
        let second = r#"
id: same-id
transport:
  type: stdio
  command: cmd-second
"#;
        fs::write(tmp.join("01_first.yaml"), first).unwrap();
        fs::write(tmp.join("02_second.yaml"), second).unwrap();

        let catalog = TestCatalog { dirs: vec![tmp] };
        let discovered = discover_plugins_with_catalog(&catalog).unwrap();
        assert_eq!(discovered.len(), 1);
        let p = &discovered[0].config;
        assert_eq!(p.id, "same-id");
        assert_eq!(p.command, "cmd-first");
    }

    #[test]
    fn enabled_flag_is_parsed_from_manifests() {
        let tmp = tempdir("disabled");
        let enabled = r#"
id: enabled
enabled: true
transport:
  type: stdio
  command: cmd-enabled
"#;
        let disabled = r#"
id: disabled
enabled: false
transport:
  type: stdio
  command: cmd-disabled
"#;
        fs::write(tmp.join("a_enabled.yaml"), enabled).unwrap();
        fs::write(tmp.join("b_disabled.yaml"), disabled).unwrap();

        let catalog = TestCatalog { dirs: vec![tmp] };
        let discovered = discover_plugins_with_catalog(&catalog).unwrap();
        assert_eq!(discovered.len(), 2);
        let mut ids: Vec<(String, bool)> = discovered
            .into_iter()
            .map(|d| (d.config.id, d.config.enabled))
            .collect();
        ids.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            ids,
            vec![
                ("disabled".to_string(), false),
                ("enabled".to_string(), true)
            ]
        );
    }

    #[test]
    fn toml_policy_section_is_parsed() {
        let tmp = tempdir("toml_policy");
        let toml = r#"
id = "acme-docs"
namespace = "acme"
command = "python"
args = ["server.py"]
enabled = true

[policy]
default_tool_mode = "require_approval"
capabilities_hint = ["network"]
notes = "Calls internal docs/search API"
"#;
        let path = tmp.join("plugin.toml");
        fs::write(&path, toml).unwrap();

        let catalog = TestCatalog { dirs: vec![tmp] };
        let discovered = discover_plugins_with_catalog(&catalog).unwrap();
        assert_eq!(discovered.len(), 1);
        let p = &discovered[0].config;
        assert_eq!(
            p.policy.default_tool_mode.as_deref(),
            Some("require_approval")
        );
        assert_eq!(p.policy.capabilities_hint, vec!["network".to_string()]);
        assert_eq!(
            p.policy.notes.as_deref(),
            Some("Calls internal docs/search API")
        );
    }

    #[test]
    fn legacy_yaml_policy_section_is_parsed_if_present() {
        let tmp = tempdir("legacy_yaml_policy");
        let yaml = r#"
id: yaml-plugin-policy
enabled: true
transport:
  type: stdio
  command: /bin/echo
policy:
  default_tool_mode: deny
  capabilities_hint:
    - fs_read
    - network
  notes: Sensitive internal plugin
"#;
        let path = tmp.join("legacy_policy.yaml");
        fs::write(&path, yaml).unwrap();

        let catalog = TestCatalog { dirs: vec![tmp] };
        let discovered = discover_plugins_with_catalog(&catalog).unwrap();
        assert_eq!(discovered.len(), 1);
        let p = &discovered[0].config;
        assert_eq!(p.id, "yaml-plugin-policy");
        assert_eq!(p.policy.default_tool_mode.as_deref(), Some("deny"));
        assert_eq!(
            p.policy.capabilities_hint,
            vec!["fs_read".to_string(), "network".to_string()]
        );
        assert_eq!(p.policy.notes.as_deref(), Some("Sensitive internal plugin"));
    }
}
