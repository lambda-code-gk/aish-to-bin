use crate::domain::{
    PromptSourceKind, ResolvedPromptSource, SkillSpec, SourceProvenance, TaskName, TaskOriginInfo,
    TaskSpec,
};
use crate::ports::outbound::{
    PackageResolver, PromptSourceResolver, ResolveSystemPromptFromHooks, SkillSpecLoader,
    TaskSpecLoader,
};
use common::domain::{CatalogKind, CatalogScope};
use common::error::Error;
use common::ports::outbound::{FileSystem, RuntimeCatalog};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct StdPromptSourceResolver {
    fs: Arc<dyn FileSystem>,
    catalog: Arc<dyn RuntimeCatalog>,
    hooks: Arc<dyn ResolveSystemPromptFromHooks>,
    task_specs: Arc<dyn TaskSpecLoader>,
    skill_specs: Arc<dyn SkillSpecLoader>,
    packages: Arc<dyn PackageResolver>,
}

impl StdPromptSourceResolver {
    pub fn new(
        fs: Arc<dyn FileSystem>,
        catalog: Arc<dyn RuntimeCatalog>,
        hooks: Arc<dyn ResolveSystemPromptFromHooks>,
        task_specs: Arc<dyn TaskSpecLoader>,
        skill_specs: Arc<dyn SkillSpecLoader>,
        packages: Arc<dyn PackageResolver>,
    ) -> Self {
        Self {
            fs,
            catalog,
            hooks,
            task_specs,
            skill_specs,
            packages,
        }
    }

    fn find_task_root(
        &self,
        task_name: &TaskName,
    ) -> Result<Option<(PathBuf, TaskKind, CatalogScope)>, Error> {
        let locations = self.catalog.locations(CatalogKind::Tasks)?;
        for loc in locations {
            if let Some(kind) = Self::task_kind_in_dir(self.fs.as_ref(), &loc.path, task_name) {
                return Ok(Some((loc.path.clone(), kind, loc.scope)));
            }
        }
        Ok(None)
    }

    /// package 配下の tasks/ も含めてタスクの所在を解決する。
    ///
    /// - まず CatalogKind::Tasks から task.d 由来タスクを探す
    /// - 見つからなければ PackageResolver から得た package 配下の tasks/ を探す
    /// 戻り値: (task_root, kind, package_if_from_package, scope)
    fn find_task_origin(
        &self,
        task_name: &TaskName,
    ) -> Result<
        Option<(
            PathBuf,
            TaskKind,
            Option<crate::domain::ResolvedPackage>,
            CatalogScope,
        )>,
        Error,
    > {
        // 1. 既存の task.d 由来
        if let Some((root, kind, scope)) = self.find_task_root(task_name)? {
            return Ok(Some((root, kind, None, scope)));
        }

        // 2. package 由来
        let pkgs = self.packages.list_packages()?;
        for pkg in pkgs {
            let task_root = pkg.spec.root_dir.join("tasks");
            if let Some(kind) = Self::task_kind_in_dir(self.fs.as_ref(), &task_root, task_name) {
                let scope = pkg.scope;
                return Ok(Some((task_root, kind, Some(pkg), scope)));
            }
        }
        Ok(None)
    }

    fn task_kind_in_dir(
        fs: &dyn FileSystem,
        task_root: &Path,
        task_name: &TaskName,
    ) -> Option<TaskKind> {
        let dir_execute = task_root.join(task_name.as_ref()).join("execute");
        if fs.exists(&dir_execute) {
            if let Ok(m) = fs.metadata(&dir_execute) {
                if m.is_file() {
                    return Some(TaskKind::Directory);
                }
            }
        }
        let script = task_root.join(format!("{}.sh", task_name.as_ref()));
        if fs.exists(&script) {
            if let Ok(m) = fs.metadata(&script) {
                if m.is_file() {
                    return Some(TaskKind::File);
                }
            }
        }
        None
    }

    fn task_prompt_path(task_root: &Path, task_name: &TaskName, kind: TaskKind) -> PathBuf {
        match kind {
            TaskKind::Directory => task_root.join(task_name.as_ref()).join("prompt.md"),
            TaskKind::File => task_root.join(format!("{}.prompt.md", task_name.as_ref())),
        }
    }

    fn load_or_use_spec(
        &self,
        task_root: Option<&Path>,
        task_name: &TaskName,
        provided: Option<&TaskSpec>,
    ) -> Result<Option<TaskSpec>, Error> {
        if let Some(spec) = provided {
            return Ok(Some(spec.clone()));
        }
        let Some(task_root) = task_root else {
            return Ok(None);
        };
        self.task_specs.load_task_spec(task_root, task_name)
    }

    fn resolve_hooks(&self) -> Result<Vec<ResolvedPromptSource>, Error> {
        let mut out = Vec::new();
        if let Some(content) = self.hooks.resolve_system_prompt_from_hooks()? {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                out.push(ResolvedPromptSource {
                    kind: PromptSourceKind::Hook,
                    name: "system_prompt_hooks".to_string(),
                    content: trimmed.to_string(),
                    memory_topics: Vec::new(),
                    preferred_mode: None,
                    allowed_tools: Vec::new(),
                    denied_tools: Vec::new(),
                    provenance: Some(SourceProvenance::new(
                        CatalogScope::UserConfig,
                        None,
                        PathBuf::from("(system_prompt from hooks)"),
                        Some("synthesized".to_string()),
                    )),
                });
            }
        }
        Ok(out)
    }

    fn resolve_package_system_hook(
        &self,
        package: &crate::domain::ResolvedPackage,
    ) -> Option<ResolvedPromptSource> {
        let path = match &package.spec.system_hook {
            Some(p) => p,
            None => return None,
        };
        if !self.fs.exists(path) {
            return None;
        }
        let content = match self.fs.read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to read package system hook '{}': {}",
                    path.display(),
                    e
                );
                return None;
            }
        };
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(ResolvedPromptSource {
            kind: PromptSourceKind::Hook,
            name: format!("package:{}", package.spec.name),
            content: trimmed.to_string(),
            memory_topics: package.spec.memory_topics.clone(),
            preferred_mode: None,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            provenance: Some(SourceProvenance::new(
                package.scope,
                Some(package.spec.name.clone()),
                path.clone(),
                Some("package system hook".to_string()),
            )),
        })
    }

    fn resolve_task_prompt(
        &self,
        task_root: &Path,
        task_name: &TaskName,
        kind: TaskKind,
        spec: Option<&TaskSpec>,
        scope: CatalogScope,
        package_name: Option<String>,
    ) -> Option<ResolvedPromptSource> {
        let path = Self::task_prompt_path(task_root, task_name, kind);
        if !self.fs.exists(&path) {
            return None;
        }
        let content = match self.fs.read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to read task prompt '{}': {}",
                    path.display(),
                    e
                );
                return None;
            }
        };
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return None;
        }
        let (memory_topics, preferred_mode) = if let Some(s) = spec {
            (s.memory_topics.clone(), s.preferred_mode.clone())
        } else {
            (Vec::new(), None)
        };
        Some(ResolvedPromptSource {
            kind: PromptSourceKind::TaskPrompt,
            name: task_name.as_ref().to_string(),
            content: trimmed.to_string(),
            memory_topics,
            preferred_mode,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            provenance: Some(SourceProvenance::new(
                scope,
                package_name,
                path.clone(),
                None,
            )),
        })
    }

    fn resolve_skill_prompt_for(
        &self,
        skill_name: &str,
    ) -> Result<Option<(SkillSpec, String, SourceProvenance)>, Error> {
        // 1. 既存の skills ディレクトリから解決する
        let locations = self.catalog.locations(CatalogKind::Skills)?;
        for loc in locations {
            match self.skill_specs.load_skill_spec(&loc.path, skill_name)? {
                Some(spec) => {
                    let content = match self.fs.read_to_string(&spec.instructions_path) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!(
                                "Warning: Failed to read skill instructions '{}': {}",
                                spec.instructions_path.display(),
                                e
                            );
                            return Ok(None);
                        }
                    };
                    let trimmed = content.trim();
                    if trimmed.is_empty() {
                        return Ok(None);
                    }
                    let prov = SourceProvenance::new(
                        loc.scope,
                        None,
                        spec.instructions_path.clone(),
                        None,
                    );
                    return Ok(Some((spec, trimmed.to_string(), prov)));
                }
                None => {}
            }
        }

        // 2. 見つからない場合は package 配下の skills/ を探す
        let pkgs = self.packages.list_packages()?;
        for pkg in pkgs {
            let skills_root = pkg.spec.root_dir.join("skills");
            match self.skill_specs.load_skill_spec(&skills_root, skill_name)? {
                Some(spec) => {
                    let content = match self.fs.read_to_string(&spec.instructions_path) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!(
                                "Warning: Failed to read skill instructions '{}': {}",
                                spec.instructions_path.display(),
                                e
                            );
                            return Ok(None);
                        }
                    };
                    let trimmed = content.trim();
                    if trimmed.is_empty() {
                        return Ok(None);
                    }
                    let prov = SourceProvenance::new(
                        pkg.scope,
                        Some(pkg.spec.name.clone()),
                        spec.instructions_path.clone(),
                        Some("package skill".to_string()),
                    );
                    return Ok(Some((spec, trimmed.to_string(), prov)));
                }
                None => {}
            }
        }

        eprintln!(
            "Warning: Skill '{}' not found in skills directories or packages",
            skill_name
        );
        Ok(None)
    }

    fn resolve_skills(
        &self,
        task_name: &TaskName,
        spec: Option<&TaskSpec>,
    ) -> Result<Vec<ResolvedPromptSource>, Error> {
        let mut out = Vec::new();
        let Some(spec) = spec else {
            return Ok(out);
        };
        for skill_name in &spec.skills {
            match self.resolve_skill_prompt_for(skill_name)? {
                Some((skill_spec, content, provenance)) => {
                    out.push(ResolvedPromptSource {
                        kind: PromptSourceKind::Skill,
                        name: skill_spec.name.clone(),
                        content,
                        memory_topics: skill_spec.memory_topics.clone(),
                        preferred_mode: skill_spec.preferred_mode.clone(),
                        allowed_tools: skill_spec.allowed_tools.clone(),
                        denied_tools: skill_spec.denied_tools.clone(),
                        provenance: Some(provenance),
                    });
                }
                None => {
                    eprintln!(
                        "Warning: Skill '{}' for task '{}' is unavailable or empty",
                        skill_name,
                        task_name.as_ref()
                    );
                }
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy)]
enum TaskKind {
    Directory,
    File,
}

impl PromptSourceResolver for StdPromptSourceResolver {
    fn resolve_for_task(
        &self,
        task_name: &TaskName,
        task_spec: Option<&TaskSpec>,
    ) -> Result<(Vec<ResolvedPromptSource>, Option<TaskOriginInfo>), Error> {
        let hooks = self.resolve_hooks()?;

        let origin = self.find_task_origin(task_name)?;
        let (task_root, task_kind, package, scope) = match origin {
            Some(v) => v,
            None => {
                return Ok((hooks, None));
            }
        };

        let owned_spec = self.load_or_use_spec(Some(&task_root), task_name, task_spec)?;
        let spec_ref = owned_spec.as_ref().or(task_spec);

        let package_name = package.as_ref().map(|p| p.spec.name.clone());
        let task_prompt_path = Self::task_prompt_path(&task_root, task_name, task_kind);
        let task_origin_info = Some(TaskOriginInfo {
            task_name: task_name.as_ref().to_string(),
            provenance: SourceProvenance::new(
                scope,
                package_name.clone(),
                task_prompt_path.clone(),
                None,
            ),
        });

        let mut out = hooks;
        if let Some(pkg) = package.as_ref() {
            if let Some(pkg_hook) = self.resolve_package_system_hook(pkg) {
                out.push(pkg_hook);
            }
        }
        if let Some(task_prompt) = self.resolve_task_prompt(
            &task_root,
            task_name,
            task_kind,
            spec_ref,
            scope,
            package_name,
        ) {
            out.push(task_prompt);
        }
        let skills = self.resolve_skills(task_name, spec_ref)?;
        out.extend(skills);
        Ok((out, task_origin_info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::{StdEnvResolver, StdFileSystem, StdRuntimeCatalog};
    use common::ports::outbound::{EnvResolver, FileSystem as FileSystemTrait, RuntimeCatalog};
    use std::fs;

    struct StubHooks {
        content: Option<String>,
    }

    impl ResolveSystemPromptFromHooks for StubHooks {
        fn resolve_system_prompt_from_hooks(&self) -> Result<Option<String>, Error> {
            Ok(self.content.clone())
        }
    }

    struct StubTaskSpecLoader;

    impl TaskSpecLoader for StubTaskSpecLoader {
        fn load_task_spec(
            &self,
            _task_root: &Path,
            _task_name: &TaskName,
        ) -> Result<Option<TaskSpec>, Error> {
            Ok(None)
        }
    }

    struct StubSkillSpecLoader {
        specs: Vec<(String, SkillSpec)>,
    }

    impl SkillSpecLoader for StubSkillSpecLoader {
        fn load_skill_spec(
            &self,
            _skills_root: &Path,
            skill_name: &str,
        ) -> Result<Option<SkillSpec>, Error> {
            for (name, spec) in &self.specs {
                if name == skill_name {
                    return Ok(Some(spec.clone()));
                }
            }
            Ok(None)
        }

        fn list_skill_specs(&self, _skills_root: &Path) -> Result<Vec<SkillSpec>, Error> {
            Ok(self.specs.iter().map(|(_, s)| s.clone()).collect())
        }
    }

    struct EmptyPackages;

    impl crate::ports::outbound::PackageResolver for EmptyPackages {
        fn list_packages(&self) -> Result<Vec<crate::domain::ResolvedPackage>, Error> {
            Ok(Vec::new())
        }

        fn resolve_package(
            &self,
            _name: &str,
        ) -> Result<Option<crate::domain::ResolvedPackage>, Error> {
            Ok(None)
        }
    }

    #[test]
    fn hooks_only_when_no_task() {
        let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
        let fs_adapter: Arc<dyn FileSystemTrait> = Arc::new(StdFileSystem);
        let catalog: Arc<dyn RuntimeCatalog> = Arc::new(StdRuntimeCatalog::new(
            Arc::clone(&env),
            Arc::clone(&fs_adapter),
        ));

        let hooks = Arc::new(StubHooks {
            content: Some("From hooks".to_string()),
        });
        let task_specs: Arc<dyn TaskSpecLoader> = Arc::new(StubTaskSpecLoader);
        let skill_specs: Arc<dyn SkillSpecLoader> = Arc::new(StubSkillSpecLoader { specs: vec![] });
        let packages: Arc<dyn crate::ports::outbound::PackageResolver> = Arc::new(EmptyPackages);

        let resolver = StdPromptSourceResolver::new(
            Arc::clone(&fs_adapter),
            Arc::clone(&catalog),
            hooks,
            task_specs,
            skill_specs,
            packages,
        );

        let name = TaskName::new("nonexistent_task");
        let (sources, task_origin) = resolver.resolve_for_task(&name, None).unwrap();
        assert!(task_origin.is_none());
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].kind, PromptSourceKind::Hook);
        assert_eq!(sources[0].content, "From hooks");
    }

    #[test]
    fn hooks_task_prompt_and_skills_are_composed_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        // task.d/config
        let task_dir = root.join("task.d");
        fs::create_dir_all(&task_dir).unwrap();
        let task_name = TaskName::new("investigate_ci_failure");
        let task_exec_dir = task_dir.join(task_name.as_ref());
        fs::create_dir_all(&task_exec_dir).unwrap();
        fs::write(task_exec_dir.join("execute"), "#!/bin/sh\nexit 0\n").unwrap();
        fs::write(task_exec_dir.join("prompt.md"), "Task prompt content").unwrap();

        // skills/
        let skills_root = root.join(".aish").join("skills");
        fs::create_dir_all(&skills_root).unwrap();
        let skill_dir = skills_root.join("analyze_test_failure");
        fs::create_dir_all(&skill_dir).unwrap();
        let instructions_path = skill_dir.join("prompt.md");
        fs::write(&instructions_path, "Skill prompt content").unwrap();

        let fs_adapter: Arc<dyn FileSystemTrait> = Arc::new(StdFileSystem);

        // RuntimeCatalog を、tasks と skills の場所を固定で返すスタブにする。
        struct StubCatalog {
            task_dir: std::path::PathBuf,
            skills_dir: std::path::PathBuf,
        }

        impl RuntimeCatalog for StubCatalog {
            fn locations(
                &self,
                kind: common::domain::CatalogKind,
            ) -> Result<Vec<common::domain::CatalogLocation>, Error> {
                use common::domain::{CatalogKind, CatalogLocation, CatalogScope};
                let mut v = Vec::new();
                match kind {
                    CatalogKind::Tasks => v.push(CatalogLocation {
                        kind,
                        scope: CatalogScope::UserConfig,
                        path: self.task_dir.clone(),
                    }),
                    CatalogKind::Skills => v.push(CatalogLocation {
                        kind,
                        scope: CatalogScope::Project,
                        path: self.skills_dir.clone(),
                    }),
                    _ => {}
                }
                Ok(v)
            }

            fn project_root(&self) -> Result<Option<std::path::PathBuf>, Error> {
                Ok(None)
            }
        }

        let catalog: Arc<dyn RuntimeCatalog> = Arc::new(StubCatalog {
            task_dir: task_dir.clone(),
            skills_dir: skills_root.clone(),
        });

        let hooks = Arc::new(StubHooks {
            content: Some("Hook content".to_string()),
        });
        let task_spec = TaskSpec {
            name: task_name.clone(),
            description: None,
            skills: vec!["analyze_test_failure".to_string()],
            memory_topics: vec!["ci".to_string()],
            preferred_mode: Some("readonly".to_string()),
        };
        struct FixedTaskSpecLoader(TaskSpec);
        impl TaskSpecLoader for FixedTaskSpecLoader {
            fn load_task_spec(
                &self,
                _task_root: &Path,
                _task_name: &TaskName,
            ) -> Result<Option<TaskSpec>, Error> {
                Ok(Some(self.0.clone()))
            }
        }
        let task_specs: Arc<dyn TaskSpecLoader> = Arc::new(FixedTaskSpecLoader(task_spec.clone()));

        let skill_spec = SkillSpec {
            name: "analyze_test_failure".to_string(),
            description: None,
            instructions_path: instructions_path.clone(),
            allowed_tools: vec!["read_file".to_string()],
            denied_tools: vec![],
            memory_topics: vec!["tests".to_string()],
            preferred_mode: Some("readonly".to_string()),
        };
        let skills = StubSkillSpecLoader {
            specs: vec![("analyze_test_failure".to_string(), skill_spec.clone())],
        };
        let skill_specs: Arc<dyn SkillSpecLoader> = Arc::new(skills);
        let packages: Arc<dyn crate::ports::outbound::PackageResolver> = Arc::new(EmptyPackages);

        let resolver = StdPromptSourceResolver::new(
            Arc::clone(&fs_adapter),
            Arc::clone(&catalog),
            hooks,
            task_specs,
            skill_specs,
            packages,
        );

        let (sources, task_origin) = resolver.resolve_for_task(&task_name, None).unwrap();
        assert!(task_origin.is_some());
        assert_eq!(sources.len(), 3);
        assert_eq!(sources[0].kind, PromptSourceKind::Hook);
        assert_eq!(sources[0].content, "Hook content");
        assert_eq!(sources[1].kind, PromptSourceKind::TaskPrompt);
        assert_eq!(sources[1].content, "Task prompt content");
        assert_eq!(sources[2].kind, PromptSourceKind::Skill);
        assert_eq!(sources[2].content, "Skill prompt content");
    }
}
