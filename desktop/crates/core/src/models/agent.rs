use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversation {
    pub id: i64,
    pub conversation_id: String,
    pub user_id: Option<i64>,
    pub title: Option<String>,
    pub project_id: Option<i64>,
    pub context_type: Option<String>,
    pub agent_type: Option<String>,
    pub category: Option<String>,
    pub context_id: Option<i64>,
    pub message_count: i64,
    pub last_message_time: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    pub id: i64,
    pub conversation_id: String,
    pub role: String,
    pub content: Option<String>,
    pub message_order: Option<i64>,
    pub run_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_status: Option<String>,
    pub tool_call_id: Option<String>,
    pub parent_tool_call_id: Option<String>,
    pub reasoning_content: Option<String>,
    pub reasoning_duration_ms: Option<i64>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: i64,
    pub name: String,
    pub transport: Option<String>,
    pub command: Option<String>,
    pub args: Option<String>,
    pub url: Option<String>,
    pub headers: Option<String>,
    pub enabled: bool,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct McpServerSaveReq {
    pub id: Option<i64>,
    pub name: String,
    pub transport: Option<String>,
    pub command: Option<String>,
    pub args: Option<serde_json::Value>,
    pub url: Option<String>,
    pub headers: Option<serde_json::Value>,
    pub enabled: Option<bool>,
}

impl Default for McpServerSaveReq {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            transport: None,
            command: None,
            args: None,
            url: None,
            headers: None,
            enabled: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSkill {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub description: String,
    pub content: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantReferenceOptions {
    pub projects: Vec<ReferenceOption>,
    pub scripts: Vec<ReferenceOption>,
    pub assets: Vec<ReferenceOption>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceOption {
    pub id: i64,
    pub name: String,
    pub extra: Option<String>,
}

/// Agent 运行态事件，SSE 推送体。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineEvent {
    pub event_type: String,
    pub run_id: String,
    pub conversation_id: Option<String>,
    pub content: Option<String>,
    pub tool_name: Option<String>,
    pub tool_args: Option<serde_json::Value>,
    pub tool_result: Option<String>,
    pub status: Option<String>,
    pub error: Option<String>,
}
