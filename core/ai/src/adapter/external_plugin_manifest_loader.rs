//! 信頼ディレクトリからプラグイン manifest を読み込む
//!
//! MVP: ~/.config/aish/plugins.d/*.yaml と ~/.aish/plugins.d/*.yaml のみ。project 配下は読まない。
//! 外部プラグイン対応用（将来有効化予定）。
#![allow(dead_code)]

use crate::domain::external_plugin::{
    PluginManifest, PluginManifestEntry, PluginTransport, StdioTransport,
};
use common::domain::event::{Event, RunId, SessionId};
use common::event_hub::EventHubHandle;
use common::ports::outbound::{EnvResolver, FileSystem};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 信頼ディレクトリを列挙する。
/// 1) config/plugins.d（XDG または AISH_HOME の config 配下）
/// 2) $HOME/.aish/plugins.d（ユーザーのホーム直下。resolve_home_dir は .config/aish を返すため、ここでは std::env::HOME を使う）
fn plugin_search_dirs(env: &Arc<dyn EnvResolver>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(dirs_resolved) = env.resolve_dirs() {
        dirs.push(dirs_resolved.config_dir.join("plugins.d"));
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            dirs.push(PathBuf::from(&home).join(".aish").join("plugins.d"));
        }
    }
    dirs
}

/// 1 ファイルを YAML としてパースし、PluginManifest に変換。不正なら Err で内容を返す。
fn parse_manifest_file(content: &str, _path: &Path) -> Result<PluginManifest, String> {
    let raw: serde_yaml::Value = serde_yaml::from_str(content).map_err(|e| e.to_string())?;
    let map = raw
        .as_mapping()
        .ok_or_else(|| "manifest must be a YAML object".to_string())?;
    let id = map
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing id".to_string())?
        .to_string();
    let version = map
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing version".to_string())?
        .to_string();
    let transport = map
        .get("transport")
        .ok_or_else(|| "missing transport".to_string())?;
    let transport_type = transport
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("stdio");
    let transport_parsed = if transport_type == "stdio" {
        let command = transport
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "transport.command required for stdio".to_string())?
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
        PluginTransport::Stdio(StdioTransport { command, args })
    } else {
        return Err(format!("unsupported transport type: {}", transport_type));
    };
    let env = map
        .get("env")
        .and_then(|v| v.as_mapping())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| {
                    let k = k.as_str()?;
                    let v = v.as_str()?;
                    Some((k.to_string(), v.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    let timeouts = map
        .get("timeouts")
        .and_then(|t| {
            let startup_ms = t.get("startup_ms").and_then(|v| v.as_u64());
            let call_ms = t.get("call_ms").and_then(|v| v.as_u64());
            Some(crate::domain::external_plugin::PluginTimeouts {
                startup_ms,
                call_ms,
            })
        })
        .unwrap_or_else(|| crate::domain::external_plugin::PluginTimeouts::default());
    let enabled = map.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    Ok(PluginManifest {
        id: id.clone(),
        version,
        transport: transport_parsed,
        env,
        timeouts,
        enabled,
    })
}

/// 1 ディレクトリから *.yaml をファイル名昇順で読み、有効な manifest のみ返す。
fn load_from_dir(
    fs: &Arc<dyn FileSystem>,
    dir: &Path,
) -> Result<Vec<PluginManifestEntry>, common::error::Error> {
    if !fs.exists(dir) {
        return Ok(Vec::new());
    }
    let mut entries = fs.read_dir(dir)?;
    entries.sort_by(|a, b| {
        let an = a.file_name().unwrap_or_default();
        let bn = b.file_name().unwrap_or_default();
        an.cmp(bn)
    });
    let mut out = Vec::new();
    for path in entries {
        if path.extension().map(|e| e == "yaml").unwrap_or(false)
            || path.extension().map(|e| e == "yml").unwrap_or(false)
        {
            let content = match fs.read_to_string(&path) {
                Ok(c) => c,
                Err(_) => {
                    // 読めないファイルはスキップ（イベントは呼び元で出す）
                    continue;
                }
            };
            match parse_manifest_file(&content, &path) {
                Ok(manifest) => {
                    if !manifest.enabled {
                        continue;
                    }
                    out.push(PluginManifestEntry {
                        manifest,
                        manifest_path: path,
                    });
                }
                Err(_) => {
                    // 不正 manifest はスキップ（イベントは呼び元で出す）
                }
            }
        }
    }
    Ok(out)
}

/// 全信頼ディレクトリから manifest を収集。ファイル名昇順で安定。
pub fn discover_manifests(
    fs: Arc<dyn FileSystem>,
    env: Arc<dyn EnvResolver>,
    event_hub: Option<&EventHubHandle>,
) -> Result<Vec<PluginManifestEntry>, common::error::Error> {
    let dirs = plugin_search_dirs(&env);
    let mut all = Vec::new();
    let session_id = SessionId::new("bootstrap");
    let run_id = RunId::new("plugins");
    for dir in dirs {
        let entries = match load_from_dir(&fs, &dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in &entries {
            if let Some(hub) = event_hub {
                hub.emit(Event {
                    v: 1,
                    session_id: session_id.clone(),
                    run_id: run_id.clone(),
                    kind: "external_plugin.discovered".to_string(),
                    payload: serde_json::json!({
                        "plugin_id": entry.manifest.id,
                        "manifest_path": entry.manifest_path.to_string_lossy(),
                    }),
                });
            }
            all.push(entry.clone());
        }
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_manifest_minimal() {
        let yaml = r#"
id: my-plugin
version: "0.1.0"
transport:
  type: stdio
  command: node
  args: ["plugin.js"]
"#;
        let m = parse_manifest_file(yaml, Path::new("test.yaml")).unwrap();
        assert_eq!(m.id, "my-plugin");
        assert!(m.enabled);
        match &m.transport {
            PluginTransport::Stdio(s) => {
                assert_eq!(s.command, "node");
                assert_eq!(s.args, &["plugin.js"]);
            }
        }
    }

    #[test]
    fn parse_manifest_with_enabled_false() {
        let yaml = r#"
id: disabled
version: "0.1.0"
transport:
  type: stdio
  command: echo
enabled: false
"#;
        let m = parse_manifest_file(yaml, Path::new("test.yaml")).unwrap();
        assert!(!m.enabled);
    }

    #[test]
    fn parse_manifest_invalid_missing_id() {
        let yaml = r#"
version: "0.1.0"
transport:
  type: stdio
  command: echo
"#;
        let r = parse_manifest_file(yaml, Path::new("test.yaml"));
        assert!(r.is_err());
    }

    #[test]
    fn parse_manifest_invalid_unsupported_transport() {
        let yaml = r#"
id: x
version: "0.1.0"
transport:
  type: http
  url: http://localhost:9999
"#;
        let r = parse_manifest_file(yaml, Path::new("test.yaml"));
        assert!(r.is_err());
    }
}
