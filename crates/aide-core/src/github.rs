use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrId {
    pub host: String,
    pub owner: String,
    pub repository: String,
    pub number: u64,
}
impl PrId {
    pub fn url(&self) -> String {
        format!(
            "https://{}/{}/{}/pull/{}",
            self.host, self.owner, self.repository, self.number
        )
    }
    pub fn label(&self) -> String {
        format!("{}/{} #{}", self.owner, self.repository, self.number)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrRole {
    Implementing,
    Reviewing,
}
impl PrRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Implementing => "Implementing",
            Self::Reviewing => "Reviewing",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrStatus {
    WaitingReview,
    Ready,
    Running,
    NeedsAttention,
    Draft,
    Queued,
    ReviewRequested,
    ReviewRequestedAgain,
    Waiting,
    Merged,
    Closed,
    Unknown,
}
impl PrStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::WaitingReview => "Waiting for review",
            Self::Ready => "Ready to merge",
            Self::Running => "CI running",
            Self::NeedsAttention => "Needs attention",
            Self::Draft => "Draft",
            Self::Queued => "In merge queue",
            Self::ReviewRequested => "Review requested",
            Self::ReviewRequestedAgain => "Review requested again",
            Self::Waiting => "Waiting",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
            Self::Unknown => "Unknown",
        }
    }
    pub fn actionable(self) -> bool {
        matches!(
            self,
            Self::Ready | Self::NeedsAttention | Self::ReviewRequested | Self::ReviewRequestedAgain
        )
    }
    pub fn color(self) -> &'static str {
        match self {
            Self::Ready => "green",
            Self::NeedsAttention | Self::ReviewRequested | Self::ReviewRequestedAgain => "yellow",
            Self::Merged => "purple",
            Self::Running => "amber",
            _ => "grey",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PrCheck {
    pub name: String,
    pub result: String,
    pub url: Option<String>,
    pub required: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrReview {
    Approved,
    ChangesRequested,
    Required,
    NotRequired,
}
impl PrReview {
    pub fn label(self) -> &'static str {
        match self {
            Self::Approved => "Approved",
            Self::ChangesRequested => "Changes requested",
            Self::Required => "Review required",
            Self::NotRequired => "No required review",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PrObservation {
    pub title: String,
    pub author: String,
    pub viewer: String,
    pub viewer_id: String,
    pub role: PrRole,
    pub status: PrStatus,
    pub reasons: Vec<String>,
    pub checks: Vec<PrCheck>,
    #[serde(default)]
    pub review: Option<PrReview>,
    pub head: Option<String>,
    pub head_sha: Option<String>,
    pub base: Option<String>,
    pub ci_running: bool,
    pub observed_at: u64,
}
impl PrObservation {
    pub fn interval(&self) -> u64 {
        if self.role == PrRole::Implementing
            && self.ci_running
            && !matches!(self.status, PrStatus::Merged | PrStatus::Closed)
        {
            60
        } else {
            300
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PrView {
    pub id: PrId,
    pub observation: Option<PrObservation>,
    pub last_attempt: u64,
    pub error: Option<String>,
    #[serde(default)]
    pub stale: bool,
    #[serde(default)]
    pub historical: bool,
}
impl PrView {
    pub fn label(&self) -> String {
        let status = self
            .observation
            .as_ref()
            .map(|o| o.status.label())
            .unwrap_or("Unavailable");
        if self.historical {
            format!("{status} · Historical")
        } else if self.stale && self.observation.is_some() {
            format!("{status} · Stale")
        } else {
            status.into()
        }
    }
    pub fn color(&self) -> &'static str {
        if self.stale || self.historical {
            "grey"
        } else {
            self.observation
                .as_ref()
                .map(|o| o.status.color())
                .unwrap_or("grey")
        }
    }
}
