use serde_json::Value;

use crate::security::redact_json;

const MAX_LOGGED_STRING_BYTES: usize = 16 * 1024;

pub(crate) fn sanitize_provider_json(value: &Value) -> Value {
    redact_json(&strip_large_payloads(value, None))
}

fn strip_large_payloads(value: &Value, key: Option<&str>) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), strip_large_payloads(value, Some(key))))
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| strip_large_payloads(value, key))
                .collect(),
        ),
        Value::String(text) if should_strip(key, text) => {
            Value::String(format!("[OMITTED {} byte payload]", text.len()))
        }
        _ => value.clone(),
    }
}

fn should_strip(key: Option<&str>, value: &str) -> bool {
    if value.len() > MAX_LOGGED_STRING_BYTES {
        return true;
    }
    let key = key
        .unwrap_or_default()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    (key.contains("base64") || key == "b64json" || key == "inlinedata" || key == "bytes")
        && value.len() > 256
        || value.starts_with("data:") && value.len() > 256
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn strips_base64_and_redacts_credentials() {
        let value = json!({
            "api_key": "secret",
            "data": format!("data:image/png;base64,{}", "a".repeat(500)),
            "prompt": "keep"
        });
        let sanitized = sanitize_provider_json(&value);

        assert_eq!(sanitized["api_key"], "[REDACTED]");
        assert!(sanitized["data"].as_str().unwrap().starts_with("[OMITTED"));
        assert_eq!(sanitized["prompt"], "keep");
    }
}
