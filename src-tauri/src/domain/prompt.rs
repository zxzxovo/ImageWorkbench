use serde::{Deserialize, Serialize};

use super::{ContextPlacement, PromptContext};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PromptContextSnapshot {
    pub id: String,
    pub name: String,
    pub content: String,
    pub placement: ContextPlacement,
    pub sort_order: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComposedPrompt {
    pub raw_prompt: String,
    pub final_prompt: String,
    pub contexts: Vec<PromptContextSnapshot>,
}

pub fn compose_prompt(raw_prompt: &str, contexts: &[PromptContext]) -> ComposedPrompt {
    let mut active: Vec<_> = contexts
        .iter()
        .filter(|context| context.enabled && !context.content.trim().is_empty())
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
            sort_order: context.sort_order,
        })
        .collect::<Vec<_>>();

    let mut fragments = active
        .iter()
        .filter(|context| context.placement == ContextPlacement::Prepend)
        .map(|context| context.content.trim().to_owned())
        .collect::<Vec<_>>();
    if !raw_prompt.trim().is_empty() {
        fragments.push(raw_prompt.trim().to_owned());
    }
    fragments.extend(
        active
            .iter()
            .filter(|context| context.placement == ContextPlacement::Append)
            .map(|context| context.content.trim().to_owned()),
    );

    ComposedPrompt {
        raw_prompt: raw_prompt.to_owned(),
        final_prompt: fragments.join("\n\n"),
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
}
