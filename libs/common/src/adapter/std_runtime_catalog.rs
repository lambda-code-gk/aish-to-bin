use crate::domain::{CatalogKind, CatalogLocation, CatalogScope, Dirs};
use crate::error::Error;
use crate::ports::outbound::{EnvResolver, FileSystem, RuntimeCatalog};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 標準 RuntimeCatalog 実装
///
/// - EnvResolver に依存して config/data 等の基点を解決
/// - FileSystem に依存してパスの存在確認・種別判定を行う
pub struct StdRuntimeCatalog {
    env: Arc<dyn EnvResolver>,
    fs: Arc<dyn FileSystem>,
}

impl StdRuntimeCatalog {
    pub fn new(env: Arc<dyn EnvResolver>, fs: Arc<dyn FileSystem>) -> Self {
        Self { env, fs }
    }

    fn push_if_dir(
        &self,
        out: &mut Vec<CatalogLocation>,
        kind: CatalogKind,
        scope: CatalogScope,
        path: PathBuf,
    ) {
        if !self.fs.exists(&path) {
            return;
        }
        if let Ok(meta) = self.fs.metadata(&path) {
            if meta.is_dir() {
                out.push(CatalogLocation { kind, scope, path });
            }
        }
    }

    fn dirs(&self) -> Result<Dirs, Error> {
        self.env.resolve_dirs()
    }

    /// AISH_HOME が設定されているかどうか
    fn has_aish_home() -> bool {
        matches!(env::var("AISH_HOME"), Ok(v) if !v.is_empty())
    }

    fn project_root_impl(&self) -> Result<Option<PathBuf>, Error> {
        let mut current = match self.env.current_dir() {
            Ok(cwd) => cwd,
            Err(_) => return Ok(None),
        };
        loop {
            let aish_dir = current.join(".aish");
            if self.fs.exists(&aish_dir) {
                if let Ok(meta) = self.fs.metadata(&aish_dir) {
                    if meta.is_dir() {
                        return Ok(Some(current.clone()));
                    }
                }
            }
            if !current.pop() {
                return Ok(None);
            }
        }
    }
}

impl RuntimeCatalog for StdRuntimeCatalog {
    fn locations(&self, kind: CatalogKind) -> Result<Vec<CatalogLocation>, Error> {
        let mut out = Vec::new();
        match kind {
            CatalogKind::Tasks => {
                let dirs = self.dirs()?;
                // 1. config/task.d （UserConfig）
                let config_task = dirs.config_dir.join("task.d");
                self.push_if_dir(
                    &mut out,
                    CatalogKind::Tasks,
                    CatalogScope::UserConfig,
                    config_task,
                );

                // 2. 互換: AISH_HOME/task.d （LegacyUser）
                if Self::has_aish_home() {
                    if let Ok(aish_home) = env::var("AISH_HOME") {
                        if !aish_home.is_empty() {
                            let legacy = Path::new(&aish_home).join("task.d");
                            self.push_if_dir(
                                &mut out,
                                CatalogKind::Tasks,
                                CatalogScope::LegacyUser,
                                legacy,
                            );
                        }
                    }
                }
            }
            CatalogKind::SystemPromptHooks => {
                let dirs = self.dirs()?;
                // 1. config/hooks/system_prompt （UserConfig）
                let config_dir = dirs.config_dir.join("hooks").join("system_prompt");
                self.push_if_dir(
                    &mut out,
                    CatalogKind::SystemPromptHooks,
                    CatalogScope::UserConfig,
                    config_dir,
                );

                // 2. ~/.aish/hooks/system_prompt （LegacyUser）
                if let Ok(home) = env::var("HOME") {
                    if !home.is_empty() {
                        let user_dir = Path::new(&home)
                            .join(".aish")
                            .join("hooks")
                            .join("system_prompt");
                        self.push_if_dir(
                            &mut out,
                            CatalogKind::SystemPromptHooks,
                            CatalogScope::LegacyUser,
                            user_dir,
                        );
                    }
                }

                // 3. {project_root}/.aish/hooks/system_prompt （Project）
                if let Some(root) = self.project_root()? {
                    let proj_dir = root.join(".aish").join("hooks").join("system_prompt");
                    self.push_if_dir(
                        &mut out,
                        CatalogKind::SystemPromptHooks,
                        CatalogScope::Project,
                        proj_dir,
                    );
                }
            }
            CatalogKind::Plugins => {
                let dirs = self.dirs()?;

                // 1. {project_root}/.aish/plugins （Project）
                if let Some(root) = self.project_root()? {
                    let proj_plugins = root.join(".aish").join("plugins");
                    self.push_if_dir(
                        &mut out,
                        CatalogKind::Plugins,
                        CatalogScope::Project,
                        proj_plugins,
                    );
                }

                // 2. config/plugins （UserConfig）
                let config_plugins = dirs.config_dir.join("plugins");
                self.push_if_dir(
                    &mut out,
                    CatalogKind::Plugins,
                    CatalogScope::UserConfig,
                    config_plugins,
                );

                // 3. config/plugins.d （LegacyUser）
                let config_plugins_d = dirs.config_dir.join("plugins.d");
                self.push_if_dir(
                    &mut out,
                    CatalogKind::Plugins,
                    CatalogScope::LegacyUser,
                    config_plugins_d,
                );

                // 4. ~/.aish/plugins.d （LegacyUser）
                if let Ok(home) = env::var("HOME") {
                    if !home.is_empty() {
                        let legacy = Path::new(&home).join(".aish").join("plugins.d");
                        self.push_if_dir(
                            &mut out,
                            CatalogKind::Plugins,
                            CatalogScope::LegacyUser,
                            legacy,
                        );
                    }
                }
            }
            CatalogKind::Skills => {
                let dirs = self.dirs()?;

                // 1. {project_root}/.aish/skills （Project）
                if let Some(root) = self.project_root()? {
                    let proj_skills = root.join(".aish").join("skills");
                    self.push_if_dir(
                        &mut out,
                        CatalogKind::Skills,
                        CatalogScope::Project,
                        proj_skills,
                    );
                }

                // 2. config/skills （UserConfig）
                let config_skills = dirs.config_dir.join("skills");
                self.push_if_dir(
                    &mut out,
                    CatalogKind::Skills,
                    CatalogScope::UserConfig,
                    config_skills,
                );
            }
            CatalogKind::Packages => {
                let dirs = self.dirs()?;

                // 1. {project_root}/.aish/packages （Project）
                if let Some(root) = self.project_root()? {
                    let proj_packages = root.join(".aish").join("packages");
                    self.push_if_dir(
                        &mut out,
                        CatalogKind::Packages,
                        CatalogScope::Project,
                        proj_packages,
                    );
                }

                // 2. config/packages （UserConfig）
                let config_packages = dirs.config_dir.join("packages");
                self.push_if_dir(
                    &mut out,
                    CatalogKind::Packages,
                    CatalogScope::UserConfig,
                    config_packages,
                );
            }
        }
        Ok(out)
    }

    fn project_root(&self) -> Result<Option<PathBuf>, Error> {
        self.project_root_impl()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::StdEnvResolver;
    use crate::adapter::StdFileSystem;
    use crate::ports::outbound::FileSystem;

    use std::fs;

    fn with_env_var<F: FnOnce()>(key: &str, value: Option<&str>, f: F) {
        let prev = env::var(key).ok();
        match value {
            Some(v) => env::set_var(key, v),
            None => env::remove_var(key),
        }
        f();
        match prev {
            Some(v) => env::set_var(key, v),
            None => env::remove_var(key),
        }
    }

    fn tempdir(prefix: &str) -> PathBuf {
        let base = env::temp_dir();
        let dir = base.join(format!("{prefix}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn tasks_locations_with_aish_home() {
        let tmp = tempdir("runtime_catalog_tasks_aish_home");
        let aish_home = tmp.join("aish_home");
        let config_task = aish_home.join("config").join("task.d");
        let legacy_task = aish_home.join("task.d");
        fs::create_dir_all(&config_task).unwrap();
        fs::create_dir_all(&legacy_task).unwrap();

        with_env_var("HOME", Some(tmp.to_str().unwrap()), || {
            with_env_var("AISH_HOME", Some(aish_home.to_str().unwrap()), || {
                with_env_var("XDG_CONFIG_HOME", None, || {
                    let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
                    let fs_adapter: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
                    let catalog = StdRuntimeCatalog::new(Arc::clone(&env), Arc::clone(&fs_adapter));

                    let locs = catalog.locations(CatalogKind::Tasks).unwrap();
                    let paths: Vec<PathBuf> = locs.into_iter().map(|l| l.path).collect();
                    assert_eq!(paths, vec![config_task, legacy_task]);
                });
            });
        });
    }

    #[test]
    fn tasks_locations_without_aish_home_prefers_xdg_config_home() {
        let tmp = tempdir("runtime_catalog_tasks_xdg");
        let xdg = tmp.join("xdg_config");
        let task_dir = xdg.join("aish").join("task.d");
        fs::create_dir_all(&task_dir).unwrap();

        with_env_var("HOME", Some(tmp.to_str().unwrap()), || {
            with_env_var("AISH_HOME", None, || {
                with_env_var("XDG_CONFIG_HOME", Some(xdg.to_str().unwrap()), || {
                    let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
                    let fs_adapter: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
                    let catalog = StdRuntimeCatalog::new(Arc::clone(&env), Arc::clone(&fs_adapter));

                    let locs = catalog.locations(CatalogKind::Tasks).unwrap();
                    let paths: Vec<PathBuf> = locs.into_iter().map(|l| l.path).collect();
                    assert_eq!(paths, vec![task_dir]);
                });
            });
        });
    }

    #[test]
    fn tasks_locations_fall_back_to_home_config() {
        let tmp = tempdir("runtime_catalog_tasks_home");
        let home = tmp.join("home");
        let task_dir = home.join(".config").join("aish").join("task.d");
        fs::create_dir_all(&task_dir).unwrap();

        with_env_var("HOME", Some(home.to_str().unwrap()), || {
            with_env_var("AISH_HOME", None, || {
                with_env_var("XDG_CONFIG_HOME", None, || {
                    let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
                    let fs_adapter: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
                    let catalog = StdRuntimeCatalog::new(Arc::clone(&env), Arc::clone(&fs_adapter));

                    let locs = catalog.locations(CatalogKind::Tasks).unwrap();
                    let paths: Vec<PathBuf> = locs.into_iter().map(|l| l.path).collect();
                    assert_eq!(paths, vec![task_dir]);
                });
            });
        });
    }

    #[test]
    fn system_prompt_hooks_locations_ordered_config_legacy_project() {
        let tmp = tempdir("runtime_catalog_hooks");
        let home = tmp.join("home");
        let xdg = tmp.join("xdg_config");
        let project = tmp.join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&xdg).unwrap();
        fs::create_dir_all(&project).unwrap();

        let config_hooks = xdg.join("aish").join("hooks").join("system_prompt");
        let legacy_hooks = home.join(".aish").join("hooks").join("system_prompt");
        let project_hooks = project.join(".aish").join("hooks").join("system_prompt");
        fs::create_dir_all(&config_hooks).unwrap();
        fs::create_dir_all(&legacy_hooks).unwrap();
        fs::create_dir_all(&project_hooks).unwrap();

        with_env_var("HOME", Some(home.to_str().unwrap()), || {
            with_env_var("AISH_HOME", None, || {
                with_env_var("XDG_CONFIG_HOME", Some(xdg.to_str().unwrap()), || {
                    let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
                    let fs_adapter: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
                    let catalog = StdRuntimeCatalog::new(Arc::clone(&env), Arc::clone(&fs_adapter));

                    let cwd = env::current_dir().unwrap();
                    env::set_current_dir(&project).unwrap();

                    let locs = catalog.locations(CatalogKind::SystemPromptHooks).unwrap();
                    let paths: Vec<PathBuf> = locs.into_iter().map(|l| l.path).collect();
                    assert_eq!(paths, vec![config_hooks, legacy_hooks, project_hooks]);

                    env::set_current_dir(cwd).unwrap();
                });
            });
        });
    }

    #[test]
    fn plugins_locations_ordered_project_then_config_then_legacy() {
        let tmp = tempdir("runtime_catalog_plugins");
        let home = tmp.join("home");
        let xdg = tmp.join("xdg_config");
        let project = tmp.join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&xdg).unwrap();
        fs::create_dir_all(&project).unwrap();

        let proj_plugins = project.join(".aish").join("plugins");
        let config_plugins = xdg.join("aish").join("plugins");
        let config_plugins_d = xdg.join("aish").join("plugins.d");
        let legacy_plugins_d = home.join(".aish").join("plugins.d");
        fs::create_dir_all(&proj_plugins).unwrap();
        fs::create_dir_all(&config_plugins).unwrap();
        fs::create_dir_all(&config_plugins_d).unwrap();
        fs::create_dir_all(&legacy_plugins_d).unwrap();

        with_env_var("HOME", Some(home.to_str().unwrap()), || {
            with_env_var("AISH_HOME", None, || {
                with_env_var("XDG_CONFIG_HOME", Some(xdg.to_str().unwrap()), || {
                    let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
                    let fs_adapter: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
                    let catalog = StdRuntimeCatalog::new(Arc::clone(&env), Arc::clone(&fs_adapter));

                    let cwd = env::current_dir().unwrap();
                    env::set_current_dir(&project).unwrap();

                    let locs = catalog.locations(CatalogKind::Plugins).unwrap();
                    let paths: Vec<PathBuf> = locs.into_iter().map(|l| l.path).collect();
                    assert_eq!(
                        paths,
                        vec![
                            proj_plugins,
                            config_plugins,
                            config_plugins_d,
                            legacy_plugins_d
                        ]
                    );

                    env::set_current_dir(cwd).unwrap();
                });
            });
        });
    }

    #[test]
    fn skills_locations_ordered_project_then_config() {
        let tmp = tempdir("runtime_catalog_skills");
        let home = tmp.join("home");
        let xdg = tmp.join("xdg_config");
        let project = tmp.join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&xdg).unwrap();
        fs::create_dir_all(&project).unwrap();

        let proj_skills = project.join(".aish").join("skills");
        let config_skills = xdg.join("aish").join("skills");
        fs::create_dir_all(&proj_skills).unwrap();
        fs::create_dir_all(&config_skills).unwrap();

        with_env_var("HOME", Some(home.to_str().unwrap()), || {
            with_env_var("AISH_HOME", None, || {
                with_env_var("XDG_CONFIG_HOME", Some(xdg.to_str().unwrap()), || {
                    let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
                    let fs_adapter: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
                    let catalog = StdRuntimeCatalog::new(Arc::clone(&env), Arc::clone(&fs_adapter));

                    let cwd = env::current_dir().unwrap();
                    env::set_current_dir(&project).unwrap();

                    let locs = catalog.locations(CatalogKind::Skills).unwrap();
                    let paths: Vec<PathBuf> = locs.into_iter().map(|l| l.path).collect();
                    assert_eq!(paths, vec![proj_skills, config_skills]);

                    env::set_current_dir(cwd).unwrap();
                });
            });
        });
    }

    #[test]
    fn project_root_detects_parent_with_aish_dir() {
        let tmp = tempdir("runtime_catalog_project_root");
        let project = tmp.join("my_project");
        let nested = project.join("src");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(project.join(".aish")).unwrap();

        with_env_var("HOME", Some(tmp.to_str().unwrap()), || {
            with_env_var("AISH_HOME", None, || {
                with_env_var("XDG_CONFIG_HOME", None, || {
                    let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
                    let fs_adapter: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
                    let catalog = StdRuntimeCatalog::new(Arc::clone(&env), Arc::clone(&fs_adapter));

                    let cwd = env::current_dir().unwrap();
                    env::set_current_dir(&nested).unwrap();

                    let root = catalog.project_root().unwrap();
                    assert_eq!(root, Some(project.clone()));

                    env::set_current_dir(cwd).unwrap();
                });
            });
        });
    }
}
