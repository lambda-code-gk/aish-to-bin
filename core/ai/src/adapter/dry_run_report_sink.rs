//! dry run 結果の出力 adapter（stdout に人間向けフォーマットで出力）

use crate::domain::DryRunInfo;
use crate::ports::outbound::DryRunReportSink;
use common::error::Error;

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
        println!("=== ai dry run ===");
        println!("profile: {}", info.profile_name);
        println!("model: {}", info.model_name);
        if let Some(ref s) = info.system_instruction {
            println!("system_instruction: |");
            for line in s.lines() {
                println!("  {}", line);
            }
        } else {
            println!("system_instruction: (none)");
        }
        match &info.mode_name {
            Some(m) => println!("mode: {}", m),
            None => println!("mode: (none)"),
        }
        println!("leakscan_enabled: {}", info.leakscan_enabled);
        match &info.tool_allowlist {
            Some(list) => println!("tool_allowlist: [{}]", list.join(", ")),
            None => println!("tool_allowlist: (all)"),
        }
        println!("tools_enabled: [{}]", info.tools_enabled.join(", "));
        println!("--- messages ({} total) ---", info.messages.len());
        for (i, m) in info.messages.iter().enumerate() {
            let (role, content) = match m {
                common::msg::Msg::System(s) => ("system", s.as_str()),
                common::msg::Msg::User(s) => ("user", s.as_str()),
                common::msg::Msg::Assistant(s) => ("assistant", s.as_str()),
                common::msg::Msg::ToolCall { call_id, name, args, .. } => {
                    let args_str = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
                    println!("  [{}] tool_call id={} name={} args={}", i, call_id, name, args_str);
                    continue;
                }
                common::msg::Msg::ToolResult { call_id, name, result } => {
                    let res_str = serde_json::to_string(result).unwrap_or_else(|_| "{}".to_string());
                    println!("  [{}] tool_result id={} name={} result={}", i, call_id, name, res_str);
                    continue;
                }
            };
            println!("  [{}] {}:", i, role);
            for line in content.lines() {
                println!("    {}", line);
            }
        }
        if let Some(count) = info.attachments_count {
            println!("attachments_count: {} (dry-run: artifact 保存なし)", count);
        }
        if let Some(ref report) = info.budget_report {
            println!("--- budget report ---");
            println!(
                "budget: max_messages={} max_chars={}",
                report.budget.max_messages, report.budget.max_chars
            );
            println!(
                "input: messages={} chars={}",
                report.input.message_count, report.input.char_count
            );
            println!(
                "output: messages={} chars={}",
                report.output.message_count, report.output.char_count
            );
            let addon_keeps = report.decisions.iter().filter(|d| d.stage == "addon.select" && d.action == "keep").count();
            let addon_drops = report.decisions.iter().filter(|d| d.stage == "addon.select" && d.action == "drop").count();
            let addon_errors = report.decisions.iter().filter(|d| d.stage == "addon.selector" && d.action == "error").count();
            if addon_keeps > 0 || addon_drops > 0 || addon_errors > 0 {
                println!("addons: {} kept, {} dropped, {} selector error(s)", addon_keeps, addon_drops, addon_errors);
            }
            for d in &report.decisions {
                println!(
                    "decision: stage={} action={} reason={}",
                    d.stage, d.action, d.reason
                );
            }
        }
        println!("=== end dry run ===");
        Ok(())
    }
}
