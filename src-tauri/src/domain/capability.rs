use std::collections::BTreeMap;

use serde_json::Value;

use super::{DomainError, ModelCapability};

/// Applies JSON Merge Patch-like semantics. Object keys are recursively merged,
/// arrays and scalar values replace the previous value, and `null` removes a key.
pub fn deep_merge_json(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(target), Value::Object(patch)) => {
            for (key, value) in patch {
                if value.is_null() {
                    target.remove(key);
                } else if let Some(existing) = target.get_mut(key) {
                    deep_merge_json(existing, value);
                } else {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
        (target, patch) => *target = patch.clone(),
    }
}

/// Merges generic protocol defaults, the versioned registry entry, then a user
/// override. Provider/model identity always comes from the generic base.
pub fn merge_capability_layers(
    generic: &ModelCapability,
    registry_patch: Option<&Value>,
    user_patch: Option<&Value>,
) -> Result<ModelCapability, DomainError> {
    let mut value = serde_json::to_value(generic)
        .map_err(|error| DomainError::Serialization(error.to_string()))?;
    if let Some(patch) = registry_patch {
        deep_merge_json(&mut value, patch);
    }
    if let Some(patch) = user_patch {
        deep_merge_json(&mut value, patch);
    }
    let mut capability: ModelCapability = serde_json::from_value(value)
        .map_err(|error| DomainError::InvalidCapability(error.to_string()))?;
    capability.provider_kind = generic.provider_kind.clone();
    capability.model_id.clone_from(&generic.model_id);
    if capability.max_outputs == 0 {
        return Err(DomainError::InvalidCapability(
            "maxOutputs must be greater than zero".to_owned(),
        ));
    }
    Ok(capability)
}

/// Parameter precedence is model defaults < project defaults < preset < form.
/// Nested objects are merged while arrays and scalars are replaced.
pub fn merge_parameter_layers(
    model_defaults: &BTreeMap<String, Value>,
    project_defaults: &BTreeMap<String, Value>,
    preset: &BTreeMap<String, Value>,
    form: &BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    let mut merged = Value::Object(model_defaults.clone().into_iter().collect());
    for layer in [project_defaults, preset, form] {
        let patch = Value::Object(layer.clone().into_iter().collect());
        deep_merge_json(&mut merged, &patch);
    }
    match merged {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::domain::ProviderKind;

    #[test]
    fn capability_layers_preserve_identity_and_apply_override_priority() {
        let base = ModelCapability::generic(ProviderKind::OpenAi, "gpt-image-1");
        let registry = json!({"maxOutputs": 4, "supportsMask": true});
        let user = json!({"maxOutputs": 2, "modelId": "not-allowed"});

        let merged = merge_capability_layers(&base, Some(&registry), Some(&user)).unwrap();

        assert_eq!(merged.max_outputs, 2);
        assert!(merged.supports_mask);
        assert_eq!(merged.model_id, "gpt-image-1");
    }

    #[test]
    fn parameter_layers_deep_merge_in_precedence_order() {
        let model = BTreeMap::from([("thinking".into(), json!({"level": "minimal", "budget": 4}))]);
        let project = BTreeMap::from([("thinking".into(), json!({"budget": 8}))]);
        let preset = BTreeMap::new();
        let form = BTreeMap::from([("thinking".into(), json!({"level": "high"}))]);

        let result = merge_parameter_layers(&model, &project, &preset, &form);

        assert_eq!(result["thinking"], json!({"level": "high", "budget": 8}));
    }
}
