use serde::{Deserialize, Serialize};

pub type Environment = std::collections::BTreeMap<String, String>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Preparing,
    Open,
    Closing,
    Completed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Starting,
    Closing,
    Working,
    Waiting,
    Parked,
    Attention,
    Completed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalActivity {
    Idle,
    Working,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Repository {
    pub name: String,
    pub checkout: String,
    pub tasks_dir: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: String,
    pub key: String,
    pub repository: String,
    pub workspace: String,
    pub lifecycle: Lifecycle,
    pub desired_running: bool,
    pub thread_id: Option<String>,
    pub native_path: Option<String>,
    pub recording: Option<String>,
    pub recovered: bool,
    pub error: Option<String>,
    pub created: u64,
}

impl Task {
    pub fn session(&self) -> String {
        format!("aide-{}", self.key)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TaskView {
    pub task: Task,
    pub status: Status,
    pub reason: String,
    pub terminal_alive: bool,
    pub backend_alive: bool,
    pub summary: Option<String>,
    #[serde(default)]
    pub terminal_activity: Option<TerminalActivity>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProjectView {
    pub root: String,
    pub name: String,
    pub repositories: Vec<Repository>,
    pub tasks: Vec<TaskView>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Command {
    Start {
        project: String,
        id: String,
        repository: Option<String>,
        #[serde(default)]
        open_terminal: bool,
    },
    Attach {
        project: String,
        id: String,
    },
    SetParked {
        project: String,
        id: String,
        parked: bool,
    },
    Recover {
        project: String,
        id: String,
        #[serde(default)]
        environment: Option<Environment>,
    },
    Cleanup {
        project: String,
        id: String,
        confirmed: bool,
    },
    Editor {
        project: String,
        id: String,
    },
    Review {
        project: String,
        id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Snapshot { projects: Vec<ProjectView> },
    Result { message: String, error: bool },
}
