use serde::{Deserialize, Serialize};

/// Raw struct deserilized from yaml
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct ReportNode {
    pub header: String,
    pub types: Option<Vec<String>>,
    pub names: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub breakdown: Option<Vec<ReportNode>>,
}
