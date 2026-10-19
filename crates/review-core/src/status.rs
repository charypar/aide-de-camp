use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "completed")]
    Completed,
}

impl ReviewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
        }
    }
}

impl std::fmt::Display for ReviewStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageRole {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageStatus {
    #[serde(rename = "complete", alias = "completed")]
    Complete,
    #[serde(rename = "streaming")]
    Streaming,
    #[serde(rename = "interrupted")]
    Interrupted,
    #[serde(rename = "failed")]
    Failed,
}

impl MessageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Streaming => "streaming",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }
}

impl std::fmt::Display for MessageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum DiffSide {
    #[serde(rename = "old")]
    Old,
    #[default]
    #[serde(rename = "new", alias = "")]
    New,
}

impl DiffSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Old => "old",
            Self::New => "new",
        }
    }
}

impl std::fmt::Display for DiffSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for DiffSide {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "old" => Ok(Self::Old),
            "new" => Ok(Self::New),
            _ => Err("Choose old or new lines"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persisted_values_remain_compatible_and_reject_unknown_states() {
        assert_eq!(
            serde_json::to_string(&ReviewStatus::Completed).unwrap(),
            "\"completed\""
        );
        assert_eq!(
            serde_json::from_str::<MessageStatus>("\"completed\"").unwrap(),
            MessageStatus::Complete
        );
        assert_eq!(
            serde_json::to_string(&MessageStatus::Complete).unwrap(),
            "\"complete\""
        );
        assert!(serde_json::from_str::<ReviewStatus>("\"invalid\"").is_err());
        assert!(serde_json::from_str::<DiffSide>("\"invalid\"").is_err());
        assert_eq!(
            serde_json::from_str::<DiffSide>("\"\"").unwrap(),
            DiffSide::New
        );
    }
}
