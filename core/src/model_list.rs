use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::providers::{self, ApiStyle, Endpoint};
use crate::state::{Client, Provider};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MODELS: usize = 500;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelList {
    pub models: Vec<ModelChoice>,
    /// `provider` when the list came from the provider, `known` when it is the
    /// fallback for a failed or unsupported read.
    pub source: String,
    pub fallback: bool,
}

impl ModelList {
    pub fn known(vendor: &str) -> Self {
        Self {
            models: providers::known_models(vendor)
                .into_iter()
                .map(|(id, label)| ModelChoice { id, label })
                .collect(),
            source: "known".to_owned(),
            fallback: true,
        }
    }
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap_or_default()
}

fn models_url(endpoint: &Endpoint) -> String {
    let base = endpoint.base.trim_end_matches('/');
    match endpoint.style {
        ApiStyle::OpenAi if base.ends_with("/v1") => format!("{base}/models"),
        ApiStyle::OpenAi => format!("{base}/models"),
        ApiStyle::Anthropic if base.ends_with("/v1") => format!("{base}/models"),
        ApiStyle::Anthropic => format!("{base}/v1/models"),
    }
}

/// Reads a provider's model list in real time. The key is used only in the
/// request and never leaves this function.
pub async fn fetch(
    provider: &Provider,
    client: Client,
    key: Option<&str>,
) -> Result<Vec<ModelChoice>, ()> {
    let Some(endpoint) = providers::endpoint(provider, client) else {
        return Err(());
    };
    let url = models_url(&endpoint);
    let mut request = http_client().get(url);
    match endpoint.style {
        ApiStyle::OpenAi => {
            if let Some(key) = key {
                request = request.bearer_auth(key);
            }
        }
        ApiStyle::Anthropic => {
            if let Some(key) = key {
                request = request
                    .header("x-api-key", key)
                    .header("anthropic-version", "2023-06-01");
            }
        }
    }
    let response = request.send().await.map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    let payload = response.json::<serde_json::Value>().await.map_err(|_| ())?;
    let models = parse(&payload);
    if models.is_empty() {
        return Err(());
    }
    Ok(models)
}

fn parse(payload: &serde_json::Value) -> Vec<ModelChoice> {
    let mut models = Vec::new();
    for entry in payload
        .get("data")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            payload
                .get("models")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten(),
        )
    {
        let id = entry
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                entry
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| {
                entry
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            });
        let Some(id) = id else {
            continue;
        };
        let id = id.trim().to_owned();
        if !valid_id(&id) || models.iter().any(|model: &ModelChoice| model.id == id) {
            continue;
        }
        let label = entry
            .get("display_name")
            .or_else(|| entry.get("displayName"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .unwrap_or(&id)
            .to_owned();
        models.push(ModelChoice { id, label });
        if models.len() >= MAX_MODELS {
            break;
        }
    }
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= 200
        && !id
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_openai_anthropic_and_local_shapes() {
        let openai = parse(&json!({
            "data": [{ "id": "gpt-5.2" }, { "id": "gpt-5.1", "display_name": "GPT 5.1" }]
        }));
        assert_eq!(openai.len(), 2);
        assert_eq!(
            openai
                .iter()
                .find(|model| model.id == "gpt-5.1")
                .expect("gpt-5.1")
                .label,
            "GPT 5.1"
        );

        let anthropic = parse(&json!({
            "data": [{ "id": "claude-sonnet-4-5", "display_name": "Claude Sonnet 4.5" }]
        }));
        assert_eq!(anthropic[0].id, "claude-sonnet-4-5");

        let local = parse(&json!({ "models": [{ "name": "llama3.2" }] }));
        assert_eq!(local[0].id, "llama3.2");
    }

    #[test]
    fn drops_blank_duplicate_and_invalid_ids() {
        let models = parse(&json!({
            "data": ["", "gpt-5.2", "gpt-5.2", "bad id", 7]
        }));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-5.2");
    }

    #[test]
    fn the_fallback_list_is_flagged() {
        let list = ModelList::known("deepseek");
        assert!(list.fallback);
        assert_eq!(list.source, "known");
        assert!(list.models.iter().any(|model| model.id == "deepseek-chat"));
    }
}
