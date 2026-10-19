use crate::{Reference, Review, UiState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub id: String,
    pub review_id: String,
    pub command: Command,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    SaveUi {
        ui: Box<UiState>,
    },
    Chat {
        text: String,
        reference: Option<Reference>,
    },
    Feedback {
        id: String,
        text: String,
        reference: Option<Reference>,
    },
    DeleteFeedback {
        id: String,
    },
    Generate,
    Interrupt,
    Respond {
        request_id: serde_json::Value,
        response: serde_json::Value,
    },
    Conclude,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    Snapshot { review: Box<Review> },
    Ack { id: String },
    Error { id: String, message: String },
}
