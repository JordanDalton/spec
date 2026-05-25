use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MessageType {
    Propose,
    Respond,
    Concede,
    Agree,
    Clarify,
    Reframe,
    Build,
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageType::Propose => write!(f, "PROPOSE"),
            MessageType::Respond => write!(f, "RESPOND"),
            MessageType::Concede => write!(f, "CONCEDE"),
            MessageType::Agree => write!(f, "AGREE"),
            MessageType::Clarify => write!(f, "CLARIFY"),
            MessageType::Reframe => write!(f, "REFRAME"),
            MessageType::Build => write!(f, "BUILD"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticProposal {
    pub content: String,
    pub spec_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub agent_id: String,
    pub message_type: MessageType,
    pub proposal: Option<SemanticProposal>,
    pub reasoning: String,
    pub timestamp: u64,
    pub session_id: String,
}

impl Message {
    pub fn new(
        agent_id: String,
        message_type: MessageType,
        proposal: Option<SemanticProposal>,
        reasoning: String,
        session_id: String,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Message {
            agent_id,
            message_type,
            proposal,
            reasoning,
            timestamp,
            session_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: String,
    pub spec_file: String,
    pub messages: Vec<Message>,
    pub locked: bool,
    pub agreed_agents: Vec<String>,
    pub created_at: u64,
}

impl Session {
    pub fn new(spec_file: &str) -> Self {
        let session_id = generate_session_id();
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Session {
            session_id,
            spec_file: spec_file.to_string(),
            messages: Vec::new(),
            locked: false,
            agreed_agents: Vec::new(),
            created_at,
        }
    }

    pub fn add_message(&mut self, msg: Message) {
        if msg.message_type == MessageType::Agree {
            if !self.agreed_agents.contains(&msg.agent_id) {
                self.agreed_agents.push(msg.agent_id.clone());
            }
        }
        self.messages.push(msg);
    }

    pub fn all_agents_agreed(&self) -> bool {
        let mut participating: std::collections::HashSet<String> = std::collections::HashSet::new();
        for msg in &self.messages {
            match msg.message_type {
                MessageType::Propose | MessageType::Respond | MessageType::Concede | MessageType::Agree => {
                    participating.insert(msg.agent_id.clone());
                }
                _ => {}
            }
        }
        // Consensus requires at least 2 distinct agents
        if participating.len() < 2 {
            return false;
        }
        for agent in &participating {
            if !self.agreed_agents.contains(agent) {
                return false;
            }
        }
        true
    }

    pub fn participating_agent_count(&self) -> usize {
        let mut participating: std::collections::HashSet<String> = std::collections::HashSet::new();
        for msg in &self.messages {
            match msg.message_type {
                MessageType::Propose | MessageType::Respond | MessageType::Concede | MessageType::Agree => {
                    participating.insert(msg.agent_id.clone());
                }
                _ => {}
            }
        }
        participating.len()
    }

    pub fn lock(&mut self) {
        self.locked = true;
    }

    pub fn agents_involved(&self) -> Vec<String> {
        let mut agents: Vec<String> = Vec::new();
        for msg in &self.messages {
            if !agents.contains(&msg.agent_id) {
                agents.push(msg.agent_id.clone());
            }
        }
        agents
    }
}

fn generate_session_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("sess_{:x}", ts)
}

/// Returns the path where a session JSON is stored for a given spec file
pub fn session_path_for(spec_file: &str) -> PathBuf {
    // spec_file could be something like "App/Http/SomeController.spec"
    // Strip leading "./" if present
    let clean = spec_file.trim_start_matches("./");
    let session_file = format!("{}.json", clean);
    Path::new(".spec").join("sessions").join(session_file)
}

pub fn load_session(spec_file: &str) -> Result<Option<Session>, Box<dyn std::error::Error>> {
    let path = session_path_for(spec_file);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let session: Session = serde_json::from_str(&content)?;
    Ok(Some(session))
}

pub fn save_session(session: &Session) -> Result<(), Box<dyn std::error::Error>> {
    let path = session_path_for(&session.spec_file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(session)?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn load_or_create_session(spec_file: &str) -> Result<Session, Box<dyn std::error::Error>> {
    match load_session(spec_file)? {
        Some(s) => Ok(s),
        None => Ok(Session::new(spec_file)),
    }
}

/// Scan all session files and return a summary
pub fn all_sessions() -> Result<Vec<Session>, Box<dyn std::error::Error>> {
    let sessions_dir = Path::new(".spec").join("sessions");
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }
    let mut results = Vec::new();
    collect_sessions(&sessions_dir, &mut results)?;
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(agent_id: &str, msg_type: MessageType) -> Message {
        Message::new(agent_id.to_string(), msg_type, None, "reasoning".to_string(), "sess_test".to_string())
    }

    #[test]
    fn single_agent_cannot_reach_consensus() {
        let mut session = Session::new("test.spec");
        session.add_message(make_msg("alice", MessageType::Propose));
        session.add_message(make_msg("alice", MessageType::Agree));
        assert!(!session.all_agents_agreed());
    }

    #[test]
    fn two_agents_both_agreed_locks() {
        let mut session = Session::new("test.spec");
        session.add_message(make_msg("alice", MessageType::Propose));
        session.add_message(make_msg("bob", MessageType::Respond));
        session.add_message(make_msg("alice", MessageType::Agree));
        session.add_message(make_msg("bob", MessageType::Agree));
        assert!(session.all_agents_agreed());
    }

    #[test]
    fn two_agents_one_agreed_not_locked() {
        let mut session = Session::new("test.spec");
        session.add_message(make_msg("alice", MessageType::Propose));
        session.add_message(make_msg("bob", MessageType::Respond));
        session.add_message(make_msg("alice", MessageType::Agree));
        assert!(!session.all_agents_agreed());
    }

    #[test]
    fn participating_count_is_distinct() {
        let mut session = Session::new("test.spec");
        session.add_message(make_msg("alice", MessageType::Propose));
        session.add_message(make_msg("alice", MessageType::Concede));
        session.add_message(make_msg("bob", MessageType::Respond));
        assert_eq!(session.participating_agent_count(), 2);
    }

    #[test]
    fn agreed_agents_deduped() {
        let mut session = Session::new("test.spec");
        session.add_message(make_msg("alice", MessageType::Agree));
        session.add_message(make_msg("alice", MessageType::Agree));
        assert_eq!(session.agreed_agents.len(), 1);
    }

    #[test]
    fn agents_involved_deduped() {
        let mut session = Session::new("test.spec");
        session.add_message(make_msg("alice", MessageType::Propose));
        session.add_message(make_msg("alice", MessageType::Agree));
        session.add_message(make_msg("bob", MessageType::Respond));
        let agents = session.agents_involved();
        assert_eq!(agents.len(), 2);
    }

    #[test]
    fn empty_session_cannot_agree() {
        let session = Session::new("test.spec");
        assert!(!session.all_agents_agreed());
    }

    #[test]
    fn lock_sets_locked_flag() {
        let mut session = Session::new("test.spec");
        assert!(!session.locked);
        session.lock();
        assert!(session.locked);
    }
}

fn collect_sessions(dir: &Path, results: &mut Vec<Session>) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_sessions(&path, results)?;
        } else if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(session) = serde_json::from_str::<Session>(&content) {
                    results.push(session);
                }
            }
        }
    }
    Ok(())
}
