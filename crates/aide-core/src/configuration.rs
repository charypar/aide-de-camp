use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ConfigAction {
    List,
    Read { path: String },
    Open { path: Option<String>, line: u32 },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConfigListing {
    pub root: String,
    pub exists: bool,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConfigDocument {
    pub path: String,
    pub absolute_path: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigData {
    Listing(ConfigListing),
    Document(ConfigDocument),
    Opened,
}
