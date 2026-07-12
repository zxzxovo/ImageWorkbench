use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

const REDACTED: &str = "[REDACTED]";

pub fn redact_headers(headers: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    redact_headers_with_names(headers, std::iter::empty::<&str>())
}

pub fn redact_headers_with_names<'a>(
    headers: &BTreeMap<String, String>,
    extra_sensitive_names: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, String> {
    let extras = extra_sensitive_names
        .into_iter()
        .map(normalize_key)
        .collect::<BTreeSet<_>>();
    headers
        .iter()
        .map(|(name, value)| {
            let value = if is_sensitive_header(name) || extras.contains(&normalize_key(name)) {
                REDACTED.to_owned()
            } else {
                value.clone()
            };
            (name.clone(), value)
        })
        .collect()
}

pub fn redact_json(value: &Value) -> Value {
    redact_json_with_keys(value, std::iter::empty::<&str>())
}

pub fn redact_json_with_keys<'a>(
    value: &Value,
    extra_sensitive_keys: impl IntoIterator<Item = &'a str>,
) -> Value {
    let extras = extra_sensitive_keys
        .into_iter()
        .map(normalize_key)
        .collect::<BTreeSet<_>>();
    redact_value(value, &extras)
}

pub fn redact_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_owned();
    };
    let redacted_query = query
        .split('&')
        .map(|part| {
            let Some((name, value)) = part.split_once('=') else {
                return part.to_owned();
            };
            if is_sensitive_key(name, &BTreeSet::new()) {
                format!("{name}={REDACTED}")
            } else {
                format!("{name}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{redacted_query}")
}

fn redact_value(value: &Value, extras: &BTreeSet<String>) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let value = if is_sensitive_key(key, extras) {
                        Value::String(REDACTED.to_owned())
                    } else {
                        redact_value(value, extras)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| redact_value(value, extras))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn is_sensitive_header(name: &str) -> bool {
    matches!(
        normalize_key(name).as_str(),
        "authorization"
            | "proxyauthorization"
            | "xapikey"
            | "apikey"
            | "xgoogapikey"
            | "cookie"
            | "setcookie"
    )
}

fn is_sensitive_key(name: &str, extras: &BTreeSet<String>) -> bool {
    let normalized = normalize_key(name);
    extras.contains(&normalized)
        || matches!(
            normalized.as_str(),
            "authorization"
                | "proxyauthorization"
                | "key"
                | "apikey"
                | "xapikey"
                | "xgoogapikey"
                | "accesstoken"
                | "refreshtoken"
                | "idtoken"
                | "password"
                | "passphrase"
                | "secret"
                | "clientsecret"
                | "privatekey"
                | "credential"
                | "credentials"
                | "cookie"
                | "setcookie"
        )
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn recursively_redacts_json_without_mutating_source() {
        let source = json!({
            "api_key": "top-secret",
            "nested": [{"accessToken": "token", "prompt": "draw"}],
            "custom": "hide-me"
        });

        let redacted = redact_json_with_keys(&source, ["custom"]);

        assert_eq!(redacted["api_key"], REDACTED);
        assert_eq!(redacted["nested"][0]["accessToken"], REDACTED);
        assert_eq!(redacted["nested"][0]["prompt"], "draw");
        assert_eq!(redacted["custom"], REDACTED);
        assert_eq!(source["api_key"], "top-secret");
    }

    #[test]
    fn redacts_headers_case_insensitively() {
        let headers = BTreeMap::from([
            ("Authorization".to_owned(), "Bearer token".to_owned()),
            ("X-Request-Id".to_owned(), "request".to_owned()),
        ]);

        let redacted = redact_headers(&headers);

        assert_eq!(redacted["Authorization"], REDACTED);
        assert_eq!(redacted["X-Request-Id"], "request");
    }

    #[test]
    fn redacts_credentials_in_query_strings() {
        assert_eq!(
            redact_url("https://example.test/models?key=visible&api_key=secret"),
            "https://example.test/models?key=[REDACTED]&api_key=[REDACTED]"
        );
    }

    #[test]
    fn redacts_custom_secret_headers() {
        let headers = BTreeMap::from([("X-Private-Token".to_owned(), "secret".to_owned())]);
        let redacted = redact_headers_with_names(&headers, ["x-private-token"]);
        assert_eq!(redacted["X-Private-Token"], REDACTED);
    }
}
