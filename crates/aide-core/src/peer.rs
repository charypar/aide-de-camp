use crate::{Command, ProjectView, TaskView};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Identity {
    pub version: u32,
    pub instance: String,
    pub name: String,
    pub desktop: bool,
    pub home: String,
    pub executable: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Source {
    pub instance: String,
    pub name: String,
    pub root: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct HostView {
    pub identity: Identity,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    pub request_id: String,
    pub instance: String,
    pub operation: Operation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Operation {
    List,
    Get {
        project: String,
        id: String,
        key: String,
    },
    EditorWorkspace {
        project: String,
        id: String,
        key: String,
        unpark: bool,
    },
    Execute {
        command: Command,
        key: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub request_id: String,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    Projects { projects: Vec<ProjectView> },
    Task { task: Box<TaskView> },
    Workspace { path: String },
    Success { message: String },
    Error { message: String },
}

impl Command {
    pub fn project_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::Configuration { .. } => None,
            Self::Start { project, .. }
            | Self::Attach { project, .. }
            | Self::Recover { project, .. }
            | Self::SetParked { project, .. }
            | Self::Cleanup { project, .. }
            | Self::Editor { project, .. }
            | Self::Review { project, .. }
            | Self::LinkPr { project, .. }
            | Self::UnlinkPr { project, .. }
            | Self::RefreshPrs { project, .. } => Some(project),
        }
    }
    pub fn target(&self) -> Option<(&str, &str)> {
        match self {
            Self::Configuration { .. } => None,
            Self::Start { project, id, .. }
            | Self::Attach { project, id }
            | Self::Recover { project, id, .. }
            | Self::SetParked { project, id, .. }
            | Self::Cleanup { project, id, .. }
            | Self::Editor { project, id }
            | Self::Review { project, id }
            | Self::LinkPr { project, id, .. }
            | Self::UnlinkPr { project, id, .. }
            | Self::RefreshPrs { project, id, .. } => Some((project, id)),
        }
    }
}
