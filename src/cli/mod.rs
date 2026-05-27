use crate::session::{all_sessions, load_session};
use crate::spec::find_all_spec_files;
use crate::memory::load_lessons;
use crate::queue::{load_queue, save_queue};

/// `spec reset <file>` — delete the session for a spec file so a fresh proposal can be made
pub fn reset(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let removed = crate::session::reset_session(file)?;
    if removed == 0 {
        println!("No session found for: {}", file);
    } else {
        println!("Session reset for: {} ({} file(s) removed)", file, removed);
        println!("Run 'spec propose {}' to start a new session.", file);
    }
    Ok(())
}

/// `spec status` — observe current project state
pub fn status() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== SPEC PROJECT STATUS ===\n");

    // Check if initialized
    if !std::path::Path::new(".spec").exists() {
        println!("Not initialized. Run 'spec init' first.");
        return Ok(());
    }

    // Find all .spec files
    let spec_files = find_all_spec_files()?;
    println!("Spec files found: {}", spec_files.len());
    for f in &spec_files {
        println!("  {}", f.display());
    }

    println!();

    // Load all sessions
    let sessions = all_sessions()?;
    println!("Open sessions: {}", sessions.iter().filter(|s| !s.locked).count());
    println!("Locked sessions: {}", sessions.iter().filter(|s| s.locked).count());

    if !sessions.is_empty() {
        println!();
        for session in &sessions {
            let status_str = if session.locked {
                "LOCKED (consensus reached)"
            } else {
                "OPEN"
            };
            println!("Session: {} [{}]", session.session_id, status_str);
            println!("  Spec file: {}", session.spec_file);
            println!("  Messages: {}", session.messages.len());

            let agents = session.agents_involved();
            if !agents.is_empty() {
                println!("  Agents involved: {}", agents.join(", "));
            }

            if !session.agreed_agents.is_empty() {
                println!("  Agreed agents: {}", session.agreed_agents.join(", "));
            }

            let agreed_count = session.agreed_agents.len();
            let total_count = agents.len();
            if total_count > 0 {
                println!(
                    "  Consensus state: {}/{} agents agreed",
                    agreed_count, total_count
                );
                if agreed_count == total_count && total_count > 0 {
                    println!("  *** CONSENSUS REACHED ***");
                }
            }
            println!();
        }
    }

    // Memory
    let lessons = load_lessons()?;
    println!("Lessons in memory: {}", lessons.lessons.len());

    Ok(())
}

/// Compute the machine-readable status for a spec file (no I/O side effects).
fn compute_status(file: &str) -> Result<&'static str, Box<dyn std::error::Error>> {
    let session = match load_session(file)? {
        Some(s) => s,
        None => return Ok("NO_SESSION"),
    };

    if session.locked {
        return Ok("LOCKED");
    }

    use crate::session::MessageType;

    let substantive: Vec<_> = session.messages.iter()
        .filter(|m| matches!(m.message_type,
            MessageType::Propose | MessageType::Respond |
            MessageType::Concede | MessageType::Agree))
        .collect();

    let last_type = substantive.last().map(|m| &m.message_type);
    let has_agree = substantive.iter().any(|m| matches!(m.message_type, MessageType::Agree));
    let multi_agent = session.participating_agent_count() >= 2;

    // STUCK: competing proposals (two agents both proposed without engaging each other),
    // or enough back-and-forth with no agreement yet.
    let competing_proposals = {
        use std::collections::HashSet;
        let proposing_agents: HashSet<&str> = substantive.iter()
            .filter(|m| matches!(m.message_type, MessageType::Propose))
            .map(|m| m.agent_id.as_str())
            .collect();
        proposing_agents.len() >= 2
    };
    if multi_agent && !has_agree && (competing_proposals || substantive.len() >= 3) {
        return Ok("STUCK");
    }

    Ok(match last_type {
        Some(MessageType::Propose) => "WAITING_FOR_REPLY",
        Some(MessageType::Respond) | Some(MessageType::Concede) => "WAITING_FOR_AGREE",
        Some(MessageType::Agree) => {
            if !multi_agent { "WAITING_FOR_REPLY" } else { "WAITING_FOR_AGREE" }
        }
        _ => "WAITING_FOR_REPLY",
    })
}

/// `spec state <file>` — machine-readable status for polling (no LLM, no noise)
pub fn state(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("STATUS: {}", compute_status(file)?);
    Ok(())
}

/// `spec wait [<file>] <status> [--timeout <secs>]`
/// Blocks until a session reaches the target status, then exits.
/// Without a file, watches all sessions. Default timeout: 30s to stay within AI tool limits.
/// Exits with STATUS: TIMEOUT if the timeout is reached — call again to keep waiting.
pub fn wait(file: Option<&str>, target: &str, timeout_secs: u64) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    let target = target.to_uppercase();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

    loop {
        match file {
            Some(f) => {
                if compute_status(f)? == target {
                    println!("STATUS: {}", target);
                    std::io::stdout().flush()?;
                    return Ok(());
                }
            }
            None => {
                let sessions = crate::session::all_sessions()?;
                for session in &sessions {
                    if compute_status(&session.spec_file)? == target {
                        println!("STATUS: {} {}", target, session.spec_file);
                        std::io::stdout().flush()?;
                        return Ok(());
                    }
                }
            }
        }

        if std::time::Instant::now() >= deadline {
            println!("STATUS: TIMEOUT");
            std::io::stdout().flush()?;
            return Ok(());
        }

        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// `spec watch <file>` — block and emit a STATUS line whenever the session changes.
/// Mediator polls for STUCK; implementer polls for LOCKED.
pub fn watch(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let mut last = String::new();
    loop {
        let current = compute_status(file)?;
        if current != last {
            println!("STATUS: {}", current);
            std::io::stdout().flush()?;
            last = current.to_string();
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// `spec log <file>` — full session message history for a spec
pub fn log(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== SESSION LOG: {} ===\n", file);

    let session = match load_session(file)? {
        Some(s) => s,
        None => {
            println!("No session found for: {}", file);
            println!("Run 'spec propose {}' to start a session.", file);
            return Ok(());
        }
    };

    println!("Session ID:  {}", session.session_id);
    println!("Spec file:   {}", session.spec_file);
    println!("Status:      {}", match (session.locked, session.solo) {
        (true, true)  => "LOCKED (solo agreement)",
        (true, false) => "LOCKED (consensus reached)",
        _             => "OPEN",
    });
    println!("Messages:    {}", session.messages.len());
    println!("Created at:  {}", format_timestamp(session.created_at));

    let agents = session.agents_involved();
    if !agents.is_empty() {
        println!("Agents:      {}", agents.join(", "));
    }

    if !session.agreed_agents.is_empty() {
        println!("Agreed:      {}", session.agreed_agents.join(", "));
    }

    println!("\n{}", "─".repeat(60));

    for (i, msg) in session.messages.iter().enumerate() {
        println!("\n[{}] Message #{}", format_timestamp(msg.timestamp), i + 1);
        println!("  Agent:   {}", msg.agent_id);
        println!("  Type:    {}", msg.message_type);
        println!("  Session: {}", msg.session_id);

        if let Some(proposal) = &msg.proposal {
            println!("\n  Proposal:");
            for line in proposal.content.lines() {
                println!("    {}", line);
            }
            if let Some(hash) = &proposal.spec_hash {
                println!("  Spec hash: {}", hash);
            }
        }

        println!("\n  Reasoning:");
        for line in msg.reasoning.lines() {
            println!("    {}", line);
        }

        println!("{}", "─".repeat(60));
    }

    if session.messages.is_empty() {
        println!("No messages in this session.");
    }

    Ok(())
}

/// `spec lessons` — view the lesson graph
pub fn lessons() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== LESSON GRAPH ===\n");

    let graph = load_lessons()?;

    if graph.lessons.is_empty() {
        println!("No lessons recorded yet.");
        println!("Lessons are added when sessions surface important patterns.");
        return Ok(());
    }

    println!("Total lessons: {}\n", graph.lessons.len());

    for lesson in &graph.lessons {
        println!("Lesson: {}", lesson.id);
        println!("  Description: {}", lesson.description);
        println!("  Session:     {}", lesson.provenance.session_id);
        println!("  Timestamp:   {}", format_timestamp(lesson.provenance.timestamp));
        println!("  Intervention: {}", lesson.provenance.intervention);
        println!("  Breakdown:   {}", lesson.provenance.breakdown_point);

        if !lesson.references.is_empty() {
            println!("  References:");
            for r in &lesson.references {
                println!("    - {}", r);
            }
        }

        if !lesson.reinforcements.is_empty() {
            println!("  Reinforcements: {}", lesson.reinforcements.len());
            for r in &lesson.reinforcements {
                println!("    - {}", r);
            }
        }

        println!();
    }

    Ok(())
}

const SKILL_AGENT: &str = include_str!("../../skills/spec-agent/SKILL.md");
const SKILL_MEDIATOR: &str = include_str!("../../skills/spec-mediator/SKILL.md");
const SKILL_IMPLEMENTER: &str = include_str!("../../skills/spec-implementer/SKILL.md");
const SKILL_PROPOSER: &str = include_str!("../../skills/spec-proposer/SKILL.md");
const SKILL_ORCHESTRATOR: &str = include_str!("../../skills/spec-orchestrator/SKILL.md");

const SKILLS: &[(&str, &str)] = &[
    ("spec-agent", SKILL_AGENT),
    ("spec-mediator", SKILL_MEDIATOR),
    ("spec-implementer", SKILL_IMPLEMENTER),
    ("spec-proposer", SKILL_PROPOSER),
    ("spec-orchestrator", SKILL_ORCHESTRATOR),
];

/// `spec install-skills` — install skills to the target directory
pub fn install_skills(target: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let dest = if let Some(t) = target {
        std::path::PathBuf::from(t)
    } else {
        detect_default_skills_dir()
            .ok_or("Could not detect a supported AI tool (checked ~/.codex and ~/.claude). Use --target <dir> to specify a destination.")?
    };

    write_skills(&dest)?;
    println!("\nDone. {} skills installed.", SKILLS.len());
    Ok(())
}

/// Called from `spec init` — installs to Codex by default, and to Claude Code if detected.
pub fn try_install_skills() {
    for (tool, dest) in detect_skills_dirs_for_init() {
        println!("\nDetected {} — installing skills to {}", tool, dest.display());
        if let Err(e) = write_skills(&dest) {
            eprintln!("  Warning: could not install skills: {}", e);
        }
    }
}

fn write_skills(dest: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(dest)?;
    for (name, content) in SKILLS {
        let skill_dir = dest.join(name);
        std::fs::create_dir_all(&skill_dir)?;
        std::fs::write(skill_dir.join("SKILL.md"), content)?;
        println!("  ✓ {}", name);
    }
    Ok(())
}

fn detect_default_skills_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let home = std::path::Path::new(&home);

    if home.join(".codex").exists() {
        return Some(home.join(".codex").join("skills"));
    }

    if home.join(".claude").exists() {
        return Some(home.join(".claude").join("skills"));
    }

    None
}

/// `spec run <role> [name] --with <claude|codex>`
pub fn run(role: &str, name: Option<&str>, runner: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (skill, needs_id) = match role {
        "agent"        => ("spec-agent", true),
        "proposer"     => ("spec-proposer", true),
        "mediator"     => ("spec-mediator", false),
        "implementer"  => ("spec-implementer", false),
        "orchestrator" => ("spec-orchestrator", false),
        _ => return Err(format!("Unknown role '{}'. Valid roles: agent, proposer, mediator, implementer, orchestrator", role).into()),
    };

    if needs_id && name.is_none() {
        return Err(format!("Role '{}' requires a name. Usage: spec run {} <name> --with <runner>", role, role).into());
    }

    let (binary, provider, prompt) = match runner {
        "claude" => ("claude", "claudecode", format!("/{}", skill)),
        "codex"  => ("codex",  "codex",      format!("use the {} skill", skill)),
        _ => return Err(format!("Unknown runner '{}'. Valid options: claude, codex", runner).into()),
    };

    println!("Launching {} as {} ({})...", binary, role, skill);

    let mut cmd = std::process::Command::new(binary);
    cmd.arg(&prompt);
    cmd.env("SPEC_PROVIDER", provider);
    cmd.env("SPEC_ROLE", role);

    if let Some(id) = name {
        cmd.env("SPEC_AGENT_ID", id);
        println!("  SPEC_AGENT_ID={}", id);
    }

    println!("  SPEC_PROVIDER={}", provider);
    println!("  SPEC_ROLE={}", role);

    use std::os::unix::process::CommandExt;
    let err = cmd.exec();
    Err(format!("Failed to launch {}: {}", binary, err).into())
}

fn detect_skills_dirs_for_init() -> Vec<(&'static str, std::path::PathBuf)> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let home = std::path::Path::new(&home);

    let mut out = Vec::new();

    // For `spec init`, install to Codex by default (creating ~/.codex/skills if needed).
    out.push(("Codex", home.join(".codex").join("skills")));

    // Also install to Claude Code if Claude is present.
    if home.join(".claude").exists() {
        out.push(("Claude Code", home.join(".claude").join("skills")));
    }

    out
}

/// `spec queue add <file> "<intent>" [--after <task-id>...]`
pub fn queue_add(file: &str, intent: &str, depends_on: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut queue = load_queue()?;
    // Validate dependencies exist
    for dep in &depends_on {
        if !queue.tasks.iter().any(|t| &t.id == dep) {
            return Err(format!("Unknown task id '{}'. Run 'spec queue list' to see task ids.", dep).into());
        }
    }
    let id = queue.add(file.to_string(), intent.to_string(), depends_on);
    save_queue(&queue)?;
    println!("Task added: {}", id);
    println!("  File:   {}", file);
    println!("  Intent: {}", intent);
    Ok(())
}

/// `spec queue list` — show all tasks and their status
pub fn queue_list() -> Result<(), Box<dyn std::error::Error>> {
    let mut queue = load_queue()?;
    queue.sync_with_sessions();
    save_queue(&queue)?;

    if queue.tasks.is_empty() {
        println!("Queue is empty. Add tasks with 'spec queue add <file> \"<intent>\"'.");
        return Ok(());
    }

    let pending   = queue.tasks.iter().filter(|t| t.status == crate::queue::TaskStatus::Pending).count();
    let blocked   = queue.blocked_count();
    let in_prog   = queue.tasks.iter().filter(|t| t.status == crate::queue::TaskStatus::InProgress).count();
    let done      = queue.tasks.iter().filter(|t| t.status == crate::queue::TaskStatus::Done).count();

    println!("=== TASK QUEUE ===");
    println!("  {} pending  {} blocked  {} in progress  {} done\n", pending, blocked, in_prog, done);

    let tasks_snapshot = queue.tasks.clone();
    for task in &queue.tasks {
        let marker = match task.status {
            crate::queue::TaskStatus::Pending if task.is_unblocked(&tasks_snapshot) => "[ ]",
            crate::queue::TaskStatus::Pending => "[B]",
            crate::queue::TaskStatus::InProgress => "[~]",
            crate::queue::TaskStatus::Done => "[✓]",
        };
        println!("{} {}  {}", marker, task.id, task.file);
        println!("    {}", task.intent);
        if let Some(ref agent) = task.assigned_agent {
            println!("    Assigned: {}", agent);
        }
        if !task.depends_on.is_empty() {
            let dep_statuses: Vec<String> = task.depends_on.iter().map(|dep_id| {
                let done = tasks_snapshot.iter()
                    .find(|t| &t.id == dep_id)
                    .map(|t| t.status == crate::queue::TaskStatus::Done)
                    .unwrap_or(false);
                format!("{} {}", dep_id, if done { "✓" } else { "✗" })
            }).collect();
            println!("    Depends on: {}", dep_statuses.join(", "));
        }
        println!();
    }

    Ok(())
}

/// `spec queue done <task-id>` — manually mark a task complete
pub fn queue_done(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut queue = load_queue()?;
    if queue.mark_done(id) {
        save_queue(&queue)?;
        println!("Task {} marked done.", id);
    } else {
        println!("No task found with id '{}'.", id);
    }
    Ok(())
}

/// `spec next` — claim the next unblocked task and print what to run
pub fn next(agent_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut queue = load_queue()?;
    queue.sync_with_sessions();

    match queue.claim_next(agent_id) {
        Some(task) => {
            save_queue(&queue)?;
            println!("=== NEXT TASK for {} ===\n", agent_id);
            println!("Task:   {}", task.id);
            println!("File:   {}", task.file);
            println!("Intent: {}", task.intent);
            println!("\nRun:");
            println!("  SPEC_AGENT_ID={} spec propose {} \"{}\"", agent_id, task.file, task.intent);
        }
        None => {
            save_queue(&queue)?;
            let blocked = queue.blocked_count();
            let pending = queue.pending_count();
            if pending == 0 && blocked == 0 {
                println!("No tasks in queue. All work is done or the queue is empty.");
            } else if blocked > 0 && pending == blocked {
                println!("All remaining tasks are blocked on dependencies.");
                println!("Run 'spec queue list' to see what's pending.");
            } else {
                println!("No unblocked tasks available right now.");
                println!("Run 'spec queue list' to see queue state.");
            }
        }
    }

    Ok(())
}

fn format_timestamp(ts: u64) -> String {
    let seconds = (ts % 60) as u32;
    let minutes = ((ts / 60) % 60) as u32;
    let hours = ((ts / 3600) % 24) as u32;

    // Gregorian calendar date from days since 1970-01-01
    let mut days = (ts / 86400) as i64;
    let mut year = 1970i32;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let month_lengths = [
        31, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut month = 1u32;
    for &len in &month_lengths {
        if days < len {
            break;
        }
        days -= len;
        month += 1;
    }
    let day = days as u32 + 1;

    format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        year, month, day, hours, minutes, seconds
    )
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
