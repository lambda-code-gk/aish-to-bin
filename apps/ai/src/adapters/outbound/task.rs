use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::domain::CatalogKind;
use common::error::Error;
use common::ports::outbound::{FileSystem, Process, ProcessOutputObserver, RuntimeCatalog};

use crate::domain::task::resolution;
use crate::domain::TaskName;
use crate::ports::outbound::{PackageResolver, TaskRunner};

/// TaskRunner の標準実装（run_task_if_exists をラップし、package 由来タスクも探索する）
pub struct StdTaskRunner {
    fs: Arc<dyn FileSystem>,
    process: Arc<dyn Process>,
    catalog: Arc<dyn RuntimeCatalog>,
    packages: Arc<dyn PackageResolver>,
    output_observer: Option<Arc<dyn ProcessOutputObserver>>,
}

impl StdTaskRunner {
    pub fn new(
        fs: Arc<dyn FileSystem>,
        process: Arc<dyn Process>,
        catalog: Arc<dyn RuntimeCatalog>,
        packages: Arc<dyn PackageResolver>,
        output_observer: Option<Arc<dyn ProcessOutputObserver>>,
    ) -> Self {
        Self {
            fs,
            process,
            catalog,
            packages,
            output_observer,
        }
    }
}

impl TaskRunner for StdTaskRunner {
    fn run_if_exists(&self, task_name: &str, args: &[String]) -> Result<Option<i32>, Error> {
        // TaskName 規約に従ってパースする。無効な名前は「存在しないタスク」として扱う。
        let task_name = match TaskName::parse(task_name) {
            Ok(n) => n,
            Err(_) => return Ok(None),
        };

        // 1. 既存の task.d 由来タスクを優先して解決する
        if let Some(code) = run_task_if_exists(
            self.fs.as_ref(),
            self.process.as_ref(),
            self.catalog.as_ref(),
            &task_name,
            args,
            self.output_observer.clone(),
        )? {
            return Ok(Some(code));
        }

        // 2. 見つからない場合は package 配下の tasks/ を探索する
        run_task_in_packages(
            self.fs.as_ref(),
            self.process.as_ref(),
            self.packages.as_ref(),
            &task_name,
            args,
            self.output_observer.clone(),
        )
    }

    fn list_names(&self) -> Result<Vec<String>, Error> {
        let mut names = list_task_names(self.fs.as_ref(), self.catalog.as_ref())?;
        let mut from_packages = list_package_task_names(self.fs.as_ref(), self.packages.as_ref())?;
        names.append(&mut from_packages);
        names.sort();
        names.dedup();
        Ok(names)
    }
}

/// タスクを解決して実行する（アダプター経由）
///
/// task.d の検索順:
/// - config: resolve_dirs().config_dir/task.d
/// - legacy: $AISH_HOME/task.d
/// - XDG / HOME による config_dir の解決は EnvResolver に委譲
/// タスクは `task_name.sh` または `task_name/execute` で解決する。
///
/// 戻り値:
/// - `Ok(Some(code))`  : タスクを実行し、その終了コードを返した
/// - `Ok(None)`       : タスクが見つからなかった（呼び出し元で他の処理を行う）
/// - `Err(Error)`     : 実行時エラー
pub fn run_task_if_exists<F, P>(
    fs: &F,
    process: &P,
    catalog: &dyn RuntimeCatalog,
    task_name: &TaskName,
    args: &[String],
    output_observer: Option<Arc<dyn ProcessOutputObserver>>,
) -> Result<Option<i32>, Error>
where
    F: FileSystem + ?Sized,
    P: Process + ?Sized,
{
    if task_name.as_ref().is_empty() {
        return Ok(None);
    }

    let locations = catalog.locations(CatalogKind::Tasks)?;
    for loc in locations {
        let resolved = resolve_task_path(fs, &loc.path, task_name);
        if let Some(task_path) = resolved {
            let exit_status = process.run_observing(&task_path, args, output_observer.clone())?;
            return Ok(Some(exit_status));
        }
    }
    Ok(None)
}

/// package 配下の tasks/ からタスクを解決して実行する。
///
/// - PackageResolver.list_packages() が返す ResolvedPackage を順に見ていく。
/// - 各 package について root_dir/tasks を task ルートとして扱う。
pub fn run_task_in_packages<F, P>(
    fs: &F,
    process: &P,
    packages: &dyn PackageResolver,
    task_name: &TaskName,
    args: &[String],
    output_observer: Option<Arc<dyn ProcessOutputObserver>>,
) -> Result<Option<i32>, Error>
where
    F: FileSystem + ?Sized,
    P: Process + ?Sized,
{
    if task_name.as_ref().is_empty() {
        return Ok(None);
    }

    let resolved_packages = packages.list_packages()?;
    for pkg in resolved_packages {
        let task_root = pkg.spec.root_dir.join("tasks");
        if !fs.exists(&task_root) {
            continue;
        }
        if let Some(task_path) = resolve_task_path(fs, &task_root, task_name) {
            let exit_status = process.run_observing(&task_path, args, output_observer.clone())?;
            return Ok(Some(exit_status));
        }
    }
    Ok(None)
}

fn resolve_task_path<F: FileSystem + ?Sized>(
    fs: &F,
    task_dir: &Path,
    task_name: &TaskName,
) -> Option<PathBuf> {
    resolution::resolve_task_path(task_dir, task_name, |p| {
        fs.exists(p) && fs.metadata(p).map(|m| m.is_file()).unwrap_or(false)
    })
    .map(|r| r.path)
}

/// タスク名一覧を返す（task.d 内のディレクトリ名と .sh のベース名）。補完用。
fn list_task_names<F: FileSystem + ?Sized>(
    fs: &F,
    catalog: &dyn RuntimeCatalog,
) -> Result<Vec<String>, Error> {
    let locations = catalog.locations(CatalogKind::Tasks)?;
    let mut names = Vec::new();

    for loc in locations {
        let dir_entries = collect_dir_entries(fs, &loc.path)?;
        let mut extracted = resolution::extract_task_names(&dir_entries, |name| {
            let exec_path = loc.path.join(name.as_dir_name()).join("execute");
            fs.exists(&exec_path)
                && fs
                    .metadata(&exec_path)
                    .map(|m| m.is_file())
                    .unwrap_or(false)
        });
        names.extend(extracted.drain(..).map(|n| n.as_ref().to_string()));
    }

    names.sort();
    names.dedup();
    Ok(names)
}

/// package 配下の tasks/ からタスク名一覧を返す。補完用。
fn list_package_task_names<F: FileSystem + ?Sized>(
    fs: &F,
    packages: &dyn PackageResolver,
) -> Result<Vec<String>, Error> {
    let mut names = Vec::new();
    let resolved_packages = packages.list_packages()?;

    for pkg in resolved_packages {
        let task_dir = pkg.spec.root_dir.join("tasks");
        if !fs.exists(&task_dir) {
            continue;
        }
        let dir_entries = collect_dir_entries(fs, &task_dir)?;
        let mut extracted = resolution::extract_task_names(&dir_entries, |name| {
            let exec_path = task_dir.join(name.as_dir_name()).join("execute");
            fs.exists(&exec_path)
                && fs
                    .metadata(&exec_path)
                    .map(|m| m.is_file())
                    .unwrap_or(false)
        });
        names.extend(extracted.drain(..).map(|n| n.as_ref().to_string()));
    }

    names.sort();
    names.dedup();
    Ok(names)
}

/// read_dir の結果を `(name, is_dir)` のタプル列に変換するヘルパー。
fn collect_dir_entries<F: FileSystem + ?Sized>(
    fs: &F,
    dir: &Path,
) -> Result<Vec<(String, bool)>, Error> {
    let paths = fs.read_dir(dir)?;
    let mut entries = Vec::new();
    for path in paths {
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let full = dir.join(&name);
        let is_dir = fs.metadata(&full).map(|m| m.is_dir()).unwrap_or(false);
        entries.push((name, is_dir));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::{StdFileSystem, StdProcess};
    use common::domain::{CatalogLocation, CatalogScope};
    use std::fs::{self, File};

    struct StubCatalog {
        roots: Vec<PathBuf>,
    }

    impl RuntimeCatalog for StubCatalog {
        fn locations(&self, kind: CatalogKind) -> Result<Vec<CatalogLocation>, Error> {
            if kind != CatalogKind::Tasks {
                return Ok(Vec::new());
            }
            Ok(self
                .roots
                .iter()
                .cloned()
                .map(|path| CatalogLocation {
                    kind: CatalogKind::Tasks,
                    scope: CatalogScope::UserConfig,
                    path,
                })
                .collect())
        }

        fn project_root(&self) -> Result<Option<PathBuf>, Error> {
            Ok(None)
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
    fn test_run_task_not_found() {
        let tmp = std::env::temp_dir().join("aish_task_test_not_found");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let fs = StdFileSystem;
        let process = StdProcess;
        let catalog = StubCatalog { roots: vec![tmp] };
        let task_name = TaskName::new("unknown_task");
        let result = run_task_if_exists(&fs, &process, &catalog, &task_name, &[], None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_resolve_task_path_and_list_names() {
        let tmp = std::env::temp_dir().join("aish_task_test_list");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let task_dir = tmp.join("task.d");
        fs::create_dir_all(&task_dir).unwrap();

        // task as script: foo.sh
        let foo_sh = task_dir.join("foo.sh");
        File::create(&foo_sh).unwrap();
        // task as dir with execute: bar/execute
        let bar_dir = task_dir.join("bar");
        fs::create_dir_all(&bar_dir).unwrap();
        let bar_exec = bar_dir.join("execute");
        File::create(&bar_exec).unwrap();

        let fs = StdFileSystem;
        let catalog = StubCatalog {
            roots: vec![task_dir.clone()],
        };

        let names = list_task_names(&fs, &catalog).unwrap();
        assert!(names.contains(&"foo".to_string()));
        assert!(names.contains(&"bar".to_string()));
        assert_eq!(names.len(), 2);

        let resolved_foo =
            resolve_task_path(&fs, &task_dir, &TaskName::new("foo")).expect("foo should resolve");
        assert_eq!(resolved_foo, foo_sh);
        let resolved_bar =
            resolve_task_path(&fs, &task_dir, &TaskName::new("bar")).expect("bar should resolve");
        assert_eq!(resolved_bar, bar_exec);
    }
}

