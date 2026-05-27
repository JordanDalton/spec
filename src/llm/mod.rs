use std::process::{Command, Stdio};

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

/// Uses the local `claude` CLI (Claude Code) instead of a direct API key.
/// Runs within the user's existing Claude subscription — no ANTHROPIC_API_KEY required.
pub struct ClaudeCodeProvider {
    pub model: Option<String>,
}

/// Uses the local `codex` CLI (OpenAI Codex) instead of a direct API key.
/// Runs within the user's existing OpenAI subscription — no OPENAI_API_KEY required.
pub struct CodexProvider {
    pub model: Option<String>,
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

impl LlmProvider for ClaudeCodeProvider {
    fn complete(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        let mut args: Vec<String> = vec!["--print".to_string()];

        if let Some(ref model) = self.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }

        args.push(prompt.to_string());

        let output = Command::new("claude")
            .args(&args)
            .output()
            .map_err(|e| format!("Failed to run claude CLI: {}. Is Claude Code installed?", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("claude CLI failed: {}", stderr).into());
        }

        let response = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if response.is_empty() {
            return Err("claude CLI returned an empty response".into());
        }

        Ok(response)
    }
}

impl LlmProvider for CodexProvider {
    fn complete(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        // Use a temp file to capture only the final agent message cleanly.
        let tmp_path = std::env::temp_dir().join(format!("spec_codex_{}.txt", std::process::id()));

        // --dangerously-bypass-approvals-and-sandbox is intentional here:
        // spec sends pure text inference prompts — it never asks Codex to execute shell commands.
        // When running inside a Codex session the parent sandbox already provides isolation,
        // and nesting a second OS-level sandbox causes EPERM on macOS/Linux.
        let mut args: Vec<String> = vec![
            "exec".to_string(),
            "--dangerously-bypass-approvals-and-sandbox".to_string(),
            "--skip-git-repo-check".to_string(),
            "--output-last-message".to_string(),
            tmp_path.to_string_lossy().into_owned(),
        ];

        if let Some(ref model) = self.model {
            args.push("-m".to_string());
            args.push(model.clone());
        }

        args.push(prompt.to_string());

        let output = Command::new("codex")
            .args(&args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("Failed to run codex CLI: {}. Is Codex CLI installed?", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = std::fs::remove_file(&tmp_path);
            return Err(format!("codex exec failed: {}", stderr).into());
        }

        let response = std::fs::read_to_string(&tmp_path)
            .map_err(|_| "codex ran but produced no output file — check codex is up to date")?;

        let _ = std::fs::remove_file(&tmp_path);

        let trimmed = response.trim().to_string();
        if trimmed.is_empty() {
            return Err("codex returned an empty response".into());
        }

        Ok(trimmed)
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
        "claudecode" | "claude-code" => {
            // Use the local claude CLI — no API key needed, runs on the user's Claude subscription.
            // Only forward an explicit SPEC_MODEL override; otherwise let claude use its own default.
            let explicit_model = std::env::var("SPEC_MODEL").ok();
            Ok(Box::new(ClaudeCodeProvider { model: explicit_model }))
        }
        "codex" => {
            // Use the local codex CLI — no API key needed, runs on the user's OpenAI subscription.
            // Only forward an explicit SPEC_MODEL override; otherwise let codex use its own default.
            let explicit_model = std::env::var("SPEC_MODEL").ok();
            Ok(Box::new(CodexProvider { model: explicit_model }))
        }
        other => Err(format!(
            "Unknown provider '{}'. Set SPEC_PROVIDER to: anthropic, openai, ollama, claudecode, codex",
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
