use common::error::Error;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
}

#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub config: PluginToml,
    pub source: String,
}

fn find_project_root(mut current: &Path) -> Option<PathBuf> {
    loop {
        if current.join(".aish").is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn project_plugins_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let root = find_project_root(cwd.as_path())?;
    Some(root.join(".aish").join("plugins"))
}

fn xdg_config_plugins_dir() -> Option<PathBuf> {
    // XDG_CONFIG_HOME/aish/plugins or $HOME/.config/aish/plugins
    if let Ok(v) = std::env::var("XDG_CONFIG_HOME") {
        if !v.trim().is_empty() {
            return Some(PathBuf::from(v).join("aish").join("plugins"));
        }
    }
    let home = std::env::var("HOME").ok()?;
    if home.trim().is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(".config").join("aish").join("plugins"))
}

fn read_toml_file(path: &Path) -> Result<PluginToml, Error> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| Error::io_msg(format!("read {}: {}", path.display(), e)))?;
    toml::from_str::<PluginToml>(&s)
        .map_err(|e| Error::invalid_argument(format!("invalid plugin.toml {}: {}", path.display(), e)))
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
            } else if matches!(p.extension().and_then(|s| s.to_str()), Some("yaml") | Some("yml")) {
                // legacy YAML (plugins.d 相当も同ディレクトリに置かれ得るため)
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

pub fn discover_plugins() -> Result<Vec<DiscoveredPlugin>, Error> {
    let mut files = Vec::new();
    if let Some(p) = project_plugins_dir() {
        files.extend(collect_from_dir(&p));
    }
    if let Some(p) = xdg_config_plugins_dir() {
        files.extend(collect_from_dir(&p));
    }
    // legacy trusted dirs: ~/.config/aish/plugins.d, ~/.aish/plugins.d
    if let Some(p) = xdg_config_plugins_dir().map(|d| d.parent().unwrap().to_path_buf().join("plugins.d")) {
        files.extend(collect_from_dir(&p));
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            files.extend(collect_from_dir(
                PathBuf::from(home).join(".aish").join("plugins.d").as_path(),
            ));
        }
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

