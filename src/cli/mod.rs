use crate::session::{all_sessions, load_session};
use crate::spec::find_all_spec_files;
use crate::memory::load_lessons;

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
    println!("Status:      {}", if session.locked { "LOCKED" } else { "OPEN" });
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

fn format_timestamp(ts: u64) -> String {
    // Simple human-readable timestamp without external deps
    // ts is seconds since UNIX epoch
    // We'll just show it in a compact format
    let seconds = ts % 60;
    let minutes = (ts / 60) % 60;
    let hours = (ts / 3600) % 24;
    let days = ts / 86400;
    // Days since epoch (Jan 1, 1970)
    // Rough date calculation
    let year = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = (day_of_year / 30) + 1;
    let day_of_month = (day_of_year % 30) + 1;
    format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        year, month.min(12), day_of_month.min(31), hours, minutes, seconds
    )
}
