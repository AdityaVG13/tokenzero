//! Provider cache-meter normalization and per-session cache economics.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokenzero_core::count_tokens;

pub const ANTHROPIC_CACHE_DIAGNOSIS_BETA: &str = "cache-diagnosis-2026-04-07";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheProvider { Anthropic, OpenAi, Gemini }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
}

impl ProviderUsage {
    pub fn total_input_tokens(self) -> u64 {
        self.input_tokens.saturating_add(self.cache_read_input_tokens).saturating_add(self.cache_creation_input_tokens)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CacheMeterError {
    #[error("missing provider usage field: {0}")]
    MissingField(&'static str),
    #[error("provider usage field is not an unsigned integer: {0}")]
    InvalidField(&'static str),
}

fn object_at<'a>(value: &'a Value, key: &str) -> &'a Value { value.get(key).unwrap_or(value) }
fn required_u64(value: &Value, key: &'static str) -> Result<u64, CacheMeterError> {
    value.get(key).ok_or(CacheMeterError::MissingField(key))?.as_u64().ok_or(CacheMeterError::InvalidField(key))
}
fn optional_u64(value: &Value, key: &'static str) -> Result<u64, CacheMeterError> {
    value.get(key).map_or(Ok(0), |field| field.as_u64().ok_or(CacheMeterError::InvalidField(key)))
}

pub fn parse_provider_usage(provider: CacheProvider, value: &Value) -> Result<ProviderUsage, CacheMeterError> {
    match provider {
        CacheProvider::Anthropic => {
            let usage = object_at(value, "usage");
            Ok(ProviderUsage { input_tokens: required_u64(usage, "input_tokens")?, output_tokens: optional_u64(usage, "output_tokens")?, cache_read_input_tokens: optional_u64(usage, "cache_read_input_tokens")?, cache_creation_input_tokens: optional_u64(usage, "cache_creation_input_tokens")? })
        }
        CacheProvider::OpenAi => {
            let usage = object_at(value, "usage");
            let prompt = required_u64(usage, "prompt_tokens")?;
            let cached = usage.get("prompt_tokens_details").and_then(|details| details.get("cached_tokens")).map_or(Ok(0), |field| field.as_u64().ok_or(CacheMeterError::InvalidField("prompt_tokens_details.cached_tokens")))?;
            Ok(ProviderUsage { input_tokens: prompt.saturating_sub(cached), output_tokens: optional_u64(usage, "completion_tokens")?, cache_read_input_tokens: cached, cache_creation_input_tokens: 0 })
        }
        CacheProvider::Gemini => {
            let usage = object_at(value, "usageMetadata");
            let prompt = required_u64(usage, "promptTokenCount")?;
            let cached = optional_u64(usage, "cachedContentTokenCount")?;
            Ok(ProviderUsage { input_tokens: prompt.saturating_sub(cached), output_tokens: optional_u64(usage, "candidatesTokenCount")?, cache_read_input_tokens: cached, cache_creation_input_tokens: 0 })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CachePricing { pub input_per_million: f64, pub cache_read_per_million: f64, pub cache_creation_per_million: f64, pub output_per_million: f64 }
impl CachePricing {
    pub fn realized_dollars(self, usage: ProviderUsage) -> f64 {
        (usage.input_tokens as f64 * self.input_per_million + usage.cache_read_input_tokens as f64 * self.cache_read_per_million + usage.cache_creation_input_tokens as f64 * self.cache_creation_per_million + usage.output_tokens as f64 * self.output_per_million) / 1_000_000.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicCacheDiagnosisRequest { pub previous_message_id: String }
impl AnthropicCacheDiagnosisRequest {
    pub fn headers(&self) -> [(&'static str, &'static str); 1] { [("anthropic-beta", ANTHROPIC_CACHE_DIAGNOSIS_BETA)] }
    pub fn body(&self) -> Value { json!({ "previous_message_id": self.previous_message_id }) }
}
pub fn cache_miss_attribution(value: &Value) -> Option<String> {
    let diagnosis = value.get("cache_diagnosis").or_else(|| value.get("diagnosis")).unwrap_or(value);
    ["cache_miss_reason", "miss_reason", "reason"].into_iter().find_map(|key| diagnosis.get(key).and_then(Value::as_str).map(str::to_owned))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheObservation { pub provider: CacheProvider, pub request_tokens: u64, pub stable_prefix_tokens: u64, pub churn_depth_tokens: u64, pub usage: ProviderUsage, pub realized_dollars: f64, #[serde(skip_serializing_if = "Option::is_none")] pub miss_attribution: Option<String> }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheSessionReport { pub requests: u64, pub prefix_stability_ratio: f64, pub average_churn_depth_tokens: f64, pub hit_rate: f64, pub realized_dollars_per_request: f64, pub exact_miss_attributions: Vec<String> }
#[derive(Debug, Default)]
pub struct CacheMeter { previous_request: Option<String>, observations: Vec<CacheObservation> }

impl CacheMeter {
    pub fn observe(&mut self, provider: CacheProvider, request: &str, usage_value: &Value, pricing: CachePricing, diagnosis: Option<&Value>) -> Result<&CacheObservation, CacheMeterError> {
        let usage = parse_provider_usage(provider, usage_value)?;
        let request_tokens = count_tokens(request) as u64;
        let stable_prefix_tokens = self.previous_request.as_deref().map_or(0, |previous| count_tokens(common_prefix(previous, request)) as u64);
        self.observations.push(CacheObservation { provider, request_tokens, stable_prefix_tokens, churn_depth_tokens: stable_prefix_tokens, usage, realized_dollars: pricing.realized_dollars(usage), miss_attribution: diagnosis.and_then(cache_miss_attribution) });
        self.previous_request = Some(request.to_owned());
        Ok(self.observations.last().expect("observation was just pushed"))
    }
    pub fn observations(&self) -> &[CacheObservation] { &self.observations }
    pub fn report(&self) -> CacheSessionReport {
        let requests = self.observations.len() as u64;
        let stable = self.observations.iter().map(|item| item.stable_prefix_tokens).sum::<u64>();
        let request_mass = self.observations.iter().map(|item| item.request_tokens).sum::<u64>();
        let cached = self.observations.iter().map(|item| item.usage.cache_read_input_tokens).sum::<u64>();
        let input = self.observations.iter().map(|item| item.usage.total_input_tokens()).sum::<u64>();
        let churn = self.observations.iter().skip(1).map(|item| item.churn_depth_tokens).sum::<u64>();
        let transitions = requests.saturating_sub(1);
        let dollars = self.observations.iter().map(|item| item.realized_dollars).sum::<f64>();
        CacheSessionReport { requests, prefix_stability_ratio: ratio(stable, request_mass), average_churn_depth_tokens: if transitions == 0 { 0.0 } else { churn as f64 / transitions as f64 }, hit_rate: ratio(cached, input), realized_dollars_per_request: if requests == 0 { 0.0 } else { dollars / requests as f64 }, exact_miss_attributions: self.observations.iter().filter_map(|item| item.miss_attribution.clone()).collect() }
    }
}
fn common_prefix<'a>(left: &'a str, right: &str) -> &'a str {
    let mut end = 0;
    for ((left_index, left_char), (_, right_char)) in left.char_indices().zip(right.char_indices()) { if left_char != right_char { break; } end = left_index + left_char.len_utf8(); }
    &left[..end]
}
fn ratio(numerator: u64, denominator: u64) -> f64 { if denominator == 0 { 0.0 } else { numerator as f64 / denominator as f64 } }
