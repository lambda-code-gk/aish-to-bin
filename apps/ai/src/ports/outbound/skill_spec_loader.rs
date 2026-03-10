use crate::domain::SkillSpec;
use common::error::Error;
use std::path::Path;

/// skills ディレクトリ配下の skill.toml から SkillSpec を読み込むポート
pub trait SkillSpecLoader: Send + Sync {
    /// 単一の skill を読み込む。
    ///
    /// - skills_root/<name>/skill.toml を探す
    /// - ファイルが存在しない場合や壊れている場合は Ok(None)
    fn load_skill_spec(
        &self,
        skills_root: &Path,
        skill_name: &str,
    ) -> Result<Option<SkillSpec>, Error>;

    /// skills_root 配下の全 SkillSpec を列挙する。
    ///
    /// 壊れている skill.toml は warn してスキップする。
    #[allow(dead_code)]
    fn list_skill_specs(&self, skills_root: &Path) -> Result<Vec<SkillSpec>, Error>;
}
