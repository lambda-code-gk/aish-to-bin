//! history コマンドのユースケース（reviewed 履歴の一覧・取得）

use crate::domain::{HistoryGetEntry, HistoryListEntry};
use crate::ports::outbound::ReviewedHistoryReader;
use common::error::Error;
use common::ports::outbound::{PathResolver, PathResolverInput};
use common::session::Session;
use std::sync::Arc;

/// history コマンドのユースケース
pub struct HistoryUseCase {
    path_resolver: Arc<dyn PathResolver>,
    reader: Arc<dyn ReviewedHistoryReader>,
}

impl HistoryUseCase {
    pub fn new(
        path_resolver: Arc<dyn PathResolver>,
        reader: Arc<dyn ReviewedHistoryReader>,
    ) -> Self {
        Self {
            path_resolver,
            reader,
        }
    }

    /// 履歴一覧。セッションが明示指定されている必要あり。
    pub fn list(
        &self,
        path_input: &PathResolverInput,
        session_explicitly_specified: bool,
        all: bool,
        user_only: bool,
        assistant_only: bool,
    ) -> Result<Vec<HistoryListEntry>, Error> {
        if !session_explicitly_specified {
            return Err(Error::invalid_argument(
                "The 'history' command requires a session. Use -s/--session-dir, -d/--home-dir, or set AISH_SESSION.",
            ));
        }
        let session = self.resolve_session(path_input)?;
        self.reader.list_entries(
            session.session_dir().as_ref(),
            all,
            user_only,
            assistant_only,
        )
    }

    /// 指定 id の内容取得。セッションが明示指定されている必要あり。
    pub fn get(
        &self,
        path_input: &PathResolverInput,
        session_explicitly_specified: bool,
        ids: &[String],
    ) -> Result<Vec<HistoryGetEntry>, Error> {
        if !session_explicitly_specified {
            return Err(Error::invalid_argument(
                "The 'history' command requires a session. Use -s/--session-dir, -d/--home-dir, or set AISH_SESSION.",
            ));
        }
        if ids.is_empty() {
            return Err(Error::invalid_argument(
                "history get requires at least one id".to_string(),
            ));
        }
        let session = self.resolve_session(path_input)?;
        self.reader.get_entries(session.session_dir().as_ref(), ids)
    }

    fn resolve_session(&self, path_input: &PathResolverInput) -> Result<Session, Error> {
        let home_dir = self.path_resolver.resolve_home_dir(path_input)?;
        let session_path = self
            .path_resolver
            .resolve_session_dir(path_input, &home_dir)?;
        Session::new(&session_path, &home_dir)
    }
}
