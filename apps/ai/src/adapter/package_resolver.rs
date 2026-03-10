use crate::domain::ResolvedPackage;
use crate::ports::outbound::{PackageResolver, PackageSpecLoader};
use common::domain::{CatalogKind, CatalogLocation};
use common::error::Error;
use common::ports::outbound::RuntimeCatalog;
use std::sync::Arc;

pub struct StdPackageResolver {
    catalog: Arc<dyn RuntimeCatalog>,
    loader: Arc<dyn PackageSpecLoader>,
}

impl StdPackageResolver {
    pub fn new(catalog: Arc<dyn RuntimeCatalog>, loader: Arc<dyn PackageSpecLoader>) -> Self {
        Self { catalog, loader }
    }

    fn list_for_location(&self, loc: &CatalogLocation) -> Result<Vec<ResolvedPackage>, Error> {
        let specs = self.loader.list_package_specs(&loc.path)?;
        let mut out = Vec::with_capacity(specs.len());
        for spec in specs {
            out.push(ResolvedPackage {
                spec,
                scope: loc.scope,
            });
        }
        Ok(out)
    }
}

impl PackageResolver for StdPackageResolver {
    fn list_packages(&self) -> Result<Vec<ResolvedPackage>, Error> {
        let locations = self.catalog.locations(CatalogKind::Packages)?;
        let mut out = Vec::new();
        for loc in &locations {
            let mut pkgs = self.list_for_location(loc)?;
            out.append(&mut pkgs);
        }
        Ok(out)
    }

    fn resolve_package(&self, name: &str) -> Result<Option<ResolvedPackage>, Error> {
        if name.is_empty() {
            return Ok(None);
        }
        let locations = self.catalog.locations(CatalogKind::Packages)?;
        for loc in &locations {
            let specs = self.loader.list_package_specs(&loc.path)?;
            for spec in specs {
                if spec.name == name {
                    return Ok(Some(ResolvedPackage {
                        spec,
                        scope: loc.scope,
                    }));
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PackageSpec;
    use common::domain::CatalogScope;
    use std::path::PathBuf;

    struct StubLoader {
        specs_by_root: Vec<(PathBuf, Vec<PackageSpec>)>,
    }

    impl PackageSpecLoader for StubLoader {
        fn load_package_spec(
            &self,
            _package_root: &std::path::Path,
        ) -> Result<Option<PackageSpec>, Error> {
            Ok(None)
        }

        fn list_package_specs(
            &self,
            packages_root: &std::path::Path,
        ) -> Result<Vec<PackageSpec>, Error> {
            for (root, specs) in &self.specs_by_root {
                if root == packages_root {
                    return Ok(specs.clone());
                }
            }
            Ok(Vec::new())
        }
    }

    struct StubCatalog {
        locations: Vec<CatalogLocation>,
    }

    impl RuntimeCatalog for StubCatalog {
        fn locations(&self, kind: CatalogKind) -> Result<Vec<CatalogLocation>, Error> {
            if kind == CatalogKind::Packages {
                Ok(self.locations.clone())
            } else {
                Ok(Vec::new())
            }
        }

        fn project_root(&self) -> Result<Option<std::path::PathBuf>, Error> {
            Ok(None)
        }
    }

    fn mk_spec(name: &str, root: &PathBuf) -> PackageSpec {
        PackageSpec {
            name: name.to_string(),
            version: None,
            description: None,
            root_dir: root.clone(),
            system_hook: None,
            memory_topics: Vec::new(),
        }
    }

    #[test]
    fn list_packages_preserves_location_order() {
        let root1 = PathBuf::from("/proj/.aish/packages");
        let root2 = PathBuf::from("/config/packages");
        let specs1 = vec![mk_spec("pkg1", &root1)];
        let specs2 = vec![mk_spec("pkg2", &root2)];
        let loader = StubLoader {
            specs_by_root: vec![
                (root1.clone(), specs1.clone()),
                (root2.clone(), specs2.clone()),
            ],
        };
        let catalog = StubCatalog {
            locations: vec![
                CatalogLocation {
                    kind: CatalogKind::Packages,
                    scope: CatalogScope::Project,
                    path: root1.clone(),
                },
                CatalogLocation {
                    kind: CatalogKind::Packages,
                    scope: CatalogScope::UserConfig,
                    path: root2.clone(),
                },
            ],
        };
        let resolver = StdPackageResolver::new(Arc::new(catalog), Arc::new(loader));
        let pkgs = resolver.list_packages().unwrap();
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].spec.name, "pkg1");
        assert_eq!(pkgs[0].scope, CatalogScope::Project);
        assert_eq!(pkgs[1].spec.name, "pkg2");
        assert_eq!(pkgs[1].scope, CatalogScope::UserConfig);
    }

    #[test]
    fn resolve_package_prefers_first_match() {
        let root1 = PathBuf::from("/proj/.aish/packages");
        let root2 = PathBuf::from("/config/packages");
        let specs1 = vec![mk_spec("ci-investigator", &root1)];
        let specs2 = vec![mk_spec("ci-investigator", &root2)];
        let loader = StubLoader {
            specs_by_root: vec![
                (root1.clone(), specs1.clone()),
                (root2.clone(), specs2.clone()),
            ],
        };
        let catalog = StubCatalog {
            locations: vec![
                CatalogLocation {
                    kind: CatalogKind::Packages,
                    scope: CatalogScope::Project,
                    path: root1.clone(),
                },
                CatalogLocation {
                    kind: CatalogKind::Packages,
                    scope: CatalogScope::UserConfig,
                    path: root2.clone(),
                },
            ],
        };
        let resolver = StdPackageResolver::new(Arc::new(catalog), Arc::new(loader));
        let pkg = resolver
            .resolve_package("ci-investigator")
            .unwrap()
            .expect("pkg");
        assert_eq!(pkg.spec.name, "ci-investigator");
        assert_eq!(pkg.scope, CatalogScope::Project);
    }
}
