//! Claude as a journaled **Oracle** — the Anthropic Messages API adapter for
//! the Pragmatic durable-execution runtime.
//!
//! ```no_run
//! use pragmatic::Runtime;
//! use pragmatic_anthropic::AnthropicOracle;
//!
//! let oracle = AnthropicOracle::from_env()          // reads ANTHROPIC_API_KEY
//!     .expect("set ANTHROPIC_API_KEY")
//!     .model("claude-sonnet-5")
//!     .max_tokens(1024)
//!     .system("You are a terse research assistant.");
//!
//! let mut rt = Runtime::on_dir("./journals", oracle).unwrap();
//! let report = rt.run("research-42", |ctx| {
//!     let plan = ctx.oracle("Plan a survey of durable execution.")?;
//!     ctx.oracle(format!("Execute step one of: {plan}"))
//! }).unwrap();
//! ```
//!
//! Every completion is journaled by the runtime; crashes resume without
//! re-calling (or re-paying for) the model, and audits replay with the model
//! never consulted.

use pragmatic::{Fault, Oracle, Value};
use serde_json::{json, Value as Json};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_MODEL: &str = "claude-sonnet-5";
const DEFAULT_MAX_TOKENS: u32 = 1024;
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// An [`Oracle`] backed by the Anthropic Messages API. Each `call` sends the
/// prompt as a single user message and returns the model's text completion.
pub struct AnthropicOracle {
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    temperature: Option<f64>,
    system: Option<String>,
    timeout: std::time::Duration,
}

impl AnthropicOracle {
    /// Build with an explicit API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        AnthropicOracle {
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: None,
            system: None,
            timeout: std::time::Duration::from_secs(120),
        }
    }

    /// Build from the `ANTHROPIC_API_KEY` environment variable.
    pub fn from_env() -> Result<Self, Fault> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| Fault::OracleErr("ANTHROPIC_API_KEY is not set".to_string()))?;
        Ok(Self::new(key))
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn temperature(mut self, t: f64) -> Self {
        self.temperature = Some(t);
        self
    }

    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Override the API endpoint (proxies, gateways, mock servers in tests).
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn timeout(mut self, d: std::time::Duration) -> Self {
        self.timeout = d;
        self
    }

    /// The request body for `prompt` (exposed for tests and debugging).
    pub fn build_request(&self, prompt: &Value) -> Json {
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": [{ "role": "user", "content": prompt.as_str() }],
        });
        if let Some(t) = self.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(s) = &self.system {
            body["system"] = json!(s);
        }
        body
    }

    /// Pull the concatenated text blocks out of a Messages API response.
    pub fn extract_text(response: &Json) -> Result<String, Fault> {
        let blocks = response["content"]
            .as_array()
            .ok_or_else(|| Fault::OracleErr("response has no content array".to_string()))?;
        let text: String = blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect();
        if text.is_empty() {
            return Err(Fault::OracleErr(format!(
                "response contained no text blocks (stop_reason: {})",
                response["stop_reason"].as_str().unwrap_or("unknown")
            )));
        }
        Ok(text)
    }
}

impl Oracle for AnthropicOracle {
    fn call(&self, prompt: &Value) -> Result<Value, Fault> {
        let body = self.build_request(prompt);
        let url = format!("{}/v1/messages", self.base_url);
        let response = ureq::post(&url)
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", ANTHROPIC_VERSION)
            .set("content-type", "application/json")
            .timeout(self.timeout)
            .send_string(&body.to_string());

        let response = match response {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let detail = r.into_string().unwrap_or_default();
                let snippet: String = detail.chars().take(300).collect();
                return Err(Fault::OracleErr(format!(
                    "anthropic api returned {code}: {snippet}"
                )));
            }
            Err(e) => return Err(Fault::OracleErr(format!("transport error: {e}"))),
        };

        let json: Json = response
            .into_string()
            .map_err(|e| Fault::OracleErr(format!("failed reading response body: {e}")))?
            .parse::<Json>()
            .map_err(|e| Fault::OracleErr(format!("response is not valid JSON: {e}")))?;

        Ok(Value::from(Self::extract_text(&json)?))
    }

    fn provenance(&self) -> String {
        match self.temperature {
            Some(t) => format!("anthropic:{}@temperature={t}", self.model),
            None => format!("anthropic:{}", self.model),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_body_shape() {
        let oracle = AnthropicOracle::new("test-key")
            .model("claude-sonnet-5")
            .max_tokens(512)
            .temperature(0.7)
            .system("be terse");
        let body = oracle.build_request(&Value::from("hello"));
        assert_eq!(body["model"], "claude-sonnet-5");
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["temperature"], 0.7);
        assert_eq!(body["system"], "be terse");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hello");
    }

    #[test]
    fn optional_fields_omitted_by_default() {
        let oracle = AnthropicOracle::new("k");
        let body = oracle.build_request(&Value::from("p"));
        assert!(body.get("temperature").is_none());
        assert!(body.get("system").is_none());
    }

    #[test]
    fn extracts_and_concatenates_text_blocks() {
        let response = json!({
            "content": [
                { "type": "text", "text": "Hello " },
                { "type": "tool_use", "id": "x", "name": "t", "input": {} },
                { "type": "text", "text": "world" }
            ],
            "stop_reason": "end_turn"
        });
        assert_eq!(
            AnthropicOracle::extract_text(&response).unwrap(),
            "Hello world"
        );
    }

    #[test]
    fn empty_content_is_an_oracle_error() {
        let response = json!({ "content": [], "stop_reason": "max_tokens" });
        assert!(matches!(
            AnthropicOracle::extract_text(&response),
            Err(Fault::OracleErr(_))
        ));
    }

    #[test]
    fn provenance_names_the_model() {
        let oracle = AnthropicOracle::new("k")
            .model("claude-opus-4-8")
            .temperature(0.0);
        assert_eq!(
            oracle.provenance(),
            "anthropic:claude-opus-4-8@temperature=0"
        );
    }
}
