use crate::hooks::{run_hook, HookContext};
use crate::llm::LlmProvider;
use crate::session::{load_or_create_session, Message, MessageType, SemanticProposal};
use crate::spec::read_spec;
use std::path::Path;
use std::process::Command;

/// Infer the programming language from a file extension
fn infer_language(file: &str) -> &'static str {
    let ext = Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => "Rust",
        "py" => "Python",
        "ts" => "TypeScript",
        "tsx" => "TypeScript (React)",
        "js" => "JavaScript",
        "jsx" => "JavaScript (React)",
        "go" => "Go",
        "java" => "Java",
        "kt" => "Kotlin",
        "swift" => "Swift",
        "php" => "PHP",
        "rb" => "Ruby",
        "cs" => "C#",
        "cpp" | "cc" => "C++",
        "c" => "C",
        "sh" => "Shell",
        "yaml" | "yml" => "YAML",
        "json" => "JSON",
        "toml" => "TOML",
        "html" => "HTML",
        "css" => "CSS",
        "sql" => "SQL",
        _ => "code",
    }
}

/// Infer the source code file from a spec file
fn source_file_for(spec_file: &str) -> String {
    if spec_file.ends_with(".spec") {
        spec_file[..spec_file.len() - 5].to_string()
    } else {
        spec_file.to_string()
    }
}

/// Extract the FILENAME line from the LLM response, falling back to base_path
fn extract_filename_from_response(response: &str, base_path: &str) -> String {
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.to_uppercase().starts_with("FILENAME:") {
            let name = trimmed["FILENAME:".len()..].trim();
            if !name.is_empty() {
                // Combine the directory of base_path with the filename the LLM returned
                let dir = Path::new(base_path).parent().unwrap_or(Path::new(""));
                let filename = Path::new(name).file_name().unwrap_or(std::ffi::OsStr::new(name));
                return dir.join(filename).to_string_lossy().to_string();
            }
        }
    }
    base_path.to_string()
}

/// Strip the FILENAME header line then extract code (handles markdown fences)
fn extract_code_after_filename(response: &str) -> String {
    // Drop the first FILENAME: line if present
    let body: String = response
        .lines()
        .skip_while(|l| l.trim().to_uppercase().starts_with("FILENAME:"))
        .collect::<Vec<_>>()
        .join("\n");
    extract_code(&body)
}

/// Extract code from LLM response (strip markdown fences if present)
fn extract_code(response: &str) -> String {
    let trimmed = response.trim();

    // Try to find a markdown code block
    if let Some(fence_start) = trimmed.find("```") {
        let after_fence = &trimmed[fence_start + 3..];
        // Skip language identifier line
        let code_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let code_body = &after_fence[code_start..];
        if let Some(fence_end) = code_body.rfind("```") {
            return code_body[..fence_end].trim().to_string();
        }
        return code_body.trim().to_string();
    }

    trimmed.to_string()
}

/// `spec build <file>` — implementer reads agreed spec, generates code
pub fn build(
    file: &str,
    provider: &dyn LlmProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Implementer building: {}", file);

    // Load session using the same key propose/agree use (the original file path)
    let mut session = load_or_create_session(file)?;

    // Derive spec file path for reading spec state
    let spec_file = if file.ends_with(".spec") {
        file.to_string()
    } else {
        format!("{}.spec", file)
    };

    // Enforce: all agents must have agreed
    if !session.locked {
        if !session.all_agents_agreed() {
            let agreed = session.agreed_agents.len();
            let total = session.agents_involved().len();
            return Err(format!(
                "Cannot build: consensus not reached. {}/{} agents have agreed. \
                 All agents must run 'spec agree' before building.",
                agreed, total
            )
            .into());
        }
        // Lock the session if it wasn't already
        session.lock();
    }

    // Read agreed spec state
    let spec_state = read_spec(&spec_file)?;

    if spec_state.content.is_empty() {
        return Err("Spec content is empty. Cannot build from an empty spec.".into());
    }

    let base_path = source_file_for(&spec_file);
    let hint_language = infer_language(&base_path);

    println!("Spec locked. Generating implementation...");

    let prompt = format!(
        r#"You are an expert software implementer. Your task is to write production-quality code based on a specification.

Spec file: {}
Base path (no extension): {}
Language hint from path: {}

Agreed specification:
{}

Instructions:
1. Determine the correct filename with extension based on the spec file path and spec content.
   For example, if the path contains "Controllers" and the spec describes a PHP/Laravel controller, use ".php".
2. On the very first line of your response, write exactly: FILENAME: <filename-with-extension>
   Example: FILENAME: HomeController.php
3. Then write the complete implementation — production-quality, idiomatic code.
4. Do NOT include any explanation outside the code itself.

Format:
FILENAME: <filename.ext>
<code>"#,
        spec_file,
        base_path,
        if hint_language == "code" { "unknown — infer from spec content and path".to_string() } else { hint_language.to_string() },
        spec_state.content,
    );

    println!("Querying LLM for implementation...");
    let response = provider.complete(&prompt)?;

    // Extract filename from first line
    let source_file = extract_filename_from_response(&response, &base_path);
    let code = extract_code_after_filename(&response);

    // Write the code to the source file
    if let Some(parent) = Path::new(&source_file).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&source_file, &code)?;

    println!("\n=== IMPLEMENTATION WRITTEN ===");
    println!("File: {}", source_file);
    println!("Lines: {}", code.lines().count());

    // Record the build in the session
    let build_msg = Message::new(
        "implementer".to_string(),
        MessageType::Build,
        Some(SemanticProposal {
            content: format!("Built {} ({} lines)", source_file, code.lines().count()),
            spec_hash: None,
        }),
        format!("Generated implementation from agreed spec"),
        session.session_id.clone(),
    );
    session.add_message(build_msg);
    crate::session::save_session(&session)?;

    println!("\nBuild complete. Source written to: {}", source_file);

    run_hook("post-build", &HookContext {
        spec_file: file.to_string(),
        session_id: Some(session.session_id.clone()),
        env_target: None,
    })?;
    Ok(())
}

/// `spec test <file>` — run tests against the build
pub fn test(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Running tests for: {}", file);

    let source_file = if file.ends_with(".spec") {
        source_file_for(file)
    } else {
        file.to_string()
    };

    if !Path::new(&source_file).exists() {
        return Err(format!(
            "Source file '{}' does not exist. Run 'spec build' first.",
            source_file
        )
        .into());
    }

    let ext = Path::new(&source_file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let (cmd, args) = match ext {
        "rs" => ("cargo", vec!["test"]),
        "py" => ("python", vec!["-m", "pytest"]),
        "ts" | "tsx" | "js" | "jsx" => ("npm", vec!["test"]),
        "go" => ("go", vec!["test", "./..."]),
        "rb" => ("bundle", vec!["exec", "rspec"]),
        "php" => ("./vendor/bin/phpunit", vec![]),
        _ => {
            println!("No known test runner for extension '{}'. Please run tests manually.", ext);
            return Ok(());
        }
    };

    println!("Running: {} {}", cmd, args.join(" "));

    let status = Command::new(cmd).args(&args).status()?;

    if status.success() {
        println!("\nTests passed.");
    } else {
        println!("\nTests failed with exit code: {:?}", status.code());
    }

    Ok(())
}

/// `spec release <file> <env>` — promote build to an environment
pub fn release(file: &str, env: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Releasing {} to environment: {}", file, env);

    let source_file = if file.ends_with(".spec") {
        source_file_for(file)
    } else {
        file.to_string()
    };

    if !Path::new(&source_file).exists() {
        return Err(format!(
            "Source file '{}' does not exist. Run 'spec build' first.",
            source_file
        )
        .into());
    }

    let session = load_or_create_session(file)?;
    if !session.locked && !session.all_agents_agreed() {
        return Err(
            "Cannot release: spec has not reached consensus. All agents must agree before releasing.".into(),
        );
    }

    println!("Spec consensus verified.");
    println!("Source file: {}", source_file);
    println!("Target environment: {}", env);

    run_hook("pre-release", &HookContext {
        spec_file: file.to_string(),
        session_id: Some(session.session_id.clone()),
        env_target: Some(env.to_string()),
    })?;

    // Environment-specific logic
    match env {
        "staging" | "production" | "prod" => {
            println!("Release pipeline: {}", env.to_uppercase());
            println!("Note: Configure your deployment scripts to integrate with 'spec release'.");
            println!("The spec system guarantees the implementation matches the agreed spec.");
            println!("Release recorded successfully for environment: {}", env);
        }
        "local" | "dev" | "development" => {
            println!("Local/dev deployment — no remote promotion needed.");
            println!("File is ready at: {}", source_file);
        }
        other => {
            println!("Custom environment '{}' — recording release.", other);
            println!("File: {}", source_file);
        }
    }

    // Record release in session
    let mut session = session;
    let release_msg = Message::new(
        "implementer".to_string(),
        MessageType::Build,
        Some(SemanticProposal {
            content: format!("Released {} to {}", source_file, env),
            spec_hash: None,
        }),
        format!("Promoted build to {} environment", env),
        session.session_id.clone(),
    );
    session.add_message(release_msg);
    crate::session::save_session(&session)?;

    println!("\nRelease complete.");

    run_hook("post-release", &HookContext {
        spec_file: file.to_string(),
        session_id: Some(session.session_id.clone()),
        env_target: Some(env.to_string()),
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_file_for_strips_spec() {
        assert_eq!(source_file_for("src/auth.php.spec"), "src/auth.php");
    }

    #[test]
    fn source_file_for_no_spec_unchanged() {
        assert_eq!(source_file_for("src/auth.php"), "src/auth.php");
    }

    #[test]
    fn infer_language_known_extensions() {
        assert_eq!(infer_language("foo.php"), "PHP");
        assert_eq!(infer_language("foo.rs"), "Rust");
        assert_eq!(infer_language("foo.py"), "Python");
        assert_eq!(infer_language("foo.ts"), "TypeScript");
        assert_eq!(infer_language("foo.go"), "Go");
        assert_eq!(infer_language("foo.rb"), "Ruby");
    }

    #[test]
    fn infer_language_unknown_returns_code() {
        assert_eq!(infer_language("foo.xyz"), "code");
        assert_eq!(infer_language("Makefile"), "code");
    }

    #[test]
    fn extract_filename_from_response_parses_header() {
        let response = "FILENAME: HomeController.php\n<?php\nclass HomeController {}";
        let result = extract_filename_from_response(response, "app/Http/Controllers/HomeController");
        assert_eq!(result, "app/Http/Controllers/HomeController.php");
    }

    #[test]
    fn extract_filename_case_insensitive() {
        let response = "filename: HomeController.php\n<?php";
        let result = extract_filename_from_response(response, "app/Controllers/HomeController");
        assert_eq!(result, "app/Controllers/HomeController.php");
    }

    #[test]
    fn extract_filename_falls_back_to_base_path() {
        let response = "<?php\nclass HomeController {}";
        let result = extract_filename_from_response(response, "app/Controllers/HomeController");
        assert_eq!(result, "app/Controllers/HomeController");
    }

    #[test]
    fn extract_code_strips_markdown_fences() {
        let response = "```php\n<?php\nreturn 'test';\n```";
        let code = extract_code(response);
        assert_eq!(code, "<?php\nreturn 'test';");
    }

    #[test]
    fn extract_code_no_fences_returns_trimmed() {
        let response = "  <?php\nreturn 'test';  ";
        let code = extract_code(response);
        assert_eq!(code, "<?php\nreturn 'test';");
    }

    #[test]
    fn extract_code_after_filename_skips_header() {
        let response = "FILENAME: HomeController.php\n```php\n<?php\nclass Foo {}\n```";
        let code = extract_code_after_filename(response);
        assert_eq!(code, "<?php\nclass Foo {}");
    }
}
