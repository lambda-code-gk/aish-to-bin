//! 責務: PromptCandidate 群を AISH の規約にしたがって並べ替え・採用する判断ロジックのみ。I/O を知らない。

use crate::domain::prompt::PromptCandidate;

/// Prompt 構成の最終決定結果。
///
/// - ordered: 実際に使用する候補を決定済み順序で並べたもの
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptAssemblyDecision {
    pub ordered: Vec<PromptCandidate>,
}

impl PromptAssemblyDecision {
    /// hooks / package hook / task prompt / skills の順序規約に従って並べる。
    ///
    /// - hooks: global/system hooks（0..n）
    /// - package_hook: package 由来 system hook（0 or 1）
    /// - task_prompt: task.d/ 配下の prompt（0 or 1）
    /// - skills: task spec 由来 skills（0..n）
    pub fn assemble(
        mut hooks: Vec<PromptCandidate>,
        package_hook: Option<PromptCandidate>,
        task_prompt: Option<PromptCandidate>,
        mut skills: Vec<PromptCandidate>,
    ) -> Self {
        let mut ordered = Vec::new();
        ordered.append(&mut hooks);
        if let Some(p) = package_hook {
            ordered.push(p);
        }
        if let Some(t) = task_prompt {
            ordered.push(t);
        }
        ordered.append(&mut skills);
        PromptAssemblyDecision { ordered }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PromptSourceKind, ResolvedPromptSource, SourceProvenance};

    fn mk(name: &str, kind: PromptSourceKind) -> PromptCandidate {
        let resolved = ResolvedPromptSource {
            kind,
            name: name.to_string(),
            content: format!("{name} content"),
            memory_topics: Vec::new(),
            preferred_mode: None,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            provenance: Some(SourceProvenance::new(
                common::domain::CatalogScope::UserConfig,
                None,
                std::path::PathBuf::from(name),
                None,
            )),
        };
        resolved.into()
    }

    #[test]
    fn assemble_orders_sources_according_to_aish_rules() {
        let hooks = vec![
            mk("hook1", PromptSourceKind::Hook),
            mk("hook2", PromptSourceKind::Hook),
        ];
        let package_hook = Some(mk("pkg", PromptSourceKind::Hook));
        let task_prompt = Some(mk("task", PromptSourceKind::TaskPrompt));
        let skills = vec![
            mk("skill_a", PromptSourceKind::Skill),
            mk("skill_b", PromptSourceKind::Skill),
        ];

        let decision = PromptAssemblyDecision::assemble(hooks, package_hook, task_prompt, skills);
        let names: Vec<String> = decision.ordered.iter().map(|c| c.name.clone()).collect();

        assert_eq!(
            names,
            vec![
                "hook1".to_string(),
                "hook2".to_string(),
                "pkg".to_string(),
                "task".to_string(),
                "skill_a".to_string(),
                "skill_b".to_string()
            ]
        );
    }

    #[test]
    fn assemble_works_with_missing_optional_sources() {
        let hooks = Vec::new();
        let package_hook = None;
        let task_prompt = None;
        let skills = vec![mk("skill_only", PromptSourceKind::Skill)];

        let decision = PromptAssemblyDecision::assemble(hooks, package_hook, task_prompt, skills);
        let names: Vec<String> = decision.ordered.iter().map(|c| c.name.clone()).collect();
        assert_eq!(names, vec!["skill_only".to_string()]);
    }
}
