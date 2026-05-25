use crate::llm::LlmProvider;
use crate::session::{Message, MessageType, SemanticProposal, Session};
use crate::spec::read_spec;

fn get_mediator_id() -> String {
    "mediator".to_string()
}

fn format_session_history(session: &Session) -> String {
    if session.messages.is_empty() {
        return "No messages in this session.".to_string();
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

/// `spec clarify <file>` — mediator surfaces a contradiction
pub fn clarify(
    file: &str,
    provider: &dyn LlmProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    let mediator_id = get_mediator_id();
    println!("Mediator [{}] clarifying: {}", mediator_id, file);

    let spec_state = read_spec(file)?;
    let mut session = crate::session::load_or_create_session(file)?;

    if session.messages.is_empty() {
        return Err("No session messages to clarify. Run 'spec propose' and 'spec respond' first.".into());
    }

    let prompt = format!(
        r#"You are a neutral mediator in a spec-driven development process.
The mediator NEVER proposes changes or agrees to proposals — only surfaces contradictions and ambiguities.

Spec file: {}

Current spec content:
{}

Full session history:
{}

Your task: Identify and surface any contradictions, ambiguities, or unresolved disagreements in the session.

For each contradiction found, describe:
1. What the contradiction is
2. Which agents hold conflicting positions
3. The specific points that need resolution

Format your response as:
CONTRADICTIONS FOUND:
<numbered list of contradictions, or "None found" if the discussion is coherent>

CLARIFICATION QUESTIONS:
<specific questions that agents must answer to resolve the contradictions>

REASONING:
<why these are genuine contradictions and not just different phrasings>"#,
        file,
        if spec_state.content.is_empty() {
            "(empty)".to_string()
        } else {
            spec_state.content.clone()
        },
        format_session_history(&session),
    );

    println!("Querying LLM for clarification...");
    let response = provider.complete(&prompt)?;

    // Extract the clarification content
    let clarification_content = response.trim().to_string();
    let reasoning = extract_reasoning(&response);

    let proposal = SemanticProposal {
        content: clarification_content.clone(),
        spec_hash: None, // Mediator doesn't propose spec content
    };

    let msg = Message::new(
        mediator_id.clone(),
        MessageType::Clarify,
        Some(proposal),
        reasoning,
        session.session_id.clone(),
    );

    println!("\n=== CLARIFICATION by {} ===", mediator_id);
    println!("{}", clarification_content);

    session.add_message(msg);
    crate::session::save_session(&session)?;

    println!("\nClarification recorded in session: {}", session.session_id);
    Ok(())
}

/// `spec reframe <file>` — mediator reframes the disagreement
pub fn reframe(
    file: &str,
    provider: &dyn LlmProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    let mediator_id = get_mediator_id();
    println!("Mediator [{}] reframing: {}", mediator_id, file);

    let spec_state = read_spec(file)?;
    let mut session = crate::session::load_or_create_session(file)?;

    if session.messages.is_empty() {
        return Err("No session messages to reframe. Run 'spec propose' and 'spec respond' first.".into());
    }

    let prompt = format!(
        r#"You are a neutral mediator in a spec-driven development process.
The mediator NEVER proposes changes or agrees to proposals — only reframes disagreements to help agents find common ground.

Spec file: {}

Current spec content:
{}

Full session history:
{}

Your task: Reframe the current disagreement to help agents find common ground.

A good reframe:
1. Acknowledges what both parties agree on
2. Identifies the core underlying concern behind each position
3. Suggests a new way of looking at the problem that might bridge the gap
4. Does NOT propose a specific spec change — that's for the agents

Format your response as:
COMMON GROUND:
<what agents actually agree on>

CORE CONCERNS:
<the underlying concern of each agent's position>

REFRAME:
<a new perspective on the problem that might help agents find consensus>

REASONING:
<why this reframe might help resolve the disagreement>"#,
        file,
        if spec_state.content.is_empty() {
            "(empty)".to_string()
        } else {
            spec_state.content.clone()
        },
        format_session_history(&session),
    );

    println!("Querying LLM for reframe...");
    let response = provider.complete(&prompt)?;

    let reframe_content = response.trim().to_string();
    let reasoning = extract_reasoning(&response);

    let proposal = SemanticProposal {
        content: reframe_content.clone(),
        spec_hash: None, // Mediator doesn't propose spec content
    };

    let msg = Message::new(
        mediator_id.clone(),
        MessageType::Reframe,
        Some(proposal),
        reasoning,
        session.session_id.clone(),
    );

    println!("\n=== REFRAME by {} ===", mediator_id);
    println!("{}", reframe_content);

    session.add_message(msg);
    crate::session::save_session(&session)?;

    println!("\nReframe recorded in session: {}", session.session_id);
    Ok(())
}

fn extract_reasoning(text: &str) -> String {
    let lower = text.to_lowercase();
    if let Some(start_idx) = lower.find("reasoning:") {
        let after = &text[start_idx + "reasoning:".len()..];
        return after.trim().to_string();
    }
    text.to_string()
}
