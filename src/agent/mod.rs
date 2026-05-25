use crate::hooks::{run_hook, HookContext};
use crate::llm::LlmProvider;
use crate::memory;
use crate::session::{Message, MessageType, SemanticProposal, Session};
use crate::spec::{read_spec, write_spec, SpecState};

fn get_agent_id() -> Result<String, Box<dyn std::error::Error>> {
    std::env::var("SPEC_AGENT_ID").map_err(|_| {
        "SPEC_AGENT_ID is not set.\n\
         Agent identity must be explicit — every command you run needs a stable, consistent ID.\n\
         Set it before running spec commands:\n\
         \n\
           export SPEC_AGENT_ID=alice\n\
         \n\
         Without this, each invocation gets a new process ID and the system treats them as different agents."
            .into()
    })
}

fn format_lessons(lessons: &[memory::Lesson]) -> String {
    if lessons.is_empty() {
        return "No relevant lessons found.".to_string();
    }
    lessons
        .iter()
        .map(|l| format!("- [{}] {}", l.id, l.description))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_session_history(session: &Session) -> String {
    if session.messages.is_empty() {
        return "No previous messages in this session.".to_string();
    }
    session
        .messages
        .iter()
        .map(|m| {
            let proposal_text = m
                .proposal
                .as_ref()
                .map(|p| format!("\n  Proposal: {}", p.content))
                .unwrap_or_default();
            format!(
                "[{}] {} | {}{}\n  Reasoning: {}",
                m.timestamp, m.agent_id, m.message_type, proposal_text, m.reasoning
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `spec propose <file> "<intent>"` — agent proposes a spec change
pub fn propose(
    file: &str,
    intent: &str,
    provider: &dyn LlmProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    let agent_id = get_agent_id()?;
    println!("Agent [{}] proposing for: {}", agent_id, file);

    // Read current spec state
    let spec_state = read_spec(file)?;

    // Load session
    let mut session = crate::session::load_or_create_session(file)?;

    // Query relevant lessons
    let lessons = memory::get_relevant_lessons(intent)?;

    // Build prompt
    let prompt = format!(
        r#"You are an AI agent participating in a spec-driven development process.

Current spec file: {}

Current spec content:
{}

Session history:
{}

Relevant lessons from past sessions:
{}

User intent / change request:
{}

Your task: Propose a concrete change to the spec that fulfills the user's intent.
Your response must include:
1. PROPOSAL: The new or modified spec content (complete, not a diff)
2. REASONING: Why this change is appropriate and how it fulfills the intent

Format your response as:
PROPOSAL:
<the full proposed spec content>

REASONING:
<your reasoning>"#,
        file,
        if spec_state.content.is_empty() {
            "(empty - new spec)".to_string()
        } else {
            spec_state.content.clone()
        },
        format_session_history(&session),
        format_lessons(&lessons),
        intent
    );

    println!("Querying LLM for proposal...");
    let response = provider.complete(&prompt)?;

    // Parse response
    let (proposal_content, reasoning) = parse_proposal_response(&response);

    let proposal = SemanticProposal {
        content: proposal_content.clone(),
        spec_hash: Some(simple_hash(&proposal_content)),
    };

    let msg = Message::new(
        agent_id.clone(),
        MessageType::Propose,
        Some(proposal),
        reasoning.clone(),
        session.session_id.clone(),
    );

    println!("\n=== PROPOSAL by {} ===", agent_id);
    println!("{}", proposal_content);
    println!("\n=== REASONING ===");
    println!("{}", reasoning);

    session.add_message(msg);
    crate::session::save_session(&session)?;

    println!("\nProposal recorded in session: {}", session.session_id);
    Ok(())
}

/// `spec respond <file>` — agent responds to an existing proposal
pub fn respond(
    file: &str,
    provider: &dyn LlmProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    let agent_id = get_agent_id()?;
    println!("Agent [{}] responding to: {}", agent_id, file);

    let spec_state = read_spec(file)?;
    let mut session = crate::session::load_or_create_session(file)?;

    if session.messages.is_empty() {
        return Err("No proposals to respond to. Run 'spec propose' first.".into());
    }

    let lessons = memory::get_relevant_lessons(file)?;

    // Find the latest proposal
    let latest_proposal = session
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.message_type, MessageType::Propose | MessageType::Concede))
        .and_then(|m| m.proposal.as_ref())
        .map(|p| p.content.clone())
        .unwrap_or_else(|| "(no proposal found)".to_string());

    let prompt = format!(
        r#"You are an AI agent reviewing a proposed spec change.

Spec file: {}

Current agreed spec content:
{}

Latest proposed spec content:
{}

Full session history:
{}

Relevant lessons:
{}

Your task: Respond to the latest proposal. You may:
- ACCEPT it (if you agree it's correct)
- REJECT it with specific objections
- SUGGEST modifications

Format your response as:
STANCE: [ACCEPT/REJECT/MODIFY]
PROPOSAL:
<if MODIFY, provide your modified version; if ACCEPT, repeat the proposal; if REJECT, explain in proposal field>

REASONING:
<your detailed reasoning>"#,
        file,
        if spec_state.content.is_empty() {
            "(empty)".to_string()
        } else {
            spec_state.content.clone()
        },
        latest_proposal,
        format_session_history(&session),
        format_lessons(&lessons),
    );

    println!("Querying LLM for response...");
    let response = provider.complete(&prompt)?;

    let (proposal_content, reasoning) = parse_proposal_response(&response);

    let proposal = SemanticProposal {
        content: proposal_content.clone(),
        spec_hash: Some(simple_hash(&proposal_content)),
    };

    let msg = Message::new(
        agent_id.clone(),
        MessageType::Respond,
        Some(proposal),
        reasoning.clone(),
        session.session_id.clone(),
    );

    println!("\n=== RESPONSE by {} ===", agent_id);
    println!("{}", proposal_content);
    println!("\n=== REASONING ===");
    println!("{}", reasoning);

    session.add_message(msg);
    crate::session::save_session(&session)?;

    println!("\nResponse recorded in session: {}", session.session_id);
    Ok(())
}

/// `spec concede <file>` — agent updates or withdraws their position
pub fn concede(
    file: &str,
    provider: &dyn LlmProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    let agent_id = get_agent_id()?;
    println!("Agent [{}] conceding on: {}", agent_id, file);

    let spec_state = read_spec(file)?;
    let mut session = crate::session::load_or_create_session(file)?;

    if session.messages.is_empty() {
        return Err("No session to concede in. Run 'spec propose' first.".into());
    }

    let lessons = memory::get_relevant_lessons(file)?;

    let prompt = format!(
        r#"You are an AI agent reconsidering your position in a spec discussion.

Spec file: {}

Current agreed spec content:
{}

Full session history:
{}

Relevant lessons:
{}

Your task: Review the discussion and update your position. You should:
- Acknowledge valid points made by other agents
- Update your proposal if the other agent's points have merit
- Withdraw your objections if the current proposal is actually correct

Format your response as:
CONCESSION:
<describe what you are conceding and why>

UPDATED PROPOSAL:
<your updated spec content, or "WITHDRAW" if you fully accept the latest proposal>

REASONING:
<your reasoning for the concession>"#,
        file,
        if spec_state.content.is_empty() {
            "(empty)".to_string()
        } else {
            spec_state.content.clone()
        },
        format_session_history(&session),
        format_lessons(&lessons),
    );

    println!("Querying LLM for concession...");
    let response = provider.complete(&prompt)?;

    let (proposal_content, reasoning) = parse_proposal_response(&response);

    let proposal = SemanticProposal {
        content: proposal_content.clone(),
        spec_hash: Some(simple_hash(&proposal_content)),
    };

    let msg = Message::new(
        agent_id.clone(),
        MessageType::Concede,
        Some(proposal),
        reasoning.clone(),
        session.session_id.clone(),
    );

    println!("\n=== CONCESSION by {} ===", agent_id);
    println!("{}", proposal_content);
    println!("\n=== REASONING ===");
    println!("{}", reasoning);

    session.add_message(msg);
    crate::session::save_session(&session)?;

    println!("\nConcession recorded in session: {}", session.session_id);
    Ok(())
}

/// `spec agree <file>` — agent signs off on the current spec state
pub fn agree(
    file: &str,
    provider: &dyn LlmProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    let agent_id = get_agent_id()?;
    println!("Agent [{}] agreeing on: {}", agent_id, file);

    let spec_state = read_spec(file)?;
    let mut session = crate::session::load_or_create_session(file)?;

    if session.locked {
        return Err("Session is already locked. Consensus has been reached.".into());
    }

    // Find the latest proposal to agree on
    let latest_proposal = session
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.message_type, MessageType::Propose | MessageType::Respond | MessageType::Concede))
        .and_then(|m| m.proposal.as_ref())
        .map(|p| p.content.clone());

    let prompt = format!(
        r#"You are an AI agent deciding whether to sign off on the current spec proposal.

Spec file: {}

Session history:
{}

Latest proposed content:
{}

Your task: Confirm your agreement with the current state of the spec.
Provide your reasoning for agreeing, noting what makes this spec correct and complete.

Format your response as:
AGREEMENT: YES
REASONING:
<your reasoning for agreeing>"#,
        file,
        format_session_history(&session),
        latest_proposal
            .as_deref()
            .unwrap_or(&spec_state.content)
    );

    println!("Querying LLM for agreement confirmation...");
    let response = provider.complete(&prompt)?;

    let reasoning = extract_reasoning(&response);

    // Use the latest proposal as the agreed content
    let agreed_content = latest_proposal.unwrap_or_else(|| spec_state.content.clone());

    let proposal = SemanticProposal {
        content: agreed_content.clone(),
        spec_hash: Some(simple_hash(&agreed_content)),
    };

    let msg = Message::new(
        agent_id.clone(),
        MessageType::Agree,
        Some(proposal),
        reasoning.clone(),
        session.session_id.clone(),
    );

    println!("\n=== AGREEMENT by {} ===", agent_id);
    println!("\n=== REASONING ===");
    println!("{}", reasoning);

    session.add_message(msg);

    // Check if all agents have agreed
    if session.all_agents_agreed() {
        session.lock();
        println!("\n*** CONSENSUS REACHED — Session locked ***");

        // Write the new spec state
        let new_state = SpecState::new(
            agreed_content,
            session.session_id.clone(),
            session.agents_involved(),
        );
        write_spec(file, &new_state)?;
        println!("Spec state updated and locked: {}", file);

        run_hook("post-agree", &HookContext {
            spec_file: file.to_string(),
            session_id: Some(session.session_id.clone()),
            env_target: None,
        })?;
    } else {
        let agreed = session.agreed_agents.len();
        let total = session.agents_involved().len();
        if session.participating_agent_count() < 2 {
            println!("\nAgreement recorded. Waiting for at least one other agent to participate before consensus can lock.");
            println!("Have another agent run: SPEC_AGENT_ID=<other> spec respond {}", file);
        } else {
            println!("\nAgreement recorded ({}/{} agents agreed)", agreed, total);
        }
    }

    crate::session::save_session(&session)?;
    Ok(())
}

fn parse_proposal_response(response: &str) -> (String, String) {
    let proposal_content = extract_section(response, "PROPOSAL:")
        .or_else(|| extract_section(response, "UPDATED PROPOSAL:"))
        .unwrap_or_else(|| response.to_string());

    let reasoning = extract_reasoning(response);

    (proposal_content.trim().to_string(), reasoning)
}

fn extract_section(text: &str, header: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let header_lower = header.to_lowercase();
    if let Some(start_idx) = lower.find(&header_lower) {
        let after_header = &text[start_idx + header.len()..];
        // Find the next section header
        let end_idx = find_next_section(after_header);
        let section = &after_header[..end_idx];
        return Some(section.trim().to_string());
    }
    None
}

fn extract_reasoning(text: &str) -> String {
    extract_section(text, "REASONING:")
        .unwrap_or_else(|| text.to_string())
        .trim()
        .to_string()
}

fn find_next_section(text: &str) -> usize {
    let section_markers = ["PROPOSAL:", "REASONING:", "STANCE:", "CONCESSION:", "UPDATED PROPOSAL:", "AGREEMENT:"];
    let mut min_idx = text.len();
    let lower = text.to_lowercase();
    for marker in &section_markers {
        let marker_lower = marker.to_lowercase();
        if let Some(idx) = lower.find(&marker_lower) {
            if idx < min_idx && idx > 0 {
                min_idx = idx;
            }
        }
    }
    min_idx
}

fn simple_hash(s: &str) -> String {
    let mut hash: u64 = 5381;
    for c in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(c as u64);
    }
    format!("{:x}", hash)
}
