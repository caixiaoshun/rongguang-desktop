//! AI 助手：会话记录 + Agent 流水线（SSE）。

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;

use fusion_ai::agent::{AgentEvent, AgentRunner};
use fusion_ai::types::ChatMessage;
use fusion_core::error::AppError;
use fusion_core::models::agent::{
    AgentConversation, AgentMessage, AssistantReferenceOptions, AssistantSkillOption,
    AssistantMcpToolOption, ReferenceOption,
};

use crate::auth::AuthUser;
use crate::routes::ai::{load_model, resolve_provider_config};
use crate::routes::tools::AssistantTools;
use crate::state::AppState;
use crate::support::{now, ok, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/ai/assistant/reference-options", get(reference_options))
        .route("/api/ai/assistant/conversations", get(list_conversations))
        .route(
            "/api/ai/assistant/conversations/{conversation_id}/messages",
            get(list_messages),
        )
        .route(
            "/api/ai/assistant/conversations/by-conversation-id/{conversation_id}",
            axum::routing::delete(delete_by_business_id),
        )
        .route(
            "/api/ai/assistant/conversations/{id}",
            axum::routing::delete(delete_conversation),
        )
        .route("/api/ai/pipeline/run", post(run_pipeline))
        .route("/api/ai/pipeline/continue", post(continue_pipeline))
        .route("/api/ai/pipeline/reconnect", get(reconnect_pipeline))
        .route("/api/ai/pipeline/cancel", post(cancel_pipeline))
        .route("/api/ai/pipeline/confirm", post(confirm_pipeline))
        .route("/api/ai/pipeline/confirm/expire", post(expire_confirmation))
        .route("/api/ai/pipeline/status", get(pipeline_status))
        .route("/api/ai/pipeline/running", get(running_pipelines))
}

async fn reference_options(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
) -> ApiResult<AssistantReferenceOptions> {
    let projects = sqlx::query("SELECT id, name FROM afv_project WHERE deleted = 0 ORDER BY id DESC LIMIT 50")
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|row| ReferenceOption {
            id: row.get("id"),
            name: row.get("name"),
            extra: None,
        })
        .collect();

    let scripts = sqlx::query("SELECT id, IFNULL(title,'未命名剧本') AS name FROM afv_script WHERE deleted = 0 ORDER BY id DESC LIMIT 50")
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|row| ReferenceOption {
            id: row.get("id"),
            name: row.get("name"),
            extra: None,
        })
        .collect();

    let assets = sqlx::query("SELECT id, IFNULL(name,'未命名资产') AS name, type FROM afv_asset WHERE deleted = 0 ORDER BY id DESC LIMIT 100")
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|row| ReferenceOption {
            id: row.get("id"),
            name: row.get("name"),
            extra: row.get::<Option<String>, _>("type"),
        })
        .collect();

    let skills = super::agent_config::read_skills(&state).into_iter().map(|skill| {
        AssistantSkillOption {
            display_name: skill.display_name.unwrap_or_else(|| skill.name.clone()),
            id: skill.id,
            name: skill.name,
            description: skill.description,
            source: skill.source,
        }
    }).collect();
    // Only explicitly enabled tools are offered; rendering the picker must not
    // contact remote MCP servers or assume that a tool is read-only.
    let servers = sqlx::query("SELECT name, enabled_tools_json FROM afv_agent_mcp_server WHERE deleted = 0 AND status = 1 ORDER BY id")
        .fetch_all(&state.db).await?;
    let mut mcp_tools = Vec::new();
    for row in servers {
        let server_name: String = row.get("name");
        let enabled: Option<String> = row.get("enabled_tools_json");
        for tool_name in crate::support::parse_string_list(&enabled) {
            mcp_tools.push(AssistantMcpToolOption {
                server_name: server_name.clone(), tool_name,
                description: String::new(), read_only: false,
            });
        }
    }
    ok(AssistantReferenceOptions {
        skills,
        mcp_tools,
        projects,
        scripts,
        assets,
    })
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ConversationQuery {
    page: Option<i64>,
    size: Option<i64>,
    page_no: Option<i64>,
    page_size: Option<i64>,
    category: Option<String>,
}

async fn list_conversations(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<ConversationQuery>,
) -> ApiResult<serde_json::Value> {
    let size = q.page_size.or(q.size).unwrap_or(50).clamp(1, 200);
    let offset = q.page_no.or(q.page).unwrap_or(1).max(1).saturating_sub(1).saturating_mul(size);
    let category = q.category.filter(|value| !value.trim().is_empty());
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM afv_agent_conversation WHERE deleted = 0 AND (? IS NULL OR COALESCE(NULLIF(category, ''), 'assistant') = ?)",
    ).bind(&category).bind(&category).fetch_one(&state.db).await?;
    let rows = sqlx::query_as::<_, AgentConversation>(
        "SELECT * FROM afv_agent_conversation WHERE deleted = 0 AND (? IS NULL OR COALESCE(NULLIF(category, ''), 'assistant') = ?) ORDER BY update_time DESC, id DESC LIMIT ? OFFSET ?",
    )
    .bind(&category).bind(&category).bind(size).bind(offset)
    .fetch_all(&state.db).await?;
    ok(json!({ "list": rows, "total": total }))
}

async fn list_messages(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(conversation_id): Path<String>,
) -> ApiResult<Vec<AgentMessage>> {
    let rows = sqlx::query_as::<_, AgentMessage>(
        "SELECT * FROM afv_agent_message WHERE conversation_id = ? AND deleted = 0 ORDER BY id ASC",
    )
    .bind(conversation_id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn delete_conversation(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    let conversation_id: Option<String> =
        sqlx::query_scalar("SELECT conversation_id FROM afv_agent_conversation WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .flatten();
    sqlx::query("UPDATE afv_agent_conversation SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    if let Some(cid) = conversation_id {
        sqlx::query("UPDATE afv_agent_message SET deleted = 1, update_time = ? WHERE conversation_id = ?")
            .bind(now())
            .bind(cid)
            .execute(&state.db)
            .await?;
    }
    ok(true)
}

async fn delete_by_business_id(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(conversation_id): Path<String>,
) -> ApiResult<bool> {
    sqlx::query(
        "UPDATE afv_agent_conversation SET deleted = 1, update_time = ? WHERE conversation_id = ?",
    )
    .bind(now())
    .bind(&conversation_id)
    .execute(&state.db)
    .await?;
    sqlx::query("UPDATE afv_agent_message SET deleted = 1, update_time = ? WHERE conversation_id = ?")
        .bind(now())
        .bind(&conversation_id)
        .execute(&state.db)
        .await?;
    ok(true)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ChatReq {
    pub message: Option<String>,
    pub conversation_id: Option<String>,
    pub model_id: Option<i64>,
    pub title: Option<String>,
    pub category: Option<String>,
    pub project_id: Option<i64>,
    pub system_prompt: Option<String>,
    pub instruction: Option<String>,
    pub enabled_tools: Option<Vec<String>>,
}

async fn resolve_model(state: &AppState, model_id: Option<i64>) -> Result<fusion_core::models::ai::AiModel, AppError> {
    if let Some(id) = model_id {
        return load_model(state, id).await;
    }
    let default: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM afv_ai_model WHERE deleted = 0 AND status = 1 AND model_type = 1 ORDER BY default_model DESC, sort ASC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;
    match default {
        Some(id) => load_model(state, id).await,
        None => Err(AppError::business("尚未配置可用的对话模型，请先在设置中添加 Grok API Key")),
    }
}

/// 构造系统提示词：注入当前工作区上下文，让模型知道有哪些工具可用。
async fn build_system_prompt(state: &AppState, req: &ChatReq) -> String {
    let mut prompt = String::from(
        "你是「融光」视频创作平台的 AI 助手，擅长剧本拆解、分镜设计与视觉提示词编写。\n\
         你可以调用工具直接操作项目数据：项目、剧本、分镜、资产、图像/视频生成。\n\
         工作原则：\n\
         1. 需要读取或修改数据时，先调用工具，不要凭空编造 ID；\n\
         2. 一次只推进一个明确步骤，确认结果后再继续；\n\
         3. 生成类工具是异步的，提交后告知用户任务已排队，并给出任务 ID；\n\
         4. 回复使用简体中文，简洁具体。\n",
    );

    if let Some(project_id) = req.project_id {
        if let Ok(Some(row)) = sqlx::query("SELECT name, art_style_description FROM afv_project WHERE id = ?")
            .bind(project_id)
            .fetch_optional(&state.db)
            .await
            .map(|r| r)
        {
            let name: String = row.get("name");
            let style: Option<String> = row.get("art_style_description");
            prompt.push_str(&format!(
                "\n当前项目：{}（ID {}），画风：{}\n",
                name,
                project_id,
                style.unwrap_or_else(|| "未设置".into())
            ));
        }
    }

    if let Some(extra) = &req.system_prompt {
        if !extra.trim().is_empty() {
            prompt.push_str("\n额外要求：\n");
            prompt.push_str(extra);
        }
    }
    prompt
}

async fn ensure_conversation(
    state: &AppState,
    req: &ChatReq,
    user_id: i64,
    first_message: &str,
) -> Result<(i64, String), AppError> {
    if let Some(cid) = &req.conversation_id {
        if !cid.trim().is_empty() {
            let id: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM afv_agent_conversation WHERE conversation_id = ? AND deleted = 0",
            )
            .bind(cid)
            .fetch_optional(&state.db)
            .await?
            .flatten();
            if let Some(id) = id {
                return Ok((id, cid.clone()));
            }
        }
    }

    let conversation_id = format!("conv_{}", uuid::Uuid::new_v4().simple());
    let title = req
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| first_message.chars().take(24).collect());
    let id =     sqlx::query(
        "INSERT INTO afv_agent_conversation (conversation_id, user_id, title, project_id, category, status, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, 'active', 0, ?, ?)",
    )
    .bind(&conversation_id)
    .bind(user_id)
    .bind(&title)
    .bind(req.project_id)
    .bind(req.category.as_deref().filter(|v| !v.trim().is_empty()).unwrap_or("assistant"))
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();
    Ok((id, conversation_id))
}

async fn save_message(
    state: &AppState,
    conversation_id: &str,
    role: &str,
    content: &str,
    metadata: Option<serde_json::Value>,
) -> Result<(), AppError> {
    // message_order 由会话表的 next_message_order 递增分配，保证同一会话内严格有序。
    let order: i64 = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT next_message_order FROM afv_agent_conversation WHERE conversation_id = ?",
    )
    .bind(conversation_id)
    .fetch_optional(&state.db)
    .await?
    .flatten()
    .unwrap_or(1);

    sqlx::query(
        "INSERT INTO afv_agent_message (conversation_id, role, content, references_json, message_order, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, 0, ?, ?)",
    )
    .bind(conversation_id)
    .bind(role)
    .bind(content)
    .bind(metadata.map(|m| m.to_string()))
    .bind(order)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?;
    sqlx::query(
        "UPDATE afv_agent_conversation SET next_message_order = ?, message_count = message_count + 1, last_message_time = ?, update_time = ? WHERE conversation_id = ?",
    )
    .bind(order + 1)
    .bind(now())
    .bind(now())
    .bind(conversation_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn load_history(state: &AppState, conversation_id: &str, limit: usize) -> Result<Vec<ChatMessage>, AppError> {
    let rows = sqlx::query(
        "SELECT role, content FROM afv_agent_message WHERE conversation_id = ? AND deleted = 0 ORDER BY id DESC LIMIT ?",
    )
    .bind(conversation_id)
    .bind(limit as i64)
    .fetch_all(&state.db)
    .await?;
    let mut messages: Vec<ChatMessage> = rows
        .into_iter()
        .rev()
        .filter_map(|row| {
            let role: String = row.get("role");
            let content: Option<String> = row.get("content");
            content.map(|c| ChatMessage::text(&role, c))
        })
        .collect();
    if messages.is_empty() {
        messages.push(ChatMessage::user("继续"));
    }
    Ok(messages)
}

/// SSE 响应封装：把 AgentEvent 转成前端约定的 outputType 事件。
fn sse_from_events(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    run_id: String,
    conversation_id: String,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let mut sequence: i64 = 0;
    let stream = async_stream::stream! {
        while let Some(event) = rx.recv().await {
            sequence += 1;
            let payload = match event {
                AgentEvent::Delta(text) => json!({
                    "outputType": "CONTENT",
                    "content": text,
                    "runId": run_id,
                    "conversationId": conversation_id,
                    "sequence": sequence,
                }),
                AgentEvent::Reasoning(text) => json!({
                    "outputType": "REASONING",
                    "reasoningContent": text,
                    "runId": run_id,
                    "conversationId": conversation_id,
                    "sequence": sequence,
                }),
                AgentEvent::Message(text) => json!({
                    "outputType": "CONTENT",
                    "content": text,
                    "finished": false,
                    "runId": run_id,
                    "conversationId": conversation_id,
                    "sequence": sequence,
                }),
                AgentEvent::ToolStarted { name, arguments } => json!({
                    "outputType": "TOOL_CALL_STARTED",
                    "toolName": name,
                    "toolCallId": format!("call_{sequence}"),
                    "toolCalls": [{ "id": format!("call_{sequence}"), "name": name, "arguments": arguments.to_string() }],
                    "runId": run_id,
                    "conversationId": conversation_id,
                    "sequence": sequence,
                }),
                AgentEvent::ToolFinished { name, result, ok } => json!({
                    "outputType": "TOOL_FINISHED",
                    "toolName": name,
                    "toolResult": result,
                    "toolStatus": if ok { "success" } else { "error" },
                    "runId": run_id,
                    "conversationId": conversation_id,
                    "sequence": sequence,
                }),
                AgentEvent::Status(text) => json!({
                    "outputType": "CONTENT",
                    "content": "",
                    "source": text,
                    "runId": run_id,
                    "conversationId": conversation_id,
                    "sequence": sequence,
                }),
                AgentEvent::Finished { content } => json!({
                    "outputType": "DONE",
                    "content": content,
                    "finished": true,
                    "runId": run_id,
                    "conversationId": conversation_id,
                    "sequence": sequence,
                }),
                AgentEvent::Failed(error) => json!({
                    "outputType": "ERROR",
                    "error": error,
                    "finished": true,
                    "runId": run_id,
                    "conversationId": conversation_id,
                    "sequence": sequence,
                }),
            };
            yield Ok(Event::default().data(payload.to_string()));
        }
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

async fn run_pipeline(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<ChatReq>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, AppError> {
    let message = req
        .message
        .clone()
        .filter(|m| !m.trim().is_empty())
        .ok_or_else(|| AppError::bad_request("消息不能为空"))?;

    let (conv_row_id, conversation_id) = ensure_conversation(&state, &req, user.id, &message).await?;
    save_message(&state, &conversation_id, "user", &message, None).await?;

    let model = resolve_model(&state, req.model_id).await?;
    let (platform, cfg) = resolve_provider_config(&state, &model).await?;
    let provider = state.registry.chat(&platform)?;

    let mut messages = vec![ChatMessage::system(build_system_prompt(&state, &req).await)];
    messages.extend(load_history(&state, &conversation_id, 30).await?);

    let run_id = format!("run_{}", uuid::Uuid::new_v4().simple());
    // afv_agent_run 没有 input/output/model_id 列：模型与首条输入记在快照 JSON 里，
    // 输出通过 afv_agent_message 投影，状态与错误分别落在 status / error_code / error_message。
    let snapshot = serde_json::json!({
        "modelId": model.id,
        "modelCode": model.code,
        "input": message,
    });
    let _ = sqlx::query(
        "INSERT INTO afv_agent_run (run_id, conversation_id, user_id, project_id, agent_type, agent_definition_snapshot_json, status, started_at, create_time, update_time) VALUES (?, ?, ?, ?, 'assistant', ?, 'running', ?, ?, ?)",
    )
    .bind(&run_id)
    .bind(&conversation_id)
    .bind(user.id)
    .bind(req.project_id)
    .bind(snapshot.to_string())
    .bind(now())
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await;

    let tools = Arc::new(AssistantTools {
        state: state.clone(),
        user_id: user.id,
    });
    let runner = AgentRunner::new(provider, cfg, model.code.clone(), tools);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let state_clone = state.clone();
    let run_id_clone = run_id.clone();
    let conversation_clone = conversation_id.clone();
    tokio::spawn(async move {
        let result = runner.run(messages, tx.clone()).await;
        match &result {
            Ok(content) => {
                let _ = save_message(&state_clone, &conversation_clone, "assistant", content, None).await;
                let _ = sqlx::query(
                    "UPDATE afv_agent_run SET status = 'completed', finished_at = ?, update_time = ? WHERE run_id = ?",
                )
                .bind(now())
                .bind(now())
                .bind(&run_id_clone)
                .execute(&state_clone.db)
                .await;
            }
            Err(err) => {
                let _ = tx.send(AgentEvent::Failed(err.to_string())).ok();
                let _ = sqlx::query(
                    "UPDATE afv_agent_run SET status = 'failed', error_code = 'provider_error', error_message = ?, finished_at = ?, update_time = ? WHERE run_id = ?",
                )
                .bind(err.to_string())
                .bind(now())
                .bind(now())
                .bind(&run_id_clone)
                .execute(&state_clone.db)
                .await;
            }
        }
        let _ = conv_row_id;
    });

    Ok(sse_from_events(rx, run_id, conversation_id))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContinueQuery {
    conversation_id: String,
}

async fn continue_pipeline(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<ContinueQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, AppError> {
    run_pipeline(
        State(state),
        user,
        Json(ChatReq {
            message: Some("继续".into()),
            conversation_id: Some(q.conversation_id),
            ..Default::default()
        }),
    )
    .await
}

#[derive(Deserialize)]
struct ReconnectQuery {
    #[serde(rename = "runId")]
    run_id: String,
    #[serde(rename = "afterSequence", default)]
    _after_sequence: i64,
}

/// 桌面端不持久化流式字节，重连时返回终态，由前端从消息表恢复。
async fn reconnect_pipeline(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<ReconnectQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, AppError> {
    let row = sqlx::query(
        "SELECT status, error_message, conversation_id FROM afv_agent_run WHERE run_id = ?",
    )
    .bind(&q.run_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("运行记录不存在"))?;
    let status: String = row.get("status");
    let error: Option<String> = row.get("error_message");
    let conversation_id: Option<String> = row.get("conversation_id");

    // 输出不落在 run 表，取该会话最后一条 assistant 消息作为终态内容。
    let output: Option<String> = match &conversation_id {
        Some(cid) => sqlx::query_scalar::<_, Option<String>>(
            "SELECT content FROM afv_agent_message WHERE conversation_id = ? AND role = 'assistant' AND deleted = 0 ORDER BY message_order DESC, id DESC LIMIT 1",
        )
        .bind(cid)
        .fetch_optional(&state.db)
        .await?
        .flatten(),
        None => None,
    };

    let payload = json!({
        "outputType": if status == "failed" { "ERROR" } else { "DONE" },
        "content": output,
        "error": error,
        "finished": true,
        "runId": q.run_id,
        "conversationId": conversation_id,
    });
    let stream = futures_util::stream::iter(vec![Ok(Event::default().data(payload.to_string()))]);
    Ok(Sse::new(stream))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct CancelReq {
    run_id: Option<String>,
    conversation_id: Option<String>,
}

async fn cancel_pipeline(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<CancelReq>,
) -> ApiResult<bool> {
    if let Some(run_id) = req.run_id {
        sqlx::query(
            "UPDATE afv_agent_run SET status = 'cancelled', finished_at = ?, update_time = ? WHERE run_id = ?",
        )
        .bind(now())
        .bind(now())
        .bind(run_id)
        .execute(&state.db)
        .await?;
    }
    ok(true)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ConfirmReq {
    run_id: Option<String>,
    approved: Option<bool>,
}

async fn confirm_pipeline(
    State(_state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(_req): Json<ConfirmReq>,
) -> ApiResult<bool> {
    // 桌面端默认全自动执行；确认接口保留以兼容前端交互。
    ok(true)
}

async fn expire_confirmation(
    State(_state): State<Arc<AppState>>,
    _u: AuthUser,
) -> ApiResult<bool> {
    ok(true)
}

#[derive(Deserialize)]
struct StatusQuery {
    #[serde(rename = "runId")]
    run_id: String,
}

async fn pipeline_status(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<StatusQuery>,
) -> ApiResult<serde_json::Value> {
    let row = sqlx::query("SELECT status FROM afv_agent_run WHERE run_id = ?")
        .bind(&q.run_id)
        .fetch_optional(&state.db)
        .await?;
    match row {
        Some(row) => {
            let status: String = row.get("status");
            ok(json!({ "runId": q.run_id, "status": status }))
        }
        None => ok(json!({ "runId": q.run_id, "status": "unknown" })),
    }
}

async fn running_pipelines(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
) -> ApiResult<Vec<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT run_id, conversation_id, status, started_at FROM afv_agent_run WHERE status = 'running' ORDER BY id DESC LIMIT 50",
    )
    .fetch_all(&state.db)
    .await?;
    ok(rows
        .into_iter()
        .map(|row| {
            json!({
                "runId": row.get::<String, _>("run_id"),
                "conversationId": row.get::<Option<String>, _>("conversation_id"),
                "status": row.get::<String, _>("status"),
                "startedAt": row.get::<Option<String>, _>("started_at"),
            })
        })
        .collect())
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use fusion_core::{config::AppConfig, paths::AppPaths};

    async fn fixture() -> Arc<AppState> {
        let root = std::env::temp_dir().join(format!("rongguang-contract-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = AppPaths {
            db_file: root.join("fusion.db"), media_dir: root.join("media"),
            upload_dir: root.join("uploads"), workspace_dir: root.join("workspace"),
            web_dir: root.join("web"), logs_dir: root.join("logs"), root,
        };
        Arc::new(AppState::new(paths, AppConfig::default()).await.unwrap())
    }

    async fn cleanup(state: Arc<AppState>) {
        state.db.close().await;
        std::fs::remove_dir_all(&state.paths.root).unwrap();
    }

    #[tokio::test]
    async fn references_include_empty_arrays_and_configured_metadata() {
        let state = fixture().await;
        let empty = reference_options(State(state.clone()), AuthUser::anonymous()).await.unwrap().0.data.unwrap();
        let empty = serde_json::to_value(empty).unwrap();
        assert_eq!(empty["skills"], json!([]));
        assert_eq!(empty["mcpTools"], json!([]));
        let skill = state.paths.root.join("agents/skills/story");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\ndisplayName: Story helper\ndescription: Outline a story\n---\nprivate skill instructions").unwrap();
        sqlx::query("INSERT INTO afv_agent_mcp_server (name, enabled_tools_json) VALUES ('library', '[\"search\"]')").execute(&state.db).await.unwrap();
        sqlx::query("INSERT INTO afv_agent_mcp_server (name, enabled_tools_json, status) VALUES ('disabled', '[\"hidden\"]', 0)").execute(&state.db).await.unwrap();
        let options = reference_options(State(state.clone()), AuthUser::anonymous()).await.unwrap().0.data.unwrap();
        let options = serde_json::to_value(options).unwrap();
        assert_eq!(options["skills"][0]["displayName"], "Story helper");
        assert!(options["skills"][0].get("content").is_none());
        assert_eq!(options["mcpTools"].as_array().unwrap().len(), 1);
        assert_eq!(options["mcpTools"][0]["toolName"], "search");
        assert_eq!(options["mcpTools"][0]["readOnly"], false);
        cleanup(state).await;
    }

    #[tokio::test]
    async fn assistant_history_preserves_legacy_and_new_conversations() {
        let state = fixture().await;
        sqlx::query("INSERT INTO afv_agent_conversation (conversation_id) VALUES ('legacy')")
            .execute(&state.db).await.unwrap();
        let (_, new_id) = ensure_conversation(&state, &ChatReq::default(), 1, "hello").await.unwrap();
        let pipeline = ChatReq { category: Some("pipeline".into()), ..Default::default() };
        ensure_conversation(&state, &pipeline, 1, "workflow").await.unwrap();
        let q: ConversationQuery = serde_json::from_value(json!({"category": "assistant"})).unwrap();
        let page = list_conversations(State(state.clone()), AuthUser::anonymous(), Query(q)).await.unwrap().0.data.unwrap();
        assert_eq!(page["total"], 2);
        assert_eq!(page["list"][0]["conversationId"], new_id);
        assert_eq!(page["list"][0]["category"], "assistant");
        assert_eq!(page["list"][1]["conversationId"], "legacy");
        cleanup(state).await;
    }

    #[tokio::test]
    async fn conversation_pagination_matches_frontend_and_filters_deleted_and_category() {
        let state = fixture().await;
        for (id, category, deleted) in [("one", "chat", 0), ("two", "chat", 0), ("other", "task", 0), ("deleted", "chat", 1)] {
            sqlx::query("INSERT INTO afv_agent_conversation (conversation_id, category, deleted) VALUES (?, ?, ?)")
                .bind(id).bind(category).bind(deleted).execute(&state.db).await.unwrap();
        }
        let q: ConversationQuery = serde_json::from_value(json!({"pageNo": 2, "pageSize": 1, "category": "chat"})).unwrap();
        let page = list_conversations(State(state.clone()), AuthUser::anonymous(), Query(q)).await.unwrap().0.data.unwrap();
        assert_eq!(page["total"], 2);
        assert_eq!(page["list"].as_array().unwrap().len(), 1);
        assert_eq!(page["list"][0]["conversationId"], "one");
        let q: ConversationQuery = serde_json::from_value(json!({"page": 1, "size": 1})).unwrap();
        let page = list_conversations(State(state.clone()), AuthUser::anonymous(), Query(q)).await.unwrap().0.data.unwrap();
        assert_eq!(page["total"], 3);
        assert_eq!(page["list"][0]["conversationId"], "other");
        cleanup(state).await;
    }
}
