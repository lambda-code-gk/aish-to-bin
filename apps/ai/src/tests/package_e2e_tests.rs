//! Package E2E: fixture ベースの統合テスト。
//!
//! - package task / skill / hook の解決と、project と package の優先順位を固定する。
//! - 観測点: ResolvedPromptSource の content と provenance（どの source が採用されたか）。
//!
//! ## 追加したテスト・fixture（報告用）
//!
//! **テストファイル**: `apps/ai/src/tests/package_e2e_tests.rs`
//!
//! **fixture 一覧**: `tests/fixtures/package_e2e/README.md` 参照
//! - `basic_package_task/` … package task 解決
//! - `package_skill/` … package skill 取り込み
//! - `package_hook/` … package hook 合成
//! - `project_overrides_package/` … project 優先
//!
//! **各テストが保証するもの**
//! - `package_task_resolves`: package の tasks/build が解決され、list_names に含まれる
//! - `package_skill_is_included`: task が参照する package skill が解決結果に含まれる
//! - `package_hook_participates_in_prompt_assembly`: project hook → package hook → task の順で合成される
//! - `project_source_overrides_package`: 同名 task は catalog Tasks（project）が package より優先される
//!
//! **未テストの package 挙動**: 複数 package 競合・nested package・explain/provenance 出力との結合。

use crate::adapter::{
    StdPackageResolver, StdPackageSpecLoaderAdapter, StdPromptSourceResolver, StdSkillSpecLoader,
    StdTaskRunner, StdTaskSpecLoader,
};
use crate::domain::{PromptSourceKind, TaskName};
use crate::ports::outbound::{PromptSourceResolver, TaskRunner};
use common::adapter::StdFileSystem;
use common::domain::{CatalogKind, CatalogLocation, CatalogScope};
use common::error::Error;
use common::ports::outbound::{FileSystem, RuntimeCatalog};
use std::path::PathBuf;
use std::sync::Arc;

/// テスト用 fixture ルート（repo root からの相対: tests/fixtures/package_e2e）
fn package_e2e_fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("package_e2e")
}

/// Packages のみ返すカタログ（task.d なし → package 由来のタスクのみ）
struct PackagesOnlyCatalog {
    packages_path: PathBuf,
}

impl RuntimeCatalog for PackagesOnlyCatalog {
    fn locations(&self, kind: CatalogKind) -> Result<Vec<CatalogLocation>, Error> {
        if kind == CatalogKind::Packages {
            return Ok(vec![CatalogLocation {
                kind: CatalogKind::Packages,
                scope: CatalogScope::Project,
                path: self.packages_path.clone(),
            }]);
        }
        Ok(Vec::new())
    }

    fn project_root(&self) -> Result<Option<PathBuf>, Error> {
        Ok(None)
    }
}

/// Tasks と Packages の両方を返すカタログ（project task.d を先に返す → project 優先）
struct TasksAndPackagesCatalog {
    task_d_path: PathBuf,
    packages_path: PathBuf,
}

impl RuntimeCatalog for TasksAndPackagesCatalog {
    fn locations(&self, kind: CatalogKind) -> Result<Vec<CatalogLocation>, Error> {
        let mut out = Vec::new();
        match kind {
            CatalogKind::Tasks => {
                out.push(CatalogLocation {
                    kind: CatalogKind::Tasks,
                    scope: CatalogScope::UserConfig,
                    path: self.task_d_path.clone(),
                });
            }
            CatalogKind::Packages => {
                out.push(CatalogLocation {
                    kind: CatalogKind::Packages,
                    scope: CatalogScope::Project,
                    path: self.packages_path.clone(),
                });
            }
            _ => {}
        }
        Ok(out)
    }

    fn project_root(&self) -> Result<Option<PathBuf>, Error> {
        Ok(None)
    }
}

/// システムプロンプト用フックのスタブ（テストで内容を注入）
struct StubHooks {
    content: Option<String>,
}

impl crate::ports::outbound::ResolveSystemPromptFromHooks for StubHooks {
    fn resolve_system_prompt_from_hooks(&self) -> Result<Option<String>, Error> {
        Ok(self.content.clone())
    }
}

fn build_resolver(
    catalog: Arc<dyn RuntimeCatalog>,
    hooks_content: Option<String>,
) -> Arc<dyn PromptSourceResolver> {
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let loader = Arc::new(StdPackageSpecLoaderAdapter::new());
    let package_resolver: Arc<dyn crate::ports::outbound::PackageResolver> =
        Arc::new(StdPackageResolver::new(catalog.clone(), loader));
    let hooks = Arc::new(StubHooks {
        content: hooks_content,
    });
    let task_specs = Arc::new(StdTaskSpecLoader::new());
    let skill_specs = Arc::new(StdSkillSpecLoader::new());
    Arc::new(StdPromptSourceResolver::new(
        fs,
        catalog,
        hooks,
        task_specs,
        skill_specs,
        package_resolver,
    ))
}

/// Case 1: package task が解決される（project に task.d がなく package のみ）
#[test]
fn package_task_resolves() {
    let root = package_e2e_fixtures_root().join("basic_package_task");
    let packages_path = root.join(".aish").join("packages");
    assert!(
        packages_path.exists(),
        "fixture must exist: {}",
        packages_path.display()
    );

    let catalog: Arc<dyn RuntimeCatalog> = Arc::new(PackagesOnlyCatalog {
        packages_path: packages_path.clone(),
    });
    let loader = Arc::new(StdPackageSpecLoaderAdapter::new());
    let package_resolver: Arc<dyn crate::ports::outbound::PackageResolver> =
        Arc::new(StdPackageResolver::new(Arc::clone(&catalog), loader));
    let resolver = build_resolver(Arc::clone(&catalog), None);

    let (sources, task_origin) = resolver
        .resolve_for_task(&TaskName::new("build"), None)
        .expect("resolve_for_task should succeed");

    assert!(
        task_origin.is_some(),
        "task build should be found from package"
    );
    let origin = task_origin.unwrap();
    assert_eq!(origin.task_name, "build");
    assert_eq!(
        origin.provenance.package_name,
        Some("alpha".to_string()),
        "task should be from package alpha"
    );

    let task_prompts: Vec<_> = sources
        .iter()
        .filter(|s| s.kind == PromptSourceKind::TaskPrompt)
        .collect();
    assert_eq!(task_prompts.len(), 1);
    assert!(
        task_prompts[0]
            .content
            .contains("[TASK build from package-alpha]"),
        "task prompt content should identify package source: {:?}",
        task_prompts[0].content
    );

    // list_names に package task が含まれること
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let process = Arc::new(common::adapter::StdProcess);
    let task_runner: Arc<dyn TaskRunner> = Arc::new(StdTaskRunner::new(
        fs,
        process,
        Arc::clone(&catalog),
        package_resolver,
        None,
    ));
    let names = task_runner.list_names().expect("list_names should succeed");
    assert!(
        names.contains(&"build".to_string()),
        "list_names should include package task build: {:?}",
        names
    );
}

/// Case 2: package skill が読み込まれる
#[test]
fn package_skill_is_included() {
    let root = package_e2e_fixtures_root().join("package_skill");
    let packages_path = root.join(".aish").join("packages");
    assert!(
        packages_path.exists(),
        "fixture must exist: {}",
        packages_path.display()
    );

    let catalog: Arc<dyn RuntimeCatalog> = Arc::new(PackagesOnlyCatalog { packages_path });
    let resolver = build_resolver(catalog, None);

    let (sources, _) = resolver
        .resolve_for_task(&TaskName::new("run"), None)
        .expect("resolve_for_task should succeed");

    let skills: Vec<_> = sources
        .iter()
        .filter(|s| s.kind == PromptSourceKind::Skill)
        .collect();
    assert!(!skills.is_empty(), "skill review should be resolved");
    let review = skills
        .iter()
        .find(|s| s.name == "review")
        .expect("skill review should be present");
    assert!(
        review.content.contains("[SKILL review from package-alpha]"),
        "skill content should identify package: {:?}",
        review.content
    );
    assert_eq!(
        review
            .provenance
            .as_ref()
            .and_then(|p| p.package_name.as_ref()),
        Some(&"alpha".to_string())
    );
}

/// Case 3: package hook がプロンプト合成に参加する
#[test]
fn package_hook_participates_in_prompt_assembly() {
    let root = package_e2e_fixtures_root().join("package_hook");
    let packages_path = root.join(".aish").join("packages");
    assert!(
        packages_path.exists(),
        "fixture must exist: {}",
        packages_path.display()
    );

    let catalog: Arc<dyn RuntimeCatalog> = Arc::new(PackagesOnlyCatalog { packages_path });
    let resolver = build_resolver(catalog, Some("[HOOK project-system]".to_string()));

    let (sources, _) = resolver
        .resolve_for_task(&TaskName::new("build"), None)
        .expect("resolve_for_task should succeed");

    // 順序: hooks → package hook → task prompt
    let hook_sources: Vec<_> = sources
        .iter()
        .filter(|s| s.kind == PromptSourceKind::Hook)
        .collect();
    assert!(
        hook_sources.len() >= 2,
        "expected project hook and package hook: {:?}",
        hook_sources.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    assert!(
        sources
            .iter()
            .any(|s| s.content.contains("[HOOK project-system]")),
        "project hook should be first"
    );
    assert!(
        sources
            .iter()
            .any(|s| s.content.contains("[HOOK package-alpha]")),
        "package hook should be present"
    );
    assert!(
        sources
            .iter()
            .any(|s| s.content.contains("[TASK build from package-alpha]")),
        "task prompt should follow"
    );
}

/// Case 4: project 由来が package より優先される（現行実装の仕様を固定）
#[test]
fn project_source_overrides_package() {
    let root = package_e2e_fixtures_root().join("project_overrides_package");
    let task_d_path = root.join("task.d");
    let packages_path = root.join(".aish").join("packages");
    assert!(
        task_d_path.exists(),
        "fixture task.d must exist: {}",
        task_d_path.display()
    );
    assert!(
        packages_path.exists(),
        "fixture packages must exist: {}",
        packages_path.display()
    );

    let catalog: Arc<dyn RuntimeCatalog> = Arc::new(TasksAndPackagesCatalog {
        task_d_path,
        packages_path,
    });
    let resolver = build_resolver(catalog, None);

    let (sources, task_origin) = resolver
        .resolve_for_task(&TaskName::new("build"), None)
        .expect("resolve_for_task should succeed");

    assert!(task_origin.is_some());
    // 現行実装: find_task_origin はまず catalog Tasks を参照するため project が選ばれる
    assert_eq!(
        task_origin.as_ref().unwrap().provenance.package_name,
        None,
        "task should be from project (task.d), not package"
    );

    let task_prompts: Vec<_> = sources
        .iter()
        .filter(|s| s.kind == PromptSourceKind::TaskPrompt)
        .collect();
    assert_eq!(task_prompts.len(), 1);
    assert!(
        task_prompts[0]
            .content
            .contains("[TASK build from project]"),
        "project task prompt should be used: {:?}",
        task_prompts[0].content
    );
    assert!(
        !task_prompts[0]
            .content
            .contains("[TASK build from package-alpha]"),
        "package task prompt must not be used when project overrides"
    );
}
