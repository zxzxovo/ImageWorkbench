use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    DomainError, GenerationRequest, ImageSize, InputAssetKind, ModelCapability, Operation,
    ParameterRule, ParameterValueKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub path: String,
    pub code: String,
    pub message: String,
    pub severity: ValidationSeverity,
}

impl ValidationIssue {
    fn error(path: impl Into<String>, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            code: code.into(),
            message: message.into(),
            severity: ValidationSeverity::Error,
        }
    }
}

pub fn validate_request(
    request: &GenerationRequest,
    capability: &ModelCapability,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if request.prompt.trim().is_empty() {
        issues.push(ValidationIssue::error(
            "prompt",
            "required",
            "A prompt is required.",
        ));
    }
    if capability
        .max_prompt_chars
        .is_some_and(|limit| request.prompt.chars().count() > limit as usize)
    {
        issues.push(ValidationIssue::error(
            "prompt",
            "prompt_too_long",
            format!(
                "The prompt exceeds the {} character limit.",
                capability.max_prompt_chars.unwrap_or_default()
            ),
        ));
    }
    if request.model_id != capability.model_id && !capability.aliases.contains(&request.model_id) {
        issues.push(ValidationIssue::error(
            "modelId",
            "model_mismatch",
            "The request model does not match this capability entry.",
        ));
    }
    if !capability.operations.contains(&request.operation) {
        issues.push(ValidationIssue::error(
            "operation",
            "unsupported_operation",
            "The selected model does not support this operation.",
        ));
    }
    if !capability.execution_modes.contains(&request.execution_mode) {
        issues.push(ValidationIssue::error(
            "executionMode",
            "unsupported_execution_mode",
            "The selected model does not support this execution mode.",
        ));
    }
    if request.output.count == 0 || request.output.count > capability.max_outputs {
        issues.push(ValidationIssue::error(
            "output.count",
            "output_count_out_of_range",
            format!(
                "Output count must be between 1 and {}.",
                capability.max_outputs
            ),
        ));
    }

    let image_count = request
        .inputs
        .iter()
        .filter(|asset| asset.kind == InputAssetKind::Image)
        .count() as u16;
    let video_count = request
        .inputs
        .iter()
        .filter(|asset| asset.kind == InputAssetKind::Video)
        .count() as u16;
    if image_count > capability.max_reference_images {
        issues.push(ValidationIssue::error(
            "inputs",
            "too_many_reference_images",
            format!(
                "This model accepts at most {} reference image(s).",
                capability.max_reference_images
            ),
        ));
    }
    if video_count > capability.max_video_inputs {
        issues.push(ValidationIssue::error(
            "inputs",
            "too_many_video_inputs",
            format!(
                "This model accepts at most {} video input(s).",
                capability.max_video_inputs
            ),
        ));
    }
    for (index, asset) in request.inputs.iter().enumerate() {
        if !capability.input_kinds.contains(&asset.kind) {
            issues.push(ValidationIssue::error(
                format!("inputs[{index}]"),
                "unsupported_input_kind",
                "The selected model does not support this input kind.",
            ));
        }
        if let (Some(limit), Some(size)) = (capability.max_file_bytes, asset.size_bytes)
            && size > limit
        {
            issues.push(ValidationIssue::error(
                format!("inputs[{index}].sizeBytes"),
                "file_too_large",
                format!("Input exceeds the {limit} byte file limit."),
            ));
        }
        if matches!(asset.source, super::InputSource::Base64 { .. })
            && let (Some(limit), Some(size)) = (capability.max_inline_bytes, asset.size_bytes)
            && size > limit
        {
            issues.push(ValidationIssue::error(
                format!("inputs[{index}].sizeBytes"),
                "inline_input_too_large",
                format!("Inline input exceeds the {limit} byte limit."),
            ));
        }
        if matches!(asset.source, super::InputSource::ProviderFile { .. })
            && !capability.supports_provider_files
        {
            issues.push(ValidationIssue::error(
                format!("inputs[{index}].source"),
                "provider_file_not_supported",
                "The selected model does not accept provider file IDs.",
            ));
        }
    }

    match request.operation {
        Operation::Edit if image_count == 0 => issues.push(ValidationIssue::error(
            "inputs",
            "edit_requires_image",
            "Image editing requires at least one image input.",
        )),
        Operation::Variation if image_count != 1 => issues.push(ValidationIssue::error(
            "inputs",
            "variation_requires_one_image",
            "Image variation requires exactly one image input.",
        )),
        Operation::VideoReferenceToImage if video_count == 0 => {
            issues.push(ValidationIssue::error(
                "inputs",
                "video_reference_required",
                "Video-to-image requires a video input.",
            ))
        }
        _ => {}
    }

    if let Some(mask) = &request.mask {
        if !capability.supports_mask {
            issues.push(ValidationIssue::error(
                "mask",
                "mask_not_supported",
                "The selected model does not support masks.",
            ));
        }
        if mask.kind != InputAssetKind::Mask {
            issues.push(ValidationIssue::error(
                "mask.kind",
                "invalid_mask_kind",
                "The mask asset must have kind `mask`.",
            ));
        }
    }

    validate_output(request, capability, &mut issues);
    validate_parameters(request, capability, &mut issues);
    issues
}

pub fn validate_request_or_error(
    request: &GenerationRequest,
    capability: &ModelCapability,
) -> Result<(), DomainError> {
    let issues = validate_request(request, capability);
    if issues
        .iter()
        .any(|issue| issue.severity == ValidationSeverity::Error)
    {
        Err(DomainError::Validation(issues))
    } else {
        Ok(())
    }
}

fn validate_output(
    request: &GenerationRequest,
    capability: &ModelCapability,
    issues: &mut Vec<ValidationIssue>,
) {
    match &request.output.size {
        ImageSize::Auto if !capability.sizes.supports_auto => issues.push(ValidationIssue::error(
            "output.size",
            "auto_size_not_supported",
            "Automatic sizing is not supported by this model.",
        )),
        ImageSize::Preset { value } if !capability.sizes.presets.contains(value) => {
            issues.push(ValidationIssue::error(
                "output.size",
                "unsupported_size",
                "The selected size is not supported by this model.",
            ));
        }
        ImageSize::Custom { width, height } => {
            if !capability.sizes.allow_custom {
                issues.push(ValidationIssue::error(
                    "output.size",
                    "custom_size_not_supported",
                    "Custom dimensions are not supported by this model.",
                ));
            }
            let out_of_range = *width == 0
                || *height == 0
                || capability.sizes.min_width.is_some_and(|min| *width < min)
                || capability.sizes.max_width.is_some_and(|max| *width > max)
                || capability.sizes.min_height.is_some_and(|min| *height < min)
                || capability.sizes.max_height.is_some_and(|max| *height > max)
                || capability
                    .sizes
                    .max_area
                    .is_some_and(|max| u64::from(*width) * u64::from(*height) > max)
                || capability
                    .sizes
                    .width_multiple_of
                    .is_some_and(|multiple| multiple == 0 || *width % multiple != 0)
                || capability
                    .sizes
                    .height_multiple_of
                    .is_some_and(|multiple| multiple == 0 || *height % multiple != 0)
                || capability
                    .sizes
                    .min_aspect_ratio
                    .is_some_and(|min| f64::from(*width) / f64::from(*height) < min)
                || capability
                    .sizes
                    .max_aspect_ratio
                    .is_some_and(|max| f64::from(*width) / f64::from(*height) > max);
            if out_of_range {
                issues.push(ValidationIssue::error(
                    "output.size",
                    "custom_size_out_of_range",
                    "Custom dimensions are outside this model's limits.",
                ));
            }
        }
        _ => {}
    }

    validate_optional_set(
        "output.aspectRatio",
        request.output.aspect_ratio.as_ref(),
        &capability.aspect_ratios,
        issues,
    );
    validate_optional_set(
        "output.resolution",
        request.output.resolution.as_ref(),
        &capability.resolutions,
        issues,
    );
    validate_optional_set(
        "output.quality",
        request.output.quality.as_ref(),
        &capability.qualities,
        issues,
    );
    validate_optional_set(
        "output.style",
        request.output.style.as_ref(),
        &capability.styles,
        issues,
    );
    validate_optional_set(
        "output.moderation",
        request.output.moderation.as_ref(),
        &capability.moderation_modes,
        issues,
    );
    if let Some(format) = request.output.format
        && !capability.formats.contains(&format)
    {
        issues.push(ValidationIssue::error(
            "output.format",
            "unsupported_format",
            "The selected format is not supported by this model.",
        ));
    }
    if let Some(background) = request.output.background
        && !capability.backgrounds.contains(&background)
    {
        issues.push(ValidationIssue::error(
            "output.background",
            "unsupported_background",
            "The selected background mode is not supported by this model.",
        ));
    }
    if let Some(response_format) = request.output.response_format
        && !capability.response_formats.contains(&response_format)
    {
        issues.push(ValidationIssue::error(
            "output.responseFormat",
            "unsupported_response_format",
            "The selected response format is not supported by this model.",
        ));
    }
    for modality in &request.output.modalities {
        if !capability.output_modalities.contains(modality) {
            issues.push(ValidationIssue::error(
                "output.modalities",
                "unsupported_modality",
                "The selected output modality is not supported by this model.",
            ));
        }
    }
    if request.output.remote_storage == Some(true) && !capability.supports_remote_storage {
        issues.push(ValidationIssue::error(
            "output.remoteStorage",
            "remote_storage_not_supported",
            "Provider-side file storage is not supported by this model.",
        ));
    }
    if request.output.ttl_seconds.is_some() && !capability.supports_ttl {
        issues.push(ValidationIssue::error(
            "output.ttlSeconds",
            "ttl_not_supported",
            "Provider-side TTL is not supported by this model.",
        ));
    }
    if let Some(ttl) = request.output.ttl_seconds
        && (capability.min_ttl_seconds.is_some_and(|min| ttl < min)
            || capability.max_ttl_seconds.is_some_and(|max| ttl > max))
    {
        issues.push(ValidationIssue::error(
            "output.ttlSeconds",
            "ttl_out_of_range",
            "The provider file TTL is outside the supported range.",
        ));
    }
}

fn validate_optional_set<T: Ord + std::fmt::Display>(
    path: &str,
    value: Option<&T>,
    allowed: &std::collections::BTreeSet<T>,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Some(value) = value
        && !allowed.contains(value)
    {
        issues.push(ValidationIssue::error(
            path,
            "unsupported_value",
            format!("`{value}` is not supported by this model."),
        ));
    }
}

fn validate_parameters(
    request: &GenerationRequest,
    capability: &ModelCapability,
    issues: &mut Vec<ValidationIssue>,
) {
    for (name, rule) in &capability.parameter_rules {
        match request.parameters.get(name) {
            Some(value) => validate_parameter(name, value, rule, issues),
            None if rule.required => issues.push(ValidationIssue::error(
                format!("parameters.{name}"),
                "required",
                "This provider parameter is required.",
            )),
            None => {}
        }
    }
    if !capability.allow_unknown_parameters {
        for name in request.parameters.keys() {
            if !capability.parameter_rules.contains_key(name) {
                issues.push(ValidationIssue::error(
                    format!("parameters.{name}"),
                    "unknown_parameter",
                    "This parameter is not declared for the selected model.",
                ));
            }
        }
    }
}

fn validate_parameter(
    name: &str,
    value: &Value,
    rule: &ParameterRule,
    issues: &mut Vec<ValidationIssue>,
) {
    let kind_matches = match rule.value_kind {
        ParameterValueKind::Any => true,
        ParameterValueKind::Boolean => value.is_boolean(),
        ParameterValueKind::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
        ParameterValueKind::Number => value.is_number(),
        ParameterValueKind::String => value.is_string(),
        ParameterValueKind::Array => value.is_array(),
        ParameterValueKind::Object => value.is_object(),
    };
    if !kind_matches {
        issues.push(ValidationIssue::error(
            format!("parameters.{name}"),
            "invalid_type",
            "The parameter has an invalid value type.",
        ));
        return;
    }
    if !rule.allowed_values.is_empty() && !rule.allowed_values.contains(value) {
        issues.push(ValidationIssue::error(
            format!("parameters.{name}"),
            "unsupported_value",
            "The parameter value is not supported by this model.",
        ));
    }
    if let Some(number) = value.as_f64()
        && (rule.min.is_some_and(|min| number < min) || rule.max.is_some_and(|max| number > max))
    {
        issues.push(ValidationIssue::error(
            format!("parameters.{name}"),
            "out_of_range",
            "The parameter value is outside the supported range.",
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::{ExecutionMode, ProviderKind};

    #[test]
    fn rejects_unsupported_operation_and_excess_outputs() {
        let capability = ModelCapability::generic(ProviderKind::Gemini, "image-model");
        let mut request = GenerationRequest::new("project", "provider", "image-model", "draw");
        request.operation = Operation::Edit;
        request.output.count = 2;

        let issues = validate_request(&request, &capability);

        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "unsupported_operation")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "output_count_out_of_range")
        );
    }

    #[test]
    fn accepts_a_request_with_declared_capabilities() {
        let mut capability = ModelCapability::generic(ProviderKind::OpenAi, "gpt-image-1");
        capability.operations = BTreeSet::from([Operation::Generate]);
        capability.execution_modes = BTreeSet::from([ExecutionMode::Realtime]);
        let request = GenerationRequest::new("project", "provider", "gpt-image-1", "draw");

        assert!(validate_request(&request, &capability).is_empty());
    }

    #[test]
    fn validates_custom_dimension_multiples_and_aspect_ratio() {
        let mut capability = ModelCapability::generic(ProviderKind::OpenAi, "gpt-image-2");
        capability.sizes.allow_custom = true;
        capability.sizes.width_multiple_of = Some(64);
        capability.sizes.height_multiple_of = Some(64);
        capability.sizes.min_aspect_ratio = Some(0.5);
        capability.sizes.max_aspect_ratio = Some(2.0);
        let mut request = GenerationRequest::new("project", "provider", "gpt-image-2", "draw");
        request.output.size = ImageSize::Custom {
            width: 1000,
            height: 256,
        };

        let issues = validate_request(&request, &capability);

        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "custom_size_out_of_range")
        );
    }
}
