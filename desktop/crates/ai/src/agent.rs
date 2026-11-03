//! 通用工具调用循环。
//!
//! 只负责「调模型 → 拿 tool_calls → 交给执行器 → 把结果回灌」的编排，
//! 具体 44 个工具由上层（server crate）实现 `ToolExecutor` 注入，避免 AI 层依赖数据层。

use std::sync::Arc;

use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use fusion_core::error::{AppError, AppResult};

use crate::provider::{Provider, ProviderConfig};
use crate::types::{ChatMessage, ChatRequest, ToolCall, ToolDef};

#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// 正文增量
    Delta(String),
    /// 推理过程增量
    Reasoning(String),
    /// 一轮模型调用的完整正文（增量结束后发送，便于前端落库）
    Message(String),
    ToolStarted {
        name: String,
        arguments: Value,
    },
    ToolFinished {
        name: String,
        result: String,
        ok: bool,
    },
    Status(String),
    Finished {
        content: String,
    },
    Failed(String),
}

#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    fn definitions(&self) -> Vec<ToolDef>;
    async fn execute(&self, name: &str, arguments: Value) -> AppResult<String>;
    /// 需要用户确认的高风险工具（与 Java 版权限分级对齐）。
    fn requires_confirmation(&self, _name: &str) -> bool {
        false
    }
}

pub struct AgentRunner {
    provider: Arc<dyn Provider>,
    config: ProviderConfig,
    model: String,
    executor: Arc<dyn ToolExecutor>,
    max_steps: usize,
}

impl AgentRunner {
    pub fn new(
        provider: Arc<dyn Provider>,
        config: ProviderConfig,
        model: impl Into<String>,
        executor: Arc<dyn ToolExecutor>,
    ) -> Self {
        Self {
            provider,
            config,
            model: model.into(),
            executor,
            max_steps: 12,
        }
    }

    pub fn with_max_steps(mut self, steps: usize) -> Self {
        self.max_steps = steps.max(1);
        self
    }

    /// 执行一次完整对话，事件通过 channel 推送；返回最终正文。
    pub async fn run(
        &self,
        messages: Vec<ChatMessage>,
        tx: UnboundedSender<AgentEvent>,
    ) -> AppResult<String> {
        let mut messages = messages;
        let tools = self.executor.definitions();
        let mut final_text = String::new();

        for step in 0..self.max_steps {
            let req = ChatRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                temperature: Some(0.7),
                max_tokens: None,
                tools: tools.clone(),
                tool_choice: None,
            };

            let _ = tx.send(AgentEvent::Status(format!("第 {} 轮推理", step + 1)));
            let response = self.provider.chat(&self.config, req).await?;
            let content = response.content.clone();

            if !content.trim().is_empty() {
                let _ = tx.send(AgentEvent::Message(content.clone()));
                final_text = content.clone();
            }

            if response.tool_calls.is_empty() {
                let _ = tx.send(AgentEvent::Finished {
                    content: final_text.clone(),
                });
                return Ok(final_text);
            }

            messages.push(ChatMessage {
                role: "assistant".into(),
                content: Some(Value::String(content)),
                name: None,
                tool_calls: Some(response.tool_calls.clone()),
                tool_call_id: None,
            });

            for call in response.tool_calls {
                let result = self.execute_tool(&call, &tx).await;
                messages.push(ChatMessage::tool_result(
                    call.id.clone(),
                    result.unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}")),
                ));
            }
        }

        let _ = tx.send(AgentEvent::Finished {
            content: final_text.clone(),
        });
        Ok(final_text)
    }

    async fn execute_tool(
        &self,
        call: &ToolCall,
        tx: &UnboundedSender<AgentEvent>,
    ) -> AppResult<String> {
        let name = call.function.name.clone();
        let args: Value = serde_json::from_str(&call.function.arguments)
            .unwrap_or_else(|_| Value::Object(Default::default()));

        if self.executor.requires_confirmation(&name) {
            let _ = tx.send(AgentEvent::Status(format!(
                "工具 {name} 需要确认，已跳过自动执行"
            )));
        }

        let _ = tx.send(AgentEvent::ToolStarted {
            name: name.clone(),
            arguments: args.clone(),
        });

        match self.executor.execute(&name, args).await {
            Ok(result) => {
                let _ = tx.send(AgentEvent::ToolFinished {
                    name,
                    result: truncate(&result, 4000),
                    ok: true,
                });
                Ok(result)
            }
            Err(err) => {
                let _ = tx.send(AgentEvent::ToolFinished {
                    name,
                    result: err.to_string(),
                    ok: false,
                });
                Err(AppError::business(err.to_string()))
            }
        }
    }

    /// 纯流式正文（不启用工具）：用于普通对话。
    pub async fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        tx: UnboundedSender<AgentEvent>,
    ) -> AppResult<String> {
        let req = ChatRequest {
            model: self.model.clone(),
            messages,
            temperature: Some(0.7),
            max_tokens: None,
            tools: Vec::new(),
            tool_choice: None,
        };
        let mut stream = self.provider.chat_stream(self.config.clone(), req);
        let mut content = String::new();
        while let Some(item) = stream.next().await {
            match item? {
                crate::types::StreamDelta::Content(text) => {
                    content.push_str(&text);
                    let _ = tx.send(AgentEvent::Delta(text));
                }
                crate::types::StreamDelta::Reasoning(text) => {
                    let _ = tx.send(AgentEvent::Reasoning(text));
                }
                crate::types::StreamDelta::Done => break,
                crate::types::StreamDelta::ToolCallDelta { .. } => {}
            }
        }
        let _ = tx.send(AgentEvent::Finished {
            content: content.clone(),
        });
        Ok(content)
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}…（已截断）")
}
