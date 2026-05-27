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
            let knowledge_text = m
                .knowledge
                .as_ref()
                .map(|k| format!("\n  Knowledge: {}", k))
                .unwrap_or_default();
            let context_text = m
                .context
                .as_ref()
                .map(|c| format!("\n  Context: {}", c))
                .unwrap_or_default();
            format!(
                "[{}] {} | {}{}{}{}\n  Reasoning: {}",
                m.timestamp, m.agent_id, m.message_type, knowledge_text, context_text, proposal_text, m.reasoning
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `spec propose <file> "<intent>"` — agent proposes a spec change
pub fn propose(
    file: &str,
    intent: &str,
    knowledge: Option<&str>,
    provider: &dyn LlmProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    let agent_id = get_agent_id()?;
    println!("Agent [{}] proposing for: {}", agent_id, file);

    // Read current spec state
    let spec_state = read_spec(file)?;

    // Read the actual source file if it exists
    let source_content = std::fs::read_to_string(file).ok();

    // Load session snapshot for prompt building.
    // If the session is locked, a new change cycle begins — use a clean snapshot.
    let session = {
        let s = crate::session::load_or_create_session(file)?;
        if s.locked {
            println!("Previous session is locked (consensus reached). Starting a new session.");
            crate::session::Session::new(file)
        } else {
            s
        }
    };

    // After a mediator intervention (clarify/reframe), agents must respond or concede —
    // not add more proposals. New proposals are allowed only after the session locks.
    if let Some(med_idx) = session.messages.iter()
        .rposition(|m| matches!(m.message_type, MessageType::Clarify | MessageType::Reframe))
    {
        let med_type = &session.messages[med_idx].message_type;
        return Err(format!(
            "A mediator has run '{}' on this session. Use 'spec respond' or 'spec concede' \
             to engage with the existing proposals — new proposals are blocked until consensus is reached.",
            med_type
        ).into());
    }

    // Prevent the same agent from proposing twice without a response from another agent
    let already_proposed = session.messages.iter().any(|m| {
        m.agent_id == agent_id && matches!(m.message_type, MessageType::Propose)
    });
    let other_responded = session.messages.iter().any(|m| {
        m.agent_id != agent_id && matches!(m.message_type, MessageType::Respond | MessageType::Concede)
    });
    if already_proposed && !other_responded {
        return Err(format!(
            "Agent '{}' has already proposed. Wait for another agent to respond before proposing again.",
            agent_id
        ).into());
    }

    // Query relevant lessons
    let lessons = memory::get_relevant_lessons(intent)?;

    // Build prompt
    let source_section = match &source_content {
        Some(src) => format!("Current source file ({}):\n{}", file, src),
        None => format!("Current source file ({}): (does not exist yet)", file),
    };

    let prompt = format!(
        r#"You are an AI agent participating in a spec-driven development process.

{}

Current spec file: {}

Current spec content:
{}

Session history:
{}

Relevant lessons from past sessions:
{}

User intent / change request:
{}

Your task: Propose a concrete change to the source file that fulfills the user's intent.
Apply ONLY the change described in the intent. Preserve everything else in the source file exactly as-is — existing comments, formatting, whitespace, unrelated methods, and structure must not be altered.

Your response must include:
1. KNOWLEDGE: {}
2. PROPOSAL: The complete updated file content (not a diff) with only the requested change applied
3. REASONING: Why this change is appropriate and how it fulfills the intent

Format your response as:
KNOWLEDGE:
{}

PROPOSAL:
<the full updated file content>

REASONING:
<your reasoning>"#,
        source_section,
        file,
        if spec_state.content.is_empty() {
            "(empty - new spec)".to_string()
        } else {
            spec_state.content.clone()
        },
        format_session_history(&session),
        format_lessons(&lessons),
        intent,
        if knowledge.is_some() {
            "The knowledge below was explicitly provided by the agent — use it verbatim, do not add to or alter it"
        } else {
            "The assumptions, constraints, and prior knowledge you are basing this proposal on — be explicit about what you know and what you are assuming"
        },
        knowledge.unwrap_or("<infer from the source file and intent>"),
    );

    println!("Querying LLM for proposal...");
    let response = provider.complete(&prompt)?;

    // Parse response
    let (proposal_content, reasoning) = parse_proposal_response(&response);
    let knowledge = extract_section(&response, "KNOWLEDGE:")
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());

    let proposal = SemanticProposal {
        content: proposal_content.clone(),
        spec_hash: Some(simple_hash(&proposal_content)),
    };

    if let Some(ref k) = knowledge {
        println!("\n=== KNOWLEDGE by {} ===", agent_id);
        println!("{}", k);
    }
    println!("\n=== PROPOSAL by {} ===", agent_id);
    println!("{}", proposal_content);
    println!("\n=== REASONING ===");
    println!("{}", reasoning);

    // When running interactively (human at a terminal), confirm before committing.
    // Automated agents pipe stdin so IsTerminal returns false — they auto-commit.
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        use std::io::Write;
        print!("\nCommit this proposal? [y/N] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => {}
            _ => {
                println!("Proposal discarded. Run 'spec propose' again to try a different intent.");
                return Ok(());
            }
        }
    }

    let session_id = crate::session::with_session_lock(file, |session| {
        // If the on-disk session is locked, archive it then reset — new change cycle.
        // Archiving preserves history and keeps the .spec file available for parallel builds.
        if session.locked {
            crate::session::archive_session(file)?;
            *session = crate::session::Session::new(file);
        }

        if let Some(med_idx) = session.messages.iter()
            .rposition(|m| matches!(m.message_type, MessageType::Clarify | MessageType::Reframe))
        {
            let med_type = &session.messages[med_idx].message_type;
            return Err(format!(
                "A mediator has run '{}' on this session. Use 'spec respond' or 'spec concede' \
                 to engage with the existing proposals — new proposals are blocked until consensus is reached.",
                med_type
            ).into());
        }

        let already_proposed = session.messages.iter().any(|m| {
            m.agent_id == agent_id && matches!(m.message_type, MessageType::Propose)
        });
        let other_responded = session.messages.iter().any(|m| {
            m.agent_id != agent_id && matches!(m.message_type, MessageType::Respond | MessageType::Concede)
        });
        if already_proposed && !other_responded {
            return Err(format!(
                "Agent '{}' has already proposed. Wait for another agent to respond before proposing again.",
                agent_id
            ).into());
        }

        // Build msg here so session_id matches the (possibly fresh) session.
        let mut fresh_msg = Message::new(
            agent_id.clone(),
            MessageType::Propose,
            Some(proposal),
            reasoning.clone(),
            session.session_id.clone(),
        );
        fresh_msg.knowledge = knowledge.clone();
        session.add_message(fresh_msg);
        Ok(session.session_id.clone())
    })?;

    println!("\nProposal recorded in session: {}", session_id);
    println!("\nNext steps:");
    println!("  Poll for a response:  spec state {}", file);
    println!("  When another agent responds, either:");
    println!("    spec agree {}     — sign off if you're satisfied", file);
    println!("    spec concede {}   — update your position", file);
    println!("  Or lock immediately:  spec agree {} --solo", file);
    println!("\nSTATUS: WAITING_FOR_REPLY");
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
    let session = crate::session::load_or_create_session(file)?;

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

Your task: Respond to the latest proposal. Before taking a stance, you may provide additional
context — new constraints, requirements, edge cases, or information the other agents should know.

Format your response as:
CONTEXT:
<any new information you want to add to the session — leave blank if none>

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
    let context = extract_section(&response, "CONTEXT:")
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty());

    let proposal = SemanticProposal {
        content: proposal_content.clone(),
        spec_hash: Some(simple_hash(&proposal_content)),
    };

    let mut msg = Message::new(
        agent_id.clone(),
        MessageType::Respond,
        Some(proposal),
        reasoning.clone(),
        session.session_id.clone(),
    );
    msg.context = context.clone();

    if let Some(ref ctx) = context {
        println!("\n=== CONTEXT from {} ===", agent_id);
        println!("{}", ctx);
    }
    println!("\n=== RESPONSE by {} ===", agent_id);
    println!("{}", proposal_content);
    println!("\n=== REASONING ===");
    println!("{}", reasoning);

    let session_id = crate::session::with_session_lock(file, |session| {
        session.add_message(msg);
        Ok(session.session_id.clone())
    })?;

    println!("\nResponse recorded in session: {}", session_id);
    println!("\nNext steps:");
    println!("  Check the discussion:  spec log {}", file);
    println!("  Sign off if satisfied: spec agree {}", file);
    println!("  Update your position:  spec concede {}", file);
    println!("\nSTATUS: WAITING_FOR_AGREE");
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
    let session = crate::session::load_or_create_session(file)?;

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

    let session_id = crate::session::with_session_lock(file, |session| {
        session.add_message(msg);
        Ok(session.session_id.clone())
    })?;

    println!("\nConcession recorded in session: {}", session_id);
    println!("\nNext steps:");
    println!("  Check the discussion:  spec log {}", file);
    println!("  Sign off if satisfied: spec agree {}", file);
    println!("  Respond if needed:     spec respond {}", file);
    println!("\nSTATUS: WAITING_FOR_AGREE");
    Ok(())
}

/// `spec agree <file> [--solo]` — agent signs off on the current spec state
pub fn agree(
    file: &str,
    solo: bool,
    provider: &dyn LlmProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    let agent_id = get_agent_id()?;
    println!("Agent [{}] agreeing on: {}{}", agent_id, file, if solo { " (solo)" } else { "" });

    let spec_state = read_spec(file)?;
    let session = crate::session::load_or_create_session(file)?;

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

    // agreed_content from the snapshot (used as fallback inside the lock too)
    let snapshot_agreed_content = latest_proposal.unwrap_or_else(|| spec_state.content.clone());

    let spec_state_content = spec_state.content.clone();

    println!("\n=== AGREEMENT by {} ===", agent_id);
    println!("\n=== REASONING ===");
    println!("{}", reasoning);

    // Acquire the lock to append and (if consensus) lock the session.
    // Returns (locked, agreed_content, session_id, agents, agreed_count, total_count, single_agent).
    type AgreeResult = (bool, String, String, Vec<String>, usize, usize, bool);
    let (did_lock, agreed_content, session_id, agents_involved, agreed_count, total_count, single_agent): AgreeResult =
        crate::session::with_session_lock(file, |session| {
            if session.locked {
                return Err("Session is already locked. Consensus has been reached.".into());
            }

            // Re-derive agreed content from the freshest session state
            let agreed_content = session
                .messages
                .iter()
                .rev()
                .find(|m| matches!(m.message_type, MessageType::Propose | MessageType::Respond | MessageType::Concede))
                .and_then(|m| m.proposal.as_ref())
                .map(|p| p.content.clone())
                .unwrap_or_else(|| spec_state_content.clone());

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

            session.add_message(msg);

            let should_lock = solo || session.all_agents_agreed();
            if should_lock {
                if solo { session.solo = true; }
                session.lock();
            }

            let agents = session.agents_involved();
            let agreed = session.agreed_agents.len();
            let total = agents.len();
            let single = session.participating_agent_count() < 2;

            Ok((should_lock, agreed_content, session.session_id.clone(), agents, agreed, total, single))
        })?;

    if did_lock {
        println!("\n*** {} — Session locked ***",
            if solo { "SOLO AGREEMENT" } else { "CONSENSUS REACHED" });

        let new_state = SpecState::new(agreed_content, session_id.clone(), agents_involved);
        write_spec(file, &new_state)?;
        println!("Spec state updated and locked: {}", file);

        run_hook("post-agree", &HookContext {
            spec_file: file.to_string(),
            session_id: Some(session_id),
            env_target: None,
        })?;
    } else if single_agent {
        println!("\nAgreement recorded. Waiting for at least one other agent to participate before consensus can lock.");
        println!("Have another agent run: SPEC_AGENT_ID=<other> spec respond {}", file);
        println!("Or lock immediately with: spec agree {} --solo", file);
        println!("\nSTATUS: WAITING_FOR_REPLY");
    } else {
        println!("\nAgreement recorded ({}/{} agents agreed)", agreed_count, total_count);
        println!("\nSTATUS: WAITING_FOR_AGREE");
    }

    let _ = snapshot_agreed_content;
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
    let section_markers = ["KNOWLEDGE:", "PROPOSAL:", "REASONING:", "STANCE:", "CONCESSION:", "UPDATED PROPOSAL:", "AGREEMENT:", "CONTEXT:"];
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
