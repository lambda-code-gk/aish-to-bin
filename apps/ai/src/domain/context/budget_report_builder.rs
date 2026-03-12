//! 責務: ContextBudget と入出力メトリクス・決定一覧から BudgetReport を組み立てるのみ。I/O を知らない。

use crate::domain::{Budget, BudgetDecision, BudgetReport, BudgetStats, ContextBudget};

/// ContextBudget と監査用メトリクスから BudgetReport を構築する単純なビルダー関数。
pub fn build_budget_report(
    budget: ContextBudget,
    input_count: usize,
    input_chars: usize,
    output_count: usize,
    output_chars: usize,
    decisions: Vec<BudgetDecision>,
) -> BudgetReport {
    BudgetReport {
        v: 1,
        budget: Budget {
            max_messages: budget.max_messages,
            max_chars: budget.max_chars,
        },
        input: BudgetStats {
            message_count: input_count,
            char_count: input_chars,
        },
        output: BudgetStats {
            message_count: output_count,
            char_count: output_chars,
        },
        decisions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_budget_report_from_inputs() {
        let budget = ContextBudget {
            max_messages: 100,
            max_chars: 10_000,
        };
        let decisions = vec![BudgetDecision {
            stage: "test".to_string(),
            action: "keep".to_string(),
            reason: "unit_test".to_string(),
            details: serde_json::json!({ "k": "v" }),
        }];

        let report = build_budget_report(budget, 10, 1000, 8, 800, decisions.clone());

        assert_eq!(report.v, 1);
        assert_eq!(report.budget.max_messages, 100);
        assert_eq!(report.budget.max_chars, 10_000);
        assert_eq!(report.input.message_count, 10);
        assert_eq!(report.input.char_count, 1000);
        assert_eq!(report.output.message_count, 8);
        assert_eq!(report.output.char_count, 800);
        assert_eq!(report.decisions, decisions);
    }
}
