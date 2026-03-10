use crate::domain::ResolvedMemoryContext;
use common::error::Error;

/// topic / kind ベースでメモリを解決し、プロンプト用の短い要約を返すポート
pub trait MemoryContextResolver: Send + Sync {
    fn resolve(
        &self,
        project_topics: &[String],
        global_topics: &[String],
    ) -> Result<ResolvedMemoryContext, Error>;
}

