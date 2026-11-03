use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ComfyWorkflow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub workflow_json: Option<String>,
    pub status: i64,
    pub published_version_id: Option<i64>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ComfyWorkflowVersion {
    pub id: i64,
    pub workflow_id: i64,
    pub version: Option<String>,
    pub workflow_json: Option<String>,
    pub input_schema: Option<String>,
    pub status: i64,
    pub remark: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ComfyWorkflowSaveReq {
    pub id: Option<i64>,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub workflow_json: Option<String>,
    pub status: Option<i64>,
}

impl Default for ComfyWorkflowSaveReq {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            description: None,
            category: None,
            workflow_json: None,
            status: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ComfyVersionSaveReq {
    pub id: Option<i64>,
    pub workflow_id: i64,
    pub version: Option<String>,
    pub workflow_json: Option<String>,
    pub input_schema: Option<String>,
    pub status: Option<i64>,
    pub remark: Option<String>,
}

impl Default for ComfyVersionSaveReq {
    fn default() -> Self {
        Self {
            id: None,
            workflow_id: 0,
            version: None,
            workflow_json: None,
            input_schema: None,
            status: None,
            remark: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}
