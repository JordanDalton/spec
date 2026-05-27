use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Done,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "PENDING"),
            TaskStatus::InProgress => write!(f, "IN_PROGRESS"),
            TaskStatus::Done => write!(f, "DONE"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueTask {
    pub id: String,
    pub file: String,
    pub intent: String,
    pub status: TaskStatus,
    pub depends_on: Vec<String>,
    pub created_at: u64,
    pub assigned_agent: Option<String>,
    pub completed_at: Option<u64>,
}

impl QueueTask {
    pub fn new(file: String, intent: String, depends_on: Vec<String>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let id = format!("task_{:x}", now ^ (depends_on.len() as u64 * 0x9e3779b9));
        QueueTask {
            id,
            file,
            intent,
            status: TaskStatus::Pending,
            depends_on,
            created_at: now,
            assigned_agent: None,
            completed_at: None,
        }
    }

    pub fn is_unblocked(&self, tasks: &[QueueTask]) -> bool {
        self.depends_on.iter().all(|dep_id| {
            tasks.iter()
                .find(|t| &t.id == dep_id)
                .map(|t| t.status == TaskStatus::Done)
                .unwrap_or(true) // unknown dependency treated as satisfied
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskQueue {
    pub tasks: Vec<QueueTask>,
}

impl TaskQueue {
    pub fn add(&mut self, file: String, intent: String, depends_on: Vec<String>) -> String {
        let task = QueueTask::new(file, intent, depends_on);
        let id = task.id.clone();
        self.tasks.push(task);
        id
    }

    /// Sync task statuses with live session state.
    /// A task is auto-completed when its session is locked and a Build message exists.
    pub fn sync_with_sessions(&mut self) {
        for task in &mut self.tasks {
            if task.status == TaskStatus::Done {
                continue;
            }
            if let Ok(Some(session)) = crate::session::load_session(&task.file) {
                let built = session.messages.iter().any(|m| {
                    matches!(m.message_type, crate::session::MessageType::Build)
                });
                if session.locked && built {
                    task.status = TaskStatus::Done;
                    task.completed_at = Some(
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    );
                }
            }
        }
    }

    /// Return the next pending, unblocked task and claim it for agent_id.
    pub fn claim_next(&mut self, agent_id: &str) -> Option<QueueTask> {
        let tasks_snapshot = self.tasks.clone();
        for task in &mut self.tasks {
            if task.status == TaskStatus::Pending && task.is_unblocked(&tasks_snapshot) {
                task.status = TaskStatus::InProgress;
                task.assigned_agent = Some(agent_id.to_string());
                return Some(task.clone());
            }
        }
        None
    }

    pub fn mark_done(&mut self, id: &str) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        for task in &mut self.tasks {
            if task.id == id {
                task.status = TaskStatus::Done;
                task.completed_at = Some(now);
                return true;
            }
        }
        false
    }

    pub fn pending_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.status == TaskStatus::Pending).count()
    }

    pub fn blocked_count(&self) -> usize {
        self.tasks.iter().filter(|t| {
            t.status == TaskStatus::Pending && !t.is_unblocked(&self.tasks)
        }).count()
    }
}

fn queue_path() -> std::path::PathBuf {
    Path::new(".spec").join("queue.json")
}

pub fn load_queue() -> Result<TaskQueue, Box<dyn std::error::Error>> {
    let path = queue_path();
    if !path.exists() {
        return Ok(TaskQueue::default());
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn save_queue(queue: &TaskQueue) -> Result<(), Box<dyn std::error::Error>> {
    let path = queue_path();
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(queue)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}
