//! policy explain ユースケース（port 経由で説明情報を取得）

use crate::domain::PolicyExplainInfo;
use crate::ports::outbound::PolicyExplainProvider;
use common::error::Error;
use std::sync::Arc;

pub struct PolicyUseCase {
    pub explainer: Arc<dyn PolicyExplainProvider>,
}

impl PolicyUseCase {
    pub fn new(explainer: Arc<dyn PolicyExplainProvider>) -> Self {
        Self { explainer }
    }

    pub fn explain(&self) -> Result<PolicyExplainInfo, Error> {
        self.explainer.explain()
    }
}
