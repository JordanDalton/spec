use std::process::Command;

pub trait LlmProvider {
    fn complete(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>>;
}

pub struct AnthropicProvider {
    pub api_key: String,
    pub model: String,
}

pub struct OpenAiProvider {
    pub api_key: String,
    pub model: String,
}

pub struct OllamaProvider {
    pub host: String,
    pub model: String,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        AnthropicProvider { api_key, model }
    }

    #[allow(dead_code)]
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| "ANTHROPIC_API_KEY environment variable not set")?;
        Ok(AnthropicProvider {
            api_key,
            model: "claude-sonnet-4-6".to_string(),
        })
    }
}

impl LlmProvider for AnthropicProvider {
    fn complete(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        // Escape the prompt for JSON
        let escaped = json_escape(prompt);

        let body = format!(
            r#"{{"model":"{}","max_tokens":4096,"messages":[{{"role":"user","content":"{}"}}]}}"#,
            self.model, escaped
        );

        let output = Command::new("curl")
            .args([
                "-s",
                "-X",
                "POST",
                "https://api.anthropic.com/v1/messages",
                "-H",
                "Content-Type: application/json",
                "-H",
                &format!("x-api-key: {}", self.api_key),
                "-H",
                "anthropic-version: 2023-06-01",
                "-d",
                &body,
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("curl failed: {}", stderr).into());
        }

        let response_str = String::from_utf8_lossy(&output.stdout).to_string();

        // Parse JSON response
        let v: serde_json::Value = serde_json::from_str(&response_str)
            .map_err(|e| format!("Failed to parse API response: {}\nResponse: {}", e, response_str))?;

        // Extract text from response
        if let Some(error) = v.get("error") {
            return Err(format!("API error: {}", error).into());
        }

        let text = v["content"][0]["text"]
            .as_str()
            .ok_or("No text in response")?;

        Ok(text.to_string())
    }
}

impl LlmProvider for OpenAiProvider {
    fn complete(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        let escaped = json_escape(prompt);

        let body = format!(
            r#"{{"model":"{}","messages":[{{"role":"user","content":"{}"}}],"max_tokens":4096}}"#,
            self.model, escaped
        );

        let output = Command::new("curl")
            .args([
                "-s",
                "-X",
                "POST",
                "https://api.openai.com/v1/chat/completions",
                "-H",
                "Content-Type: application/json",
                "-H",
                &format!("Authorization: Bearer {}", self.api_key),
                "-d",
                &body,
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("curl failed: {}", stderr).into());
        }

        let response_str = String::from_utf8_lossy(&output.stdout).to_string();
        let v: serde_json::Value = serde_json::from_str(&response_str)?;

        if let Some(error) = v.get("error") {
            return Err(format!("API error: {}", error).into());
        }

        let text = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("No content in response")?;

        Ok(text.to_string())
    }
}

impl LlmProvider for OllamaProvider {
    fn complete(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        let escaped = json_escape(prompt);

        let body = format!(
            r#"{{"model":"{}","prompt":"{}","stream":false}}"#,
            self.model, escaped
        );

        let url = format!("{}/api/generate", self.host);

        let output = Command::new("curl")
            .args(["-s", "-X", "POST", &url, "-H", "Content-Type: application/json", "-d", &body])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("curl failed: {}", stderr).into());
        }

        let response_str = String::from_utf8_lossy(&output.stdout).to_string();
        let v: serde_json::Value = serde_json::from_str(&response_str)?;

        let text = v["response"]
            .as_str()
            .ok_or("No response field in Ollama output")?;

        Ok(text.to_string())
    }
}

/// Build an LLM provider from the project config.
/// SPEC_PROVIDER and SPEC_API_KEY always take precedence over config file
/// and provider-specific env vars, so Spec's keys never collide with the app's.
pub fn build_provider(
    config: &crate::spec::Config,
) -> Result<Box<dyn LlmProvider>, Box<dyn std::error::Error>> {
    let provider = std::env::var("SPEC_PROVIDER")
        .unwrap_or_else(|_| config.llm_provider.clone())
        .to_lowercase();

    let model = std::env::var("SPEC_MODEL")
        .unwrap_or_else(|_| config.model.clone());

    let spec_api_key = std::env::var("SPEC_API_KEY").ok();

    match provider.as_str() {
        "anthropic" => {
            let api_key = spec_api_key
                .or_else(|| std::env::var(&config.anthropic_api_key_env).ok())
                .ok_or("No API key found. Set SPEC_API_KEY or ANTHROPIC_API_KEY.")?;
            Ok(Box::new(AnthropicProvider::new(api_key, model.clone())))
        }
        "openai" => {
            let api_key = spec_api_key
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .ok_or("No API key found. Set SPEC_API_KEY or OPENAI_API_KEY.")?;
            Ok(Box::new(OpenAiProvider {
                api_key,
                model: model.clone(),
            }))
        }
        "ollama" => {
            let host = std::env::var("OLLAMA_HOST")
                .unwrap_or_else(|_| "http://localhost:11434".to_string());
            Ok(Box::new(OllamaProvider {
                host,
                model: model.clone(),
            }))
        }
        other => Err(format!(
            "Unknown provider '{}'. Set SPEC_PROVIDER to: anthropic, openai, ollama",
            other
        )
        .into()),
    }
}

/// Escape a string for embedding in a JSON string value
fn json_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => result.push(c),
        }
    }
    result
}
