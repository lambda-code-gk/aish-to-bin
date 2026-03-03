use crate::domain::{ToolMode, ToolProfile};
use crate::ports::outbound::ToolProfileProvider;
use std::collections::HashMap;

/// テスト・既存 PolicyEngine 用。本番 wiring は ConfigurableToolProfileProvider を使用。
#[allow(dead_code)]
pub struct StaticToolProfileProvider {
    profiles: HashMap<String, ToolProfile>,
}

#[allow(dead_code)]
impl StaticToolProfileProvider {
    pub fn new(profiles: HashMap<String, ToolProfile>) -> Self {
        Self { profiles }
    }
}

impl ToolProfileProvider for StaticToolProfileProvider {
    fn get(&self, tool_name: &str) -> ToolProfile {
        if let Some(p) = self.profiles.get(tool_name) {
            return p.clone();
        }
        ToolProfile {
            tool_name: tool_name.to_string(),
            mode: ToolMode::RequireApproval,
            capabilities: Vec::new(),
            notes: Some("default".to_string()),
        }
    }
}
