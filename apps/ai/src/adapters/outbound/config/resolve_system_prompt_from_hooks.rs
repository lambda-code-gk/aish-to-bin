//! フックを実行してシステムプロンプトを解決する標準実装
//!
//! システム・ユーザー・プロジェクトの 3 種類の hooks/system_prompt を順に実行し、
//! 各 stdout を `\n\n` で結合する。

use crate::ports::outbound::ResolveSystemPromptFromHooks;
use common::domain::CatalogKind;
use common::error::Error;
use common::ports::outbound::{FileSystem, RuntimeCatalog};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

pub struct StdResolveSystemPromptFromHooks {
    fs: Arc<dyn FileSystem>,
    catalog: Arc<dyn RuntimeCatalog>,
}

impl StdResolveSystemPromptFromHooks {
    pub fn new(fs: Arc<dyn FileSystem>, catalog: Arc<dyn RuntimeCatalog>) -> Self {
        Self { fs, catalog }
    }
}

impl ResolveSystemPromptFromHooks for StdResolveSystemPromptFromHooks {
    fn resolve_system_prompt_from_hooks(&self) -> Result<Option<String>, Error> {
        let mut parts = Vec::new();

        // CatalogKind::SystemPromptHooks の候補を優先順位順に実行する。
        for loc in self.catalog.locations(CatalogKind::SystemPromptHooks)? {
            if let Some(s) = run_hook_dir(self.fs.as_ref(), &loc.path)? {
                parts.push(s);
            }
        }

        if parts.is_empty() {
            return Ok(None);
        }
        let combined = parts.join("\n\n");
        let trimmed = combined.trim();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed.to_string()))
        }
    }
}

/// 指定ディレクトリ内の実行可能ファイルを名前昇順で実行し、stdout を結合して返す。
/// ディレクトリが無い・空・全スクリプトが非ゼロ終了の場合は None。
/// 非ゼロ終了したスクリプトの出力は無視する（捨てて続行）。
fn run_hook_dir(fs: &dyn FileSystem, dir: &Path) -> Result<Option<String>, Error> {
    if !fs.exists(dir) {
        return Ok(None);
    }
    let entries = match fs.read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    let mut executables: Vec<PathBuf> = entries
        .into_iter()
        .filter(|p| is_executable_file(fs, p))
        .collect();
    executables.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let mut outputs = Vec::new();
    for path in executables {
        if let Ok(output) = Command::new(&path).output() {
            if output.status.success() {
                if let Ok(s) = String::from_utf8(output.stdout) {
                    let t = s.trim();
                    if !t.is_empty() {
                        outputs.push(t.to_string());
                    }
                }
            }
        }
    }
    if outputs.is_empty() {
        Ok(None)
    } else {
        Ok(Some(outputs.join("\n\n")))
    }
}

fn is_executable_file(fs: &dyn FileSystem, path: &Path) -> bool {
    let meta = match fs.metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(std_meta) = std::fs::metadata(path) {
            return std_meta.permissions().mode() & 0o111 != 0;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::StdFileSystem;
    use common::domain::{CatalogLocation, CatalogScope};
    use std::fs;

    struct StubCatalog {
        locations: Vec<CatalogLocation>,
    }

    impl RuntimeCatalog for StubCatalog {
        fn locations(&self, kind: CatalogKind) -> Result<Vec<CatalogLocation>, Error> {
            if kind != CatalogKind::SystemPromptHooks {
                return Ok(Vec::new());
            }
            Ok(self.locations.clone())
        }

        fn project_root(&self) -> Result<Option<PathBuf>, Error> {
            Ok(None)
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_resolve_system_prompt_from_hooks_project_hook() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let project_root = tmp.path();
        let hook_dir = project_root
            .join(".aish")
            .join("hooks")
            .join("system_prompt");
        fs::create_dir_all(&hook_dir).expect("create hook dir");
        let script = hook_dir.join("01_echo.sh");
        fs::write(&script, "#!/bin/sh\necho 'You are a helpful assistant.'\n")
            .expect("write script");
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("chmod");

        // モック用の一時ディレクトリを作成
        let mock_home = tmp.path().join("mock_home");
        let mock_config = tmp.path().join("mock_config");
        fs::create_dir_all(&mock_home).expect("create mock home");
        fs::create_dir_all(&mock_config).expect("create mock config");

        // 環境変数をモック
        let original_home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let original_xdg_config =
            std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| "".to_string());
        let original_aish_home = std::env::var("AISH_HOME").unwrap_or_else(|_| "".to_string());

        std::env::set_var("HOME", &mock_home);
        std::env::set_var("XDG_CONFIG_HOME", &mock_config);
        std::env::remove_var("AISH_HOME"); // AISH_HOMEが未設定の場合、XDG_CONFIG_HOMEが使われる

        let fs_adapter: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let catalog = StubCatalog {
            locations: vec![CatalogLocation {
                kind: CatalogKind::SystemPromptHooks,
                scope: CatalogScope::Project,
                path: hook_dir.clone(),
            }],
        };
        let resolver = StdResolveSystemPromptFromHooks::new(fs_adapter, Arc::new(catalog));

        let cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(project_root).expect("set_current_dir");

        let out = resolver
            .resolve_system_prompt_from_hooks()
            .expect("resolve");

        // 環境変数を元に戻す
        if original_aish_home.is_empty() {
            std::env::remove_var("AISH_HOME");
        } else {
            std::env::set_var("AISH_HOME", &original_aish_home);
        }
        if original_xdg_config.is_empty() {
            std::env::remove_var("XDG_CONFIG_HOME");
        } else {
            std::env::set_var("XDG_CONFIG_HOME", &original_xdg_config);
        }
        std::env::set_var("HOME", &original_home);

        let _ = std::env::set_current_dir(&cwd); // 元のディレクトリに戻す
        assert!(out.is_some());
        assert_eq!(out.unwrap().trim(), "You are a helpful assistant.");
    }
}

