use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub session_id: String,
    pub request_id: String,
    pub intervention: String,
    pub breakdown_point: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub description: String,
    pub provenance: Provenance,
    pub references: Vec<String>,
    pub reinforcements: Vec<String>,
}

impl Lesson {
    #[allow(dead_code)]
    pub fn new(
        description: String,
        session_id: String,
        request_id: String,
        intervention: String,
        breakdown_point: String,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let id = format!("lesson_{:x}", timestamp);
        Lesson {
            id,
            description,
            provenance: Provenance {
                session_id,
                request_id,
                intervention,
                breakdown_point,
                timestamp,
            },
            references: Vec::new(),
            reinforcements: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LessonGraph {
    pub lessons: Vec<Lesson>,
}

impl LessonGraph {
    #[allow(dead_code)]
    pub fn add_lesson(&mut self, lesson: Lesson) {
        self.lessons.push(lesson);
    }

    pub fn find_relevant(&self, query: &str) -> Vec<&Lesson> {
        let query_lower = query.to_lowercase();
        self.lessons
            .iter()
            .filter(|l| {
                l.description.to_lowercase().contains(&query_lower)
                    || l.references.iter().any(|r| r.to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn reinforce(&mut self, lesson_id: &str, evidence: String) {
        if let Some(lesson) = self.lessons.iter_mut().find(|l| l.id == lesson_id) {
            lesson.reinforcements.push(evidence);
        }
    }
}

fn lessons_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".spec").join("lessons.json")
}

pub fn load_lessons() -> Result<LessonGraph, Box<dyn std::error::Error>> {
    let path = lessons_path();
    if !path.exists() {
        return Ok(LessonGraph::default());
    }
    let content = std::fs::read_to_string(&path)?;
    let graph: LessonGraph = serde_json::from_str(&content)?;
    Ok(graph)
}

pub fn save_lessons(graph: &LessonGraph) -> Result<(), Box<dyn std::error::Error>> {
    let path = lessons_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(graph)?;
    std::fs::write(&path, content)?;
    Ok(())
}

#[allow(dead_code)]
pub fn add_lesson(
    description: String,
    session_id: String,
    request_id: String,
    intervention: String,
    breakdown_point: String,
) -> Result<Lesson, Box<dyn std::error::Error>> {
    let mut graph = load_lessons()?;
    let lesson = Lesson::new(description, session_id, request_id, intervention, breakdown_point);
    let lesson_clone = lesson.clone();
    graph.add_lesson(lesson);
    save_lessons(&graph)?;
    Ok(lesson_clone)
}

pub fn get_relevant_lessons(query: &str) -> Result<Vec<Lesson>, Box<dyn std::error::Error>> {
    let graph = load_lessons()?;
    Ok(graph.find_relevant(query).into_iter().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_lesson(id: &str, description: &str) -> Lesson {
        Lesson {
            id: id.to_string(),
            description: description.to_string(),
            provenance: Provenance {
                session_id: "sess_test".to_string(),
                request_id: "req_test".to_string(),
                intervention: "manual fix".to_string(),
                breakdown_point: "implementation".to_string(),
                timestamp: 0,
            },
            references: Vec::new(),
            reinforcements: Vec::new(),
        }
    }

    #[test]
    fn find_relevant_matches_description() {
        let mut graph = LessonGraph::default();
        graph.add_lesson(make_lesson("l1", "agents should not cache spec state"));
        graph.add_lesson(make_lesson("l2", "mediator must not propose"));
        let results = graph.find_relevant("cache");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "l1");
    }

    #[test]
    fn find_relevant_no_match_returns_empty() {
        let mut graph = LessonGraph::default();
        graph.add_lesson(make_lesson("l1", "agents should not cache spec state"));
        let results = graph.find_relevant("deployment");
        assert!(results.is_empty());
    }

    #[test]
    fn find_relevant_case_insensitive() {
        let mut graph = LessonGraph::default();
        graph.add_lesson(make_lesson("l1", "Agents should not Cache spec state"));
        let results = graph.find_relevant("cache");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn reinforce_adds_evidence() {
        let mut graph = LessonGraph::default();
        graph.add_lesson(make_lesson("l1", "test lesson"));
        graph.reinforce("l1", "seen again in session xyz".to_string());
        let lesson = graph.lessons.iter().find(|l| l.id == "l1").unwrap();
        assert_eq!(lesson.reinforcements.len(), 1);
    }
}
