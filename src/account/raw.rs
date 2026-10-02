use serde::{Deserialize, Serialize};

/// Raw struct deserilized from yaml
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    pub description: Option<String>,
    pub class: String,
    pub num: Option<usize>,
    pub tags: Option<Vec<String>>,
}
