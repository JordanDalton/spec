use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

pub struct HookContext {
    pub spec_file: String,
    pub session_id: Option<String>,
    pub env_target: Option<String>,
}

/// Run a hook script from .spec/hooks/<name> if it exists.
/// Returns Err if the hook exits non-zero (for pre- hooks that should abort).
pub fn run_hook(name: &str, ctx: &HookContext) -> Result<(), Box<dyn std::error::Error>> {
    let hook_path = Path::new(".spec").join("hooks").join(name);

    if !hook_path.exists() {
        return Ok(());
    }

    // Ensure it's executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&hook_path)?;
        if meta.permissions().mode() & 0o111 == 0 {
            eprintln!(
                "Warning: hook '{}' exists but is not executable. Run: chmod +x {}",
                name,
                hook_path.display()
            );
            return Ok(());
        }
    }

    let mut env_vars: HashMap<&str, String> = HashMap::new();
    env_vars.insert("SPEC_FILE", ctx.spec_file.clone());
    if let Some(ref sid) = ctx.session_id {
        env_vars.insert("SPEC_SESSION_ID", sid.clone());
    }
    if let Some(ref env) = ctx.env_target {
        env_vars.insert("SPEC_ENV", env.clone());
    }

    println!("Running hook: {}", name);

    let status = Command::new(&hook_path)
        .envs(&env_vars)
        .status()
        .map_err(|e| format!("Failed to run hook '{}': {}", name, e))?;

    if !status.success() {
        return Err(format!(
            "Hook '{}' failed with exit code {:?}. Aborting.",
            name,
            status.code()
        )
        .into());
    }

    Ok(())
}

/// Create the .spec/hooks directory and example hook scripts during init.
pub fn init_hooks() -> Result<(), Box<dyn std::error::Error>> {
    let hooks_dir = Path::new(".spec").join("hooks");
    std::fs::create_dir_all(&hooks_dir)?;

    write_example_hook(
        &hooks_dir.join("post-agree.example"),
        "post-agree",
        r#"#!/bin/sh
# Called when all agents reach consensus and the session locks.
#
# Environment variables:
#   SPEC_FILE        — the spec file that reached consensus
#   SPEC_SESSION_ID  — the session ID
#
# Example: notify a Slack channel
# curl -s -X POST "$SLACK_WEBHOOK" \
#   -H "Content-Type: application/json" \
#   -d "{\"text\": \"Consensus reached on $SPEC_FILE (session $SPEC_SESSION_ID)\"}"

echo "post-agree: $SPEC_FILE (session $SPEC_SESSION_ID)"
"#,
    )?;

    write_example_hook(
        &hooks_dir.join("post-build.example"),
        "post-build",
        r#"#!/bin/sh
# Called after the implementer writes the code file.
#
# Environment variables:
#   SPEC_FILE        — the spec file that was built
#   SPEC_SESSION_ID  — the session ID
#
# Example: run a linter on the output
# php -l "${SPEC_FILE%.spec}"

echo "post-build: $SPEC_FILE"
"#,
    )?;

    write_example_hook(
        &hooks_dir.join("pre-release.example"),
        "pre-release",
        r#"#!/bin/sh
# Called before a release is recorded. Exit non-zero to abort the release.
#
# Environment variables:
#   SPEC_FILE        — the file being released
#   SPEC_SESSION_ID  — the session ID
#   SPEC_ENV         — the target environment (e.g. staging, production)
#
# Example: block releases to production on Fridays
# if [ "$SPEC_ENV" = "production" ] && [ "$(date +%u)" -eq 5 ]; then
#   echo "No production releases on Fridays." >&2
#   exit 1
# fi

echo "pre-release: $SPEC_FILE → $SPEC_ENV"
"#,
    )?;

    write_example_hook(
        &hooks_dir.join("post-release.example"),
        "post-release",
        r#"#!/bin/sh
# Called after a release is successfully recorded.
#
# Environment variables:
#   SPEC_FILE        — the file that was released
#   SPEC_SESSION_ID  — the session ID
#   SPEC_ENV         — the target environment
#
# Example: trigger a deployment pipeline
# curl -s -X POST "https://ci.example.com/deploy" \
#   -H "Authorization: Bearer $CI_TOKEN" \
#   -d "env=$SPEC_ENV&file=$SPEC_FILE"

echo "post-release: $SPEC_FILE → $SPEC_ENV"
"#,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> HookContext {
        HookContext {
            spec_file: "src/auth.php".to_string(),
            session_id: Some("sess_abc".to_string()),
            env_target: Some("production".to_string()),
        }
    }

    #[test]
    fn missing_hook_is_skipped() {
        let result = run_hook("nonexistent-hook-xyz-abc", &ctx());
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn failing_hook_returns_error() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("spec-hook-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let hook_path = dir.join("pre-release-test-fail");
        std::fs::write(&hook_path, "#!/bin/sh\nexit 1\n").unwrap();
        let mut perms = std::fs::metadata(&hook_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_path, perms).unwrap();

        // Temporarily create the hook in .spec/hooks/ path by running from the temp dir
        // Instead, test run_hook directly by changing what it looks up
        // Since run_hook always looks in .spec/hooks/, we write there for the test
        let hooks_dir = Path::new(".spec").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        let test_hook = hooks_dir.join("test-fail-hook");
        std::fs::write(&test_hook, "#!/bin/sh\nexit 1\n").unwrap();
        let mut perms = std::fs::metadata(&test_hook).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&test_hook, perms).unwrap();

        let result = run_hook("test-fail-hook", &ctx());
        std::fs::remove_file(&test_hook).ok();
        assert!(result.is_err());
    }

    #[test]
    #[cfg(unix)]
    fn passing_hook_returns_ok() {
        use std::os::unix::fs::PermissionsExt;

        let hooks_dir = Path::new(".spec").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        let test_hook = hooks_dir.join("test-pass-hook");
        std::fs::write(&test_hook, "#!/bin/sh\nexit 0\n").unwrap();
        let mut perms = std::fs::metadata(&test_hook).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&test_hook, perms).unwrap();

        let result = run_hook("test-pass-hook", &ctx());
        std::fs::remove_file(&test_hook).ok();
        assert!(result.is_ok());
    }
}

fn write_example_hook(path: &Path, _name: &str, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::write(path, content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }

    Ok(())
}
