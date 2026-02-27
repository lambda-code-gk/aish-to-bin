use crate::domain::ConfigExplainInfo;
use crate::ports::outbound::ConfigExplainProvider;
use common::error::Error;
use std::sync::Arc;

pub struct ConfigUseCase {
    explainer: Arc<dyn ConfigExplainProvider>,
}

impl ConfigUseCase {
    pub fn new(explainer: Arc<dyn ConfigExplainProvider>) -> Self {
        Self { explainer }
    }

    pub fn explain(&self) -> Result<ConfigExplainInfo, Error> {
        self.explainer.explain()
    }
}

