//! dry run 結果の出力 adapter（stdout に人間向けフォーマットで出力）

use crate::domain::{DryRunInfo, PromptSourceKind};
use crate::ports::outbound::DryRunReportSink;
use common::domain::CatalogScope;
use common::error::Error;

fn scope_label(scope: CatalogScope) -> &'static str {
    match scope {
        CatalogScope::Project => "project",
        CatalogScope::UserConfig => "config",
        CatalogScope::LegacyUser => "legacy",
        CatalogScope::System => "system",
    }
}

pub(crate) fn render_dry_run_report(info: &DryRunInfo) -> String {
    let mut out = String::new();
    out.push_str("=== ai dry run ===\n");
    out.push_str(&format!("profile: {}\n", info.profile_name));
    out.push_str(&format!("model: {}\n", info.model_name));
    if let Some(ref s) = info.system_instruction {
        out.push_str("system_instruction: |\n");
        for line in s.lines() {
            out.push_str(&format!("  {}\n", line));
        }
    } else {
        out.push_str("system_instruction: (none)\n");
    }
    match &info.mode_name {
        Some(m) => out.push_str(&format!("mode: {}\n", m)),
        None => out.push_str("mode: (none)\n"),
    }
    out.push_str(&format!("leakscan_enabled: {}\n", info.leakscan_enabled));
    match &info.tool_allowlist {
        Some(list) => out.push_str(&format!("tool_allowlist: [{}]\n", list.join(", "))),
        None => out.push_str("tool_allowlist: (all)\n"),
    }
    out.push_str(&format!(
        "tools_enabled: [{}]\n",
        info.tools_enabled.join(", ")
    ));

    if info.task_origin.is_some() || info.prompt_sources.as_ref().is_some_and(|v| !v.is_empty()) {
        out.push_str("--- source provenance ---\n");
        if let Some(ref to) = info.task_origin {
            let p = &to.provenance;
            let scope = scope_label(p.scope);
            let package = p.package_name.as_deref().unwrap_or("(none)");
            if let Some(ref note) = p.note {
                out.push_str(&format!(
                    "task: name={} scope={} package={} path={} note={}\n",
                    to.task_name,
                    scope,
                    package,
                    p.path.display(),
                    note
                ));
            } else {
                out.push_str(&format!(
                    "task: name={} scope={} package={} path={}\n",
                    to.task_name,
                    scope,
                    package,
                    p.path.display()
                ));
            }
        }
        if let Some(ref sources) = info.prompt_sources {
            for (i, s) in sources.iter().enumerate() {
                let kind_label = match s.kind {
                    PromptSourceKind::Hook => "hook",
                    PromptSourceKind::TaskPrompt => "task_prompt",
                    PromptSourceKind::Skill => "skill",
                };
                let (scope, package, path, note) = match &s.provenance {
                    Some(p) => (
                        scope_label(p.scope),
                        p.package_name.as_deref().unwrap_or("(none)"),
                        p.path.display().to_string(),
                        p.note.as_deref().unwrap_or("").to_string(),
                    ),
                    None => ("(unknown)", "(none)", "(none)".to_string(), String::new()),
                };
                let note_suffix = if note.is_empty() {
                    String::new()
                } else {
                    format!(" note={}", note)
                };
                out.push_str(&format!(
                    "  [{}] {} name={} scope={} package={} path={}{}\n",
                    i, kind_label, s.name, scope, package, path, note_suffix
                ));
            }
        }
    }

    out.push_str(&format!(
        "--- messages ({} total) ---\n",
        info.messages.len()
    ));
    for (i, m) in info.messages.iter().enumerate() {
        let (role, content) = match m {
            common::msg::Msg::System(s) => ("system", s.as_str()),
            common::msg::Msg::User(s) => ("user", s.as_str()),
            common::msg::Msg::Assistant(s) => ("assistant", s.as_str()),
            common::msg::Msg::ToolCall {
                call_id,
                name,
                args,
                ..
            } => {
                let args_str = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
                out.push_str(&format!(
                    "  [{}] tool_call id={} name={} args={}\n",
                    i, call_id, name, args_str
                ));
                continue;
            }
            common::msg::Msg::ToolResult {
                call_id,
                name,
                result,
            } => {
                let res_str = serde_json::to_string(result).unwrap_or_else(|_| "{}".to_string());
                out.push_str(&format!(
                    "  [{}] tool_result id={} name={} result={}\n",
                    i, call_id, name, res_str
                ));
                continue;
            }
        };
        out.push_str(&format!("  [{}] {}:\n", i, role));
        for line in content.lines() {
            out.push_str(&format!("    {}\n", line));
        }
    }
    if let Some(count) = info.attachments_count {
        out.push_str(&format!(
            "attachments_count: {} (dry-run: artifact 保存なし)\n",
            count
        ));
    }
    if let Some(ref report) = info.budget_report {
        out.push_str("--- budget report ---\n");
        out.push_str(&format!(
            "budget: max_messages={} max_chars={}\n",
            report.budget.max_messages, report.budget.max_chars
        ));
        out.push_str(&format!(
            "input: messages={} chars={}\n",
            report.input.message_count, report.input.char_count
        ));
        out.push_str(&format!(
            "output: messages={} chars={}\n",
            report.output.message_count, report.output.char_count
        ));
        let addon_keeps = report
            .decisions
            .iter()
            .filter(|d| d.stage == "addon.select" && d.action == "keep")
            .count();
        let addon_drops = report
            .decisions
            .iter()
            .filter(|d| d.stage == "addon.select" && d.action == "drop")
            .count();
        let addon_errors = report
            .decisions
            .iter()
            .filter(|d| d.stage == "addon.selector" && d.action == "error")
            .count();
        if addon_keeps > 0 || addon_drops > 0 || addon_errors > 0 {
            out.push_str(&format!(
                "addons: {} kept, {} dropped, {} selector error(s)\n",
                addon_keeps, addon_drops, addon_errors
            ));
        }
        for d in &report.decisions {
            out.push_str(&format!(
                "decision: stage={} action={} reason={}\n",
                d.stage, d.action, d.reason
            ));
        }
    }
    out.push_str("=== end dry run ===\n");
    out
}

/// dry run の結果を stdout に出力する adapter
pub struct StdoutDryRunReportSink;

impl StdoutDryRunReportSink {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StdoutDryRunReportSink {
    fn default() -> Self {
        Self::new()
    }
}

impl DryRunReportSink for StdoutDryRunReportSink {
    fn report(&self, info: &DryRunInfo) -> Result<(), Error> {
        print!("{}", render_dry_run_report(info));
        Ok(())
    }
}
