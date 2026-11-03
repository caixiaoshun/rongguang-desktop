//! 对话与工具调用的通用数据结构（OpenAI Chat Completions 兼容）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(text: impl Into<String>) -> Self {
        Self::text("system", text)
    }
    pub fn user(text: impl Into<String>) -> Self {
        Self::text("user", text)
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::text("assistant", text)
    }
    pub fn text(role: &str, text: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: Some(serde_json::Value::String(text.into())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(serde_json::Value::String(content.into())),
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
    /// 纯文本内容提取，供非流式展示。
    pub fn text_content(&self) -> String {
        match &self.content {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(parts)) => parts
                .iter()
                .filter_map(|p| {
                    p.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolCall {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FunctionCall {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub r#type: String,
    pub function: ToolFunctionDef,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolFunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolDef {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            r#type: "function".into(),
            function: ToolFunctionDef {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i64>,
    pub tools: Vec<ToolDef>,
    pub tool_choice: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ChatResponse {
    pub content: String,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub enum StreamDelta {
    Content(String),
    Reasoning(String),
    Done,
    /// 工具调用增量（完整累积后由调用方组装）
    ToolCallDelta {
        index: usize,
        id: String,
        name: String,
        arguments: String,
    },
}
