use std::collections::BTreeMap;

use serde_json::{Map, Value};

pub type ConfigOverrides = BTreeMap<String, String>;

/// Reads a config value as JSON when possible, otherwise as plain text.
pub fn json_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned()))
}

/// Merges config keys into a JSON object. Dots in a key create nested objects.
pub fn apply_json_config(target: &mut Map<String, Value>, config: &ConfigOverrides) {
    for (key, value) in config {
        insert_path(target, key, json_value(value));
    }
}

fn insert_path(target: &mut Map<String, Value>, path: &str, value: Value) {
    let mut segments = path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .peekable();
    let Some(first) = segments.next() else {
        return;
    };

    let mut current = target;
    let mut segment = first;
    loop {
        let Some(next) = segments.next() else {
            current.insert(segment.to_owned(), value);
            return;
        };
        let entry = current
            .entry(segment.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        current = entry
            .as_object_mut()
            .expect("the entry was replaced with an object");
        segment = next;
    }
}

/// Builds Codex `-c key=value` arguments for the given config.
pub fn codex_arguments(config: &ConfigOverrides) -> Vec<String> {
    config
        .iter()
        .flat_map(|(key, value)| ["-c".to_owned(), format!("{key}={}", codex_value(value))])
        .collect()
}

/// Keeps a JSON value as written, and quotes plain text.
pub fn codex_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if serde_json::from_str::<Value>(trimmed).is_ok() {
        trimmed.to_owned()
    } else {
        Value::String(raw.to_owned()).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_keys_create_objects_and_keep_literal_json() {
        let mut config = ConfigOverrides::new();
        config.insert(
            "permissions.defaultMode".to_owned(),
            "acceptEdits".to_owned(),
        );
        config.insert("enabled".to_owned(), "true".to_owned());
        config.insert("limits".to_owned(), "{\"max\":3}".to_owned());

        let mut target = Map::new();
        apply_json_config(&mut target, &config);

        assert_eq!(target["permissions"]["defaultMode"], "acceptEdits");
        assert_eq!(target["enabled"], true);
        assert_eq!(target["limits"]["max"], 3);
    }

    #[test]
    fn a_value_replaces_a_plain_object_on_the_path() {
        let mut config = ConfigOverrides::new();
        config.insert("a.b".to_owned(), "value".to_owned());

        let mut target =
            serde_json::from_value::<Map<String, Value>>(json!({ "a": 1 })).expect("object");
        apply_json_config(&mut target, &config);

        assert_eq!(target["a"]["b"], "value");
    }

    #[test]
    fn codex_arguments_keep_json_and_quote_plain_text() {
        let mut config = ConfigOverrides::new();
        config.insert("model".to_owned(), "gpt-5.2".to_owned());
        config.insert("model_reasoning_effort".to_owned(), "high".to_owned());
        config.insert("features".to_owned(), "{\"web\":true}".to_owned());

        let arguments = codex_arguments(&config);

        assert_eq!(
            arguments,
            [
                "-c",
                "features={\"web\":true}",
                "-c",
                "model=\"gpt-5.2\"",
                "-c",
                "model_reasoning_effort=\"high\"",
            ]
        );
    }
}
