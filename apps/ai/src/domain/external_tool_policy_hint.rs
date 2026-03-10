/// external tool 向けの最小 policy メタデータ
///
/// - tool_name: OpenAI tools[].function.name に渡すサニタイズ済み名
/// - canonical_tool_id: `namespace.tool` 形式の canonical id
/// - server_id: McpServerId（文字列表現）
/// - source: 設定ファイル等のソースパス
/// - default_tool_mode_hint: plugin 側からの既定モード hint（`allow` / `deny` / `require_approval` 等）
/// - capabilities_hint: `fs_read` / `fs_write` / `exec` / `network` 等の capability 種別
/// - notes: 人間向けメモ（explain / debug 用）
#[derive(Debug, Clone)]
pub struct ExternalToolPolicyHint {
    #[allow(dead_code)]
    pub tool_name: String,
    pub canonical_tool_id: String,
    pub server_id: String,
    pub source: Option<String>,
    pub default_tool_mode_hint: Option<String>,
    pub capabilities_hint: Vec<String>,
    #[allow(dead_code)]
    pub notes: Option<String>,
}

/// external tool の policy 向けメタデータ index。
///
/// key は OpenAI tools[].function.name に渡すサニタイズ済みツール名。
#[derive(Debug, Default)]
pub struct ExternalToolPolicyIndex {
    pub(crate) hints: std::collections::HashMap<String, ExternalToolPolicyHint>,
}

impl ExternalToolPolicyIndex {
    pub fn new(hints: std::collections::HashMap<String, ExternalToolPolicyHint>) -> Self {
        Self { hints }
    }

    pub fn get(&self, tool_name: &str) -> Option<&ExternalToolPolicyHint> {
        self.hints.get(tool_name)
    }

    pub fn is_external(&self, tool_name: &str) -> bool {
        self.hints.contains_key(tool_name)
    }
}
