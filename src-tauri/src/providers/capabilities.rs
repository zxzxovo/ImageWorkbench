use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::ProviderError;
use super::types::{Operation, ProviderKind};

const OPENAI_CATALOG: &str = include_str!("../../resources/capabilities/openai-2026-07-12.json");
const XAI_CATALOG: &str = include_str!("../../resources/capabilities/xai-2026-07-12.json");
const GEMINI_CATALOG: &str = include_str!("../../resources/capabilities/gemini-2026-07-12.json");
const COMPATIBLE_CATALOG: &str =
    include_str!("../../resources/capabilities/openai-compatible-2026-07-12.json");

static CATALOGS: OnceLock<Result<Vec<CapabilityCatalog>, ProviderError>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CapabilityCatalog {
    pub schema_version: u32,
    pub catalog_version: String,
    pub provider: ProviderKind,
    #[serde(default)]
    pub documentation: Vec<String>,
    pub models: Vec<ModelCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ModelCapability {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub api_surfaces: Vec<String>,
    #[serde(default)]
    pub operations: Vec<Operation>,
    pub prompt_max_chars: Option<u32>,
    pub max_input_images: Option<u8>,
    pub output_count: Option<NumericLimit>,
    #[serde(default)]
    pub sizes: Vec<String>,
    pub custom_size: Option<CustomSizeRule>,
    #[serde(default)]
    pub aspect_ratios: Vec<String>,
    #[serde(default)]
    pub resolutions: Vec<String>,
    #[serde(default)]
    pub qualities: Vec<String>,
    #[serde(default)]
    pub formats: Vec<String>,
    #[serde(default)]
    pub response_formats: Vec<String>,
    #[serde(default)]
    pub backgrounds: Vec<String>,
    #[serde(default)]
    pub features: FeatureFlags,
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct NumericLimit {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CustomSizeRule {
    pub multiple_of: u32,
    pub min_aspect_ratio: f64,
    pub max_aspect_ratio: f64,
    pub max_edge: u32,
    pub max_pixels: u64,
    pub experimental_above_pixels: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct FeatureFlags {
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub batch: bool,
    #[serde(default)]
    pub mask: bool,
    #[serde(default)]
    pub multiple_images: bool,
    #[serde(default)]
    pub file_inputs: bool,
    #[serde(default)]
    pub file_outputs: bool,
    #[serde(default)]
    pub interleaved_text: bool,
    #[serde(default)]
    pub thinking: bool,
    #[serde(default)]
    pub google_search: bool,
    #[serde(default)]
    pub image_search: bool,
    #[serde(default)]
    pub video_input: bool,
}

pub fn catalogs() -> Result<&'static [CapabilityCatalog], ProviderError> {
    CATALOGS
        .get_or_init(|| {
            [
                OPENAI_CATALOG,
                XAI_CATALOG,
                GEMINI_CATALOG,
                COMPATIBLE_CATALOG,
            ]
            .into_iter()
            .map(|json| serde_json::from_str(json).map_err(ProviderError::from))
            .collect()
        })
        .as_deref()
        .map_err(Clone::clone)
}

pub fn catalog(provider: ProviderKind) -> Result<&'static CapabilityCatalog, ProviderError> {
    catalogs()?
        .iter()
        .find(|catalog| catalog.provider == provider)
        .ok_or_else(|| ProviderError::parse("capability catalog is missing", None))
}

pub fn find_model(
    provider: ProviderKind,
    model: &str,
) -> Result<Option<&'static ModelCapability>, ProviderError> {
    let catalog = catalog(provider)?;
    let specific = catalog
        .models
        .iter()
        .find(|capability| capability.id != "*" && capability.id == model)
        .or_else(|| {
            catalog
                .models
                .iter()
                .filter(|capability| capability.id != "*")
                .filter_map(|capability| {
                    capability
                        .aliases
                        .iter()
                        .filter(|alias| alias.as_str() != "*")
                        .filter_map(|alias| {
                            if alias == model {
                                Some(alias.len() + 1)
                            } else {
                                alias
                                    .strip_suffix('*')
                                    .filter(|prefix| model.starts_with(prefix))
                                    .map(str::len)
                            }
                        })
                        .max()
                        .map(|specificity| (specificity, capability))
                })
                .max_by_key(|(specificity, _)| *specificity)
                .map(|(_, capability)| capability)
        });
    Ok(specific.or_else(|| {
        catalog.models.iter().find(|capability| {
            capability.id == "*" || capability.aliases.iter().any(|alias| alias == "*")
        })
    }))
}

pub fn resolve_model(
    provider: ProviderKind,
    model: &str,
    user_patch: Option<&Value>,
) -> Result<Option<ModelCapability>, ProviderError> {
    if model.trim().is_empty() {
        return Err(ProviderError::validation("model is required"));
    }
    if user_patch.is_some_and(|patch| !patch.is_object()) {
        return Err(ProviderError::validation(
            "a capability override must be a JSON object",
        ));
    }
    let catalog_capability = find_model(provider, model)?;
    if catalog_capability.is_none() && user_patch.is_none() {
        return Ok(None);
    }
    let generic_base = catalog_capability.is_none_or(|capability| capability.id == "*");
    let base = catalog_capability
        .cloned()
        .unwrap_or_else(|| generic_capability(provider, model));
    let mut value = serde_json::to_value(base)?;
    if let Some(patch) = user_patch {
        deep_merge_json(&mut value, patch);
    }
    let mut resolved: ModelCapability = serde_json::from_value(value).map_err(|error| {
        ProviderError::validation(format!("invalid capability override for {model}: {error}"))
    })?;
    resolved.id = model.to_owned();
    if generic_base {
        resolved.aliases.clear();
        if resolved.display_name.trim().is_empty()
            || catalog_capability
                .is_some_and(|capability| resolved.display_name == capability.display_name)
        {
            resolved.display_name = model.to_owned();
        }
    }
    validate_resolved_capability(&resolved)?;
    Ok(Some(resolved))
}

fn generic_capability(provider: ProviderKind, model: &str) -> ModelCapability {
    let api_surfaces = match provider {
        ProviderKind::OpenAi => vec!["images".to_owned(), "responses".to_owned()],
        ProviderKind::OpenAiCompatible | ProviderKind::Xai => vec!["images".to_owned()],
        ProviderKind::Gemini => vec!["interactions".to_owned(), "generate_content".to_owned()],
    };
    ModelCapability {
        id: model.to_owned(),
        display_name: model.to_owned(),
        aliases: Vec::new(),
        api_surfaces,
        operations: vec![Operation::Generate],
        prompt_max_chars: None,
        max_input_images: None,
        output_count: Some(NumericLimit { min: 1, max: 1 }),
        sizes: Vec::new(),
        custom_size: None,
        aspect_ratios: Vec::new(),
        resolutions: Vec::new(),
        qualities: Vec::new(),
        formats: Vec::new(),
        response_formats: Vec::new(),
        backgrounds: Vec::new(),
        features: FeatureFlags::default(),
        defaults: BTreeMap::new(),
        notes: vec!["User capability override applied to an unknown model.".to_owned()],
    }
}

fn deep_merge_json(target: &mut Value, patch: &Value) {
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

fn validate_resolved_capability(capability: &ModelCapability) -> Result<(), ProviderError> {
    if let Some(limit) = &capability.output_count {
        if limit.min == 0 || limit.max < limit.min {
            return Err(ProviderError::validation(
                "capability output_count must have 1 <= min <= max",
            ));
        }
    }
    if let Some(rule) = &capability.custom_size {
        if rule.multiple_of == 0
            || rule.max_edge == 0
            || rule.max_pixels == 0
            || rule.min_aspect_ratio <= 0.0
            || rule.max_aspect_ratio < rule.min_aspect_ratio
        {
            return Err(ProviderError::validation(
                "capability custom_size contains invalid limits",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_embedded_catalogs_parse() {
        let catalogs = catalogs().unwrap();
        assert_eq!(catalogs.len(), 4);
        assert!(catalogs.iter().all(|catalog| !catalog.models.is_empty()));
    }

    #[test]
    fn resolves_versioned_openai_alias() {
        let model = find_model(ProviderKind::OpenAi, "gpt-image-2-2026-04-21")
            .unwrap()
            .unwrap();
        assert_eq!(model.id, "gpt-image-2");
        assert!(model.custom_size.is_some());
    }

    #[test]
    fn resolves_the_most_specific_alias() {
        let model = find_model(ProviderKind::OpenAi, "gpt-image-1-mini-2026-01-01")
            .unwrap()
            .unwrap();
        assert_eq!(model.id, "gpt-image-1-mini");
        let model = find_model(ProviderKind::Xai, "grok-imagine-image-quality-20260403")
            .unwrap()
            .unwrap();
        assert_eq!(model.id, "grok-imagine-image-quality");
    }

    #[test]
    fn known_model_override_wins_after_registry_resolution() {
        let resolved = resolve_model(
            ProviderKind::OpenAi,
            "gpt-image-1",
            Some(&serde_json::json!({
                "output_count": { "min": 1, "max": 2 },
                "features": { "streaming": false }
            })),
        )
        .unwrap()
        .unwrap();

        assert_eq!(resolved.id, "gpt-image-1");
        assert_eq!(resolved.output_count.unwrap().max, 2);
        assert!(!resolved.features.streaming);
        assert!(!resolved.sizes.is_empty());
    }

    #[test]
    fn unknown_model_resolves_only_when_an_override_exists() {
        assert!(
            resolve_model(ProviderKind::Xai, "future-image", None)
                .unwrap()
                .is_none()
        );
        let resolved = resolve_model(
            ProviderKind::Xai,
            "future-image",
            Some(&serde_json::json!({
                "operations": ["generate", "edit"],
                "max_input_images": 2,
                "output_count": { "min": 1, "max": 4 },
                "aspect_ratios": ["1:1"],
                "resolutions": ["1k"],
                "features": { "multiple_images": true }
            })),
        )
        .unwrap()
        .unwrap();

        assert_eq!(resolved.id, "future-image");
        assert_eq!(resolved.max_input_images, Some(2));
        assert_eq!(resolved.output_count.unwrap().max, 4);
    }

    #[test]
    fn contains_all_required_model_families() {
        for (provider, models) in [
            (
                ProviderKind::OpenAi,
                &[
                    "gpt-image-2",
                    "gpt-image-1.5",
                    "gpt-image-1",
                    "gpt-image-1-mini",
                    "dall-e-2",
                    "dall-e-3",
                ][..],
            ),
            (
                ProviderKind::Xai,
                &["grok-imagine-image", "grok-imagine-image-quality"][..],
            ),
            (
                ProviderKind::Gemini,
                &[
                    "gemini-3.1-flash-image",
                    "gemini-3.1-flash-lite-image",
                    "gemini-3-pro-image",
                    "gemini-2.5-flash-image",
                ][..],
            ),
        ] {
            for model in models {
                assert!(find_model(provider, model).unwrap().is_some(), "{model}");
            }
        }
    }
}
