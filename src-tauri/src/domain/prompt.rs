use serde::{Deserialize, Serialize};

use super::{ContextPlacement, PromptContext};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PromptContextSnapshot {
    pub id: String,
    pub name: String,
    pub content: String,
    pub placement: ContextPlacement,
    pub prefix_content: String,
    pub suffix_content: String,
    pub negative_content: String,
    pub sort_order: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComposedPrompt {
    pub raw_prompt: String,
    pub final_prompt: String,
    pub negative_prompt: String,
    pub contexts: Vec<PromptContextSnapshot>,
}

pub fn compose_prompt(raw_prompt: &str, contexts: &[PromptContext]) -> ComposedPrompt {
    let mut active: Vec<_> = contexts
        .iter()
        .filter(|context| context.enabled && context.has_content())
        .collect();
    active.sort_by(|left, right| {
        left.sort_order
            .cmp(&right.sort_order)
            .then_with(|| left.id.cmp(&right.id))
    });

    let snapshots = active
        .iter()
        .map(|context| PromptContextSnapshot {
            id: context.id.clone(),
            name: context.name.clone(),
            content: context.content.trim().to_owned(),
            placement: context.placement,
            prefix_content: context.resolved_prefix().trim().to_owned(),
            suffix_content: context.resolved_suffix().trim().to_owned(),
            negative_content: context.negative_content.trim().to_owned(),
            sort_order: context.sort_order,
        })
        .collect::<Vec<_>>();

    let mut fragments = active
        .iter()
        .map(|context| context.resolved_prefix().trim())
        .filter(|content| !content.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if !raw_prompt.trim().is_empty() {
        fragments.push(raw_prompt.trim().to_owned());
    }
    fragments.extend(
        active
            .iter()
            .map(|context| context.resolved_suffix().trim())
            .filter(|content| !content.is_empty())
            .map(ToOwned::to_owned),
    );
    let negative_prompt = active
        .iter()
        .map(|context| context.negative_content.trim())
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    ComposedPrompt {
        raw_prompt: raw_prompt.to_owned(),
        final_prompt: fragments.join("\n\n"),
        negative_prompt,
        contexts: snapshots,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn context(id: &str, content: &str, placement: ContextPlacement, order: i32) -> PromptContext {
        PromptContext {
            id: id.to_owned(),
            name: id.to_owned(),
            content: content.to_owned(),
            placement,
            prefix_content: String::new(),
            suffix_content: String::new(),
            negative_content: String::new(),
            sort_order: order,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn composes_contexts_in_stable_order() {
        let contexts = vec![
            context("b", "anime", ContextPlacement::Append, 0),
            context("a", "post-millennium", ContextPlacement::Prepend, 10),
            context("c", "cinematic", ContextPlacement::Prepend, -1),
        ];

        let composed = compose_prompt("a city at night", &contexts);

        assert_eq!(
            composed.final_prompt,
            "cinematic\n\npost-millennium\n\na city at night\n\nanime"
        );
        assert_eq!(composed.contexts.len(), 3);
    }

    #[test]
    fn ignores_disabled_and_blank_contexts() {
        let mut disabled = context("disabled", "hidden", ContextPlacement::Prepend, 0);
        disabled.enabled = false;
        let blank = context("blank", "  ", ContextPlacement::Append, 0);

        let composed = compose_prompt(" subject ", &[disabled, blank]);

        assert_eq!(composed.final_prompt, "subject");
        assert!(composed.contexts.is_empty());
    }

    #[test]
    fn composes_all_context_parts_in_stable_group_order() {
        let mut first = context("first", "", ContextPlacement::Prepend, 0);
        first.prefix_content = "prefix one".to_owned();
        first.suffix_content = "suffix one".to_owned();
        first.negative_content = "negative one".to_owned();
        let mut second = context("second", "", ContextPlacement::Prepend, 1);
        second.prefix_content = "prefix two".to_owned();
        second.suffix_content = "suffix two".to_owned();
        second.negative_content = "negative two".to_owned();

        let composed = compose_prompt("subject", &[second, first]);

        assert_eq!(
            composed.final_prompt,
            "prefix one\n\nprefix two\n\nsubject\n\nsuffix one\n\nsuffix two"
        );
        assert_eq!(composed.negative_prompt, "negative one\nnegative two");
    }
}
