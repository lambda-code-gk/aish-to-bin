//! メモリ用ディレクトリ解決の標準実装
//!
//! カレントから上に遡って .aish/memory を探し、無ければグローバル（EnvResolver::resolve_dirs().data_dir/memory）を返す。

use crate::ports::outbound::ResolveMemoryDir;
use common::error::Error;
use common::ports::outbound::{EnvResolver, RuntimeCatalog};
use std::path::PathBuf;
use std::sync::Arc;

const MEMORY_SUBDIR: &str = "memory";
const AISH_DIR: &str = ".aish";

pub struct StdResolveMemoryDir {
    env: Arc<dyn EnvResolver>,
    catalog: Arc<dyn RuntimeCatalog>,
}

impl StdResolveMemoryDir {
    pub fn new(env: Arc<dyn EnvResolver>, catalog: Arc<dyn RuntimeCatalog>) -> Self {
        Self { env, catalog }
    }
}

impl ResolveMemoryDir for StdResolveMemoryDir {
    fn resolve(&self) -> Result<(Option<PathBuf>, PathBuf), Error> {
        // ディレクトリ解決は EnvResolver::resolve_dirs() に集約し、home を data/config の「root」として扱わない
        let dirs = self.env.resolve_dirs()?;
        let global = dirs.data_dir.join(MEMORY_SUBDIR);

        let project = match self.catalog.project_root()? {
            Some(root) => {
                let candidate = root.join(AISH_DIR).join(MEMORY_SUBDIR);
                if candidate.exists() {
                    let meta = std::fs::metadata(&candidate).map_err(|e| {
                        Error::io_msg(format!("metadata {}: {}", candidate.display(), e))
                    })?;
                    if meta.is_dir() {
                        Some(candidate)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            None => None,
        };
        Ok((project, global))
    }
}
