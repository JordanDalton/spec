use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecState {
    pub version: u64,
    pub content: String,
    pub session_id: Option<String>,
    pub locked_at: Option<u64>,
    pub authors: Vec<String>,
}

impl SpecState {
    pub fn empty() -> Self {
        SpecState {
            version: 0,
            content: String::new(),
            session_id: None,
            locked_at: None,
            authors: Vec::new(),
        }
    }

    pub fn new(content: String, session_id: String, authors: Vec<String>) -> Self {
        let version = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        SpecState {
            version,
            content,
            session_id: Some(session_id),
            locked_at: Some(version),
            authors,
        }
    }
}

/// Infer the spec file path from a source file path.
/// If the path already ends with ".spec", return it as-is.
/// Otherwise, append ".spec".
pub fn spec_path_for(file: &str) -> PathBuf {
    if file.ends_with(".spec") {
        PathBuf::from(file)
    } else {
        PathBuf::from(format!("{}.spec", file))
    }
}

/// Read the spec state from disk. Returns empty state if file doesn't exist.
pub fn read_spec(spec_file: &str) -> Result<SpecState, Box<dyn std::error::Error>> {
    let path = spec_path_for(spec_file);
    if !path.exists() {
        return Ok(SpecState::empty());
    }
    let content = std::fs::read_to_string(&path)?;
    // Try to parse as JSON first (structured spec state)
    if let Ok(state) = serde_json::from_str::<SpecState>(&content) {
        return Ok(state);
    }
    // Otherwise treat raw text as spec content
    Ok(SpecState {
        version: 0,
        content,
        session_id: None,
        locked_at: None,
        authors: Vec::new(),
    })
}

/// Write the spec state to disk as plain markdown.
/// Metadata (version, session_id, authors) lives in the session log — not here.
pub fn write_spec(spec_file: &str, state: &SpecState) -> Result<(), Box<dyn std::error::Error>> {
    let path = spec_path_for(spec_file);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&path, &state.content)?;
    Ok(())
}

/// Find all .spec files in the project (walking from cwd)
pub fn find_all_spec_files() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut results = Vec::new();
    let cwd = std::env::current_dir()?;
    walk_for_spec_files(&cwd, &cwd, &mut results)?;
    Ok(results)
}

fn walk_for_spec_files(
    base: &Path,
    dir: &Path,
    results: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Skip .spec directory and hidden directories
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_for_spec_files(base, &path, results)?;
        } else if path.extension().map(|e| e == "spec").unwrap_or(false) {
            let rel = path.strip_prefix(base).unwrap_or(&path);
            results.push(rel.to_path_buf());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub llm_provider: String,
    pub model: String,
    pub anthropic_api_key_env: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            llm_provider: "anthropic".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            anthropic_api_key_env: "ANTHROPIC_API_KEY".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_path_for_already_spec() {
        let p = spec_path_for("src/auth.php.spec");
        assert_eq!(p, std::path::PathBuf::from("src/auth.php.spec"));
    }

    #[test]
    fn spec_path_for_php_file() {
        let p = spec_path_for("src/auth.php");
        assert_eq!(p, std::path::PathBuf::from("src/auth.php.spec"));
    }

    #[test]
    fn spec_path_for_no_extension() {
        let p = spec_path_for("src/Makefile");
        assert_eq!(p, std::path::PathBuf::from("src/Makefile.spec"));
    }

    #[test]
    fn spec_path_for_nested() {
        let p = spec_path_for("app/Http/Controllers/HomeController.php");
        assert_eq!(p, std::path::PathBuf::from("app/Http/Controllers/HomeController.php.spec"));
    }
}

pub fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let path = Path::new(".spec").join("config.json");
    if !path.exists() {
        return Ok(Config::default());
    }
    let content = std::fs::read_to_string(&path)?;
    let config: Config = serde_json::from_str(&content)?;
    Ok(config)
}

pub fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new(".spec").join("config.json");
    let content = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, content)?;
    Ok(())
}
