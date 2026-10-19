use crate::{DiffSide, MessageRole, MessageStatus, Reference, ReviewStatus};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UiState {
    pub chat_draft: String,
    pub feedback_draft: String,
    pub editing_feedback: Option<String>,
    pub selection: Option<Reference>,
    pub code_anchor: Option<Reference>,
    pub prose_offset: Option<u32>,
    pub scroll: f64,
    pub chat_scroll: f64,
    pub feedback_scroll: f64,
    pub details: std::collections::BTreeMap<String, bool>,
    pub horizontal_scroll: std::collections::BTreeMap<String, i32>,
    pub tab: String,
    pub manual_file: String,
    pub manual_start: String,
    pub manual_end: String,
    pub manual_side: DiffSide,
    pub request_drafts: std::collections::BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Feedback {
    pub id: String,
    pub reference: Option<Reference>,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub text: String,
    pub reference: Option<Reference>,
    pub status: MessageStatus,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AgentRequest {
    pub id: serde_json::Value,
    pub method: String,
    pub params: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SourceFile {
    pub path: String,
    pub old: Option<String>,
    pub new: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub fingerprint: String,
    pub diff: String,
    pub files: Vec<SourceFile>,
    pub vcs: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Review {
    pub id: String,
    pub directory: String,
    pub workspace: Workspace,
    pub guide: String,
    pub guide_html: String,
    pub guide_warnings: Vec<String>,
    pub messages: Vec<Message>,
    pub feedback: Vec<Feedback>,
    pub ui: UiState,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub busy: bool,
    pub generating: bool,
    #[serde(default)]
    pub preparation_started_at_ms: Option<u64>,
    pub requests: Vec<AgentRequest>,
    pub activity: Vec<String>,
    pub status: ReviewStatus,
    pub error: Option<String>,
    pub export_path: Option<String>,
    pub notice: Option<String>,
}

impl Review {
    /// Wall time for this preparation attempt, including time waiting for a response.
    pub fn preparation_elapsed(&self, now_ms: u64) -> Option<String> {
        let seconds = now_ms.saturating_sub(self.preparation_started_at_ms?) / 1000;
        Some(format!("{}:{:02}", seconds / 60, seconds % 60))
    }
}
