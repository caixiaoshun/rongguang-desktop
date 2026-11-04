//! Durable pipeline transport. Execution and journaling outlive the SSE connection.
use super::*;
use std::{convert::Infallible, pin::Pin, time::Duration};
use axum::response::sse::{Event, Sse};
use fusion_ai::agent::{AgentEvent, AgentRunner};
use futures_util::Stream;
use serde::Serialize;
use crate::routes::tools::AssistantTools;

type EventStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

fn normalized_status(value: &str) -> &str {
    match value {
        "running" | "pending" | "active" => "RUNNING",
        "completed" | "done" => "COMPLETED",
        "failed" | "error" => "FAILED",
        "cancelled" | "canceled" => "CANCELLED",
        other => other,
    }
}
fn terminal(value: &str) -> bool {
    matches!(normalized_status(value), "COMPLETED" | "FAILED" | "CANCELLED")
}
fn conversation_status(value: &str) -> &str {
    match value { "COMPLETED" => "completed", "FAILED" => "failed", "CANCELLED" => "cancelled", "RUNNING" => "running", other => other }
}
fn ui_tool_name(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::new();
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch.is_uppercase() && index > 0 && (chars[index - 1].is_lowercase()
            || (chars[index - 1].is_uppercase() && chars.get(index + 1).is_some_and(|next| next.is_lowercase()))) {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    match out.as_str() { "asset_create" => "create_asset".into(), "storyboard_item_insert" => "insert_storyboard_item".into(), _ => out }
}

/// Allocate the sequence and commit the event before exposing it to any client.
pub(super) async fn append_event(state: &AppState, run_id: &str, mut value: serde_json::Value) -> Result<serde_json::Value, AppError> {
    let mut tx = state.db.begin().await?;
    let row = sqlx::query("UPDATE afv_agent_run SET next_sequence = next_sequence + 1, update_time = ? WHERE run_id = ? AND terminal_sequence IS NULL RETURNING next_sequence - 1 AS sequence, conversation_id")
        .bind(now()).bind(run_id).fetch_optional(&mut *tx).await?
        .ok_or_else(|| AppError::business("运行已结束，不能追加事件"))?;
    let sequence: i64 = row.get("sequence");
    let conversation_id: String = row.get("conversation_id");
    value["schemaVersion"] = json!(1);
    value["runId"] = json!(run_id);
    value["conversationId"] = json!(conversation_id);
    value["sequence"] = json!(sequence);
    value["source"] = json!("assistant");
    value["createdAt"] = json!(chrono::Utc::now().to_rfc3339());
    let output = value["outputType"].as_str().unwrap_or("").to_owned();
    sqlx::query("INSERT INTO afv_agent_event (run_id, sequence_no, schema_version, output_type, payload_json, event_created_at, create_time) VALUES (?, ?, 1, ?, ?, ?, ?)")
        .bind(run_id).bind(sequence).bind(&output).bind(value.to_string())
        .bind(value["createdAt"].as_str()).bind(now()).execute(&mut *tx).await?;
    let next_status = match output.as_str() {
        "DONE" => Some("COMPLETED"), "ERROR" => Some("FAILED"), "CANCELLED" => Some("CANCELLED"),
        "USER_CONFIRMATION_REQUIRED" => Some("WAITING_CONFIRMATION"),
        "USER_CONFIRM_RESULT" => Some("RUNNING"), _ => None,
    };
    if let Some(status) = next_status {
        if terminal(status) {
            sqlx::query("UPDATE afv_agent_run SET status = ?, terminal_sequence = ?, terminal_output_type = ?, error_message = ?, active_conversation_id = NULL, waiting_reply_id = NULL, waiting_tool_call_id = NULL, waiting_tool_name = NULL, wait_expires_at = NULL, finished_at = ?, update_time = ? WHERE run_id = ?")
                .bind(status).bind(sequence).bind(&output).bind(value["error"].as_str())
                .bind(now()).bind(now()).bind(run_id).execute(&mut *tx).await?;
        } else if status == "WAITING_CONFIRMATION" {
            let tool = &value["pendingToolCalls"][0];
            sqlx::query("UPDATE afv_agent_run SET status = ?, waiting_reply_id = ?, waiting_tool_call_id = ?, waiting_tool_name = ?, wait_expires_at = ?, update_time = ? WHERE run_id = ?")
                .bind(status).bind(value["replyId"].as_str()).bind(tool["toolCallId"].as_str())
                .bind(tool["toolName"].as_str()).bind(value["expiresAt"].as_str()).bind(now())
                .bind(run_id).execute(&mut *tx).await?;
        } else {
            sqlx::query("UPDATE afv_agent_run SET status = ?, waiting_reply_id = NULL, waiting_tool_call_id = NULL, waiting_tool_name = NULL, wait_expires_at = NULL, update_time = ? WHERE run_id = ?")
                .bind(status).bind(now()).bind(run_id).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE afv_agent_conversation SET status = ?, update_time = ? WHERE conversation_id = ?")
            .bind(conversation_status(status)).bind(now()).bind(&conversation_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(value)
}

fn journal_stream(state: Arc<AppState>, run_id: String, after: i64) -> Sse<EventStream> {
    let stream = async_stream::stream! {
        let mut cursor = after;
        loop {
            let rows = match sqlx::query("SELECT sequence_no, payload_json, output_type FROM afv_agent_event WHERE run_id = ? AND sequence_no > ? ORDER BY sequence_no LIMIT 256")
                .bind(&run_id).bind(cursor).fetch_all(&state.db).await {
                    Ok(rows) => rows,
                    Err(error) => { tracing::warn!("读取流水线事件失败: {error}"); break; }
                };
            let mut ended = false;
            for row in rows {
                cursor = row.get("sequence_no");
                let payload: String = row.get("payload_json");
                let output: String = row.get("output_type");
                ended = matches!(output.as_str(), "DONE" | "ERROR" | "CANCELLED");
                yield Ok(Event::default().id(format!("{run_id}:{cursor}")).data(payload));
                if ended { break; }
            }
            if ended { break; }
            let metadata = sqlx::query("SELECT status, next_sequence FROM afv_agent_run WHERE run_id = ?")
                .bind(&run_id).fetch_optional(&state.db).await;
            match metadata {
                Ok(Some(row)) if terminal(row.get::<String, _>("status").as_str())
                    && cursor >= row.get::<i64, _>("next_sequence") - 1 => break,
                Ok(None) | Err(_) => break,
                _ => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
    };
    Sse::new(Box::pin(stream) as EventStream).keep_alive(axum::response::sse::KeepAlive::default())
}

fn pipeline_message(req: &ChatReq) -> Result<String, AppError> {
    if let Some(message) = req.message.as_ref().filter(|value| !value.trim().is_empty()) {
        return Ok(message.clone());
    }
    let operation = match req.agent_type.as_deref().unwrap_or("") {
        "story_to_script" | "script_full_parse" => "读取指定剧本的原文，整理故事梗概与角色，使用 UpdateScript、SaveScriptEpisode、SaveScriptSceneItems 保存完整的分集和场次；不要只输出建议。",
        "script_episode_parse" => "读取指定分集原文，提取场次、时间、地点、角色及对白，使用 SaveScriptSceneItems 保存解析结果。",
        "episode_scene_writer" => "读取指定剧本、分集和已有场次，依据用户要求创作并保存场次内容。",
        "episode_script_creator" => "读取剧本上下文，依据用户要求创作新的分集，使用 SaveScriptEpisode 和 SaveScriptSceneItems 保存结果。",
        "script_to_storyboard" | "episode_storyboard_writer" => "读取剧本结构与指定分集，查询项目唯一分镜容器，使用 SaveStoryboardEpisode、SaveStoryboardSceneShots 保存对应的分集和镜头。镜头应包含内容、景别、时长、运动、对白及声音。",
        "storyboard_asset_preprocessor" => "读取指定分镜及剧本，识别角色、场景与道具，查询已有资产并只补充缺失资产，保存可用于生成图片的提示词。",
        "asset_image_gen" | "asset_image_executor" => "读取指定资产，依据已有设定调用图像生成工具，等待生成完成后把图片地址回填资产，已成功的任务不得重复提交。",
        "storyboard_frame_gen" | "storyboard_frame_executor" => "读取指定镜头及其关联资产，生成所需首帧或尾帧图片并使用 UpdateStoryboardItemFrame 回填结果，保持人物与场景一致。",
        "storyboard_video_gen" | "storyboard_video_executor" => "读取指定镜头、首尾帧和运镜，调用视频生成工具，等待任务完成后使用 UpdateStoryboardItemVideo 回填结果，已成功的任务不得重复提交。",
        _ => return Err(AppError::bad_request("消息不能为空，或请选择受支持的创作任务")),
    };
    Ok(format!("{operation}\n任务上下文（只操作这里指定的项目与记录）：{}\n用户补充要求：{}",
        req.context.as_ref().unwrap_or(&serde_json::Value::Null), req.instruction.as_deref().unwrap_or("无")))
}

async fn record_agent_event(state: &AppState, run_id: &str, event: AgentEvent) -> Result<(), AppError> {
    let values = match event {
        AgentEvent::Delta(text) => vec![json!({"outputType":"CONTENT","content":text})],
        AgentEvent::Reasoning(text) => vec![json!({"outputType":"REASONING","reasoningContent":text})],
        AgentEvent::Message(text) => vec![json!({"outputType":"CONTENT","content":text})],
        AgentEvent::Status(text) => vec![json!({"outputType":"CONTENT","content":"","statusMessage":text})],
        AgentEvent::ToolStarted { id, name, arguments } => {
            let name = ui_tool_name(&name);
            let reply_id = format!("reply_{id}");
            vec![
                json!({"outputType":"TOOL_CALL_STARTED","replyId":reply_id,"toolName":name,"toolCallId":id,"toolCalls":[{"id":id,"name":name,"arguments":""}]}),
                json!({"outputType":"TOOL_CALL","replyId":reply_id,"toolName":name,"toolCallId":id,"toolCalls":[{"id":id,"name":name,"arguments":arguments.to_string()}]}),
            ]
        }
        AgentEvent::ToolFinished { id, name, result, ok } => vec![json!({
            "outputType":"TOOL_FINISHED","replyId":format!("reply_{id}"),"toolCallId":id,
            "toolName":ui_tool_name(&name),"toolResult":result,"toolStatus":if ok {"success"} else {"error"}
        })],
        AgentEvent::ConfirmationRequired { reply_id, call } => vec![json!({
            "outputType":"USER_CONFIRMATION_REQUIRED","replyId":reply_id,
            "pendingToolCalls":[{"toolCallId":call.id,"toolName":ui_tool_name(&call.function.name),"argumentsPreview":call.function.arguments}],
            "expiresAt":(chrono::Utc::now()+chrono::Duration::seconds(300)).to_rfc3339()
        })],
        AgentEvent::ConfirmationResolved { reply_id, tool_call_id, approved } => vec![json!({
            "outputType":"USER_CONFIRM_RESULT","replyId":reply_id,"decisions":[{"toolCallId":tool_call_id,"approved":approved}]
        })],
        // Terminal events are committed only after the assistant message and run
        // outcome have been saved. Repeating final content would duplicate it in UI.
        AgentEvent::Finished { .. } | AgentEvent::Failed(_) => Vec::new(),
    };
    for value in values { append_event(state, run_id, value).await?; }
    Ok(())
}

pub(super) async fn run_pipeline(
    State(state): State<Arc<AppState>>, user: AuthUser, Json(req): Json<ChatReq>,
) -> Result<Sse<EventStream>, AppError> {
    let message = pipeline_message(&req)?;
    let model = resolve_model(&state, req.model_id).await?;
    if model.model_type != 1 { return Err(AppError::bad_request("请选择对话模型")); }
    let (platform, cfg) = resolve_provider_config(&state, &model).await?;
    cfg.require_base_url()?;
    cfg.require_key()?;
    let provider = state.registry.chat(&platform)?;
    let mode = req.tool_execution_mode.as_deref().unwrap_or("DEFAULT").to_owned();
    if !matches!(mode.as_str(), "DEFAULT" | "ALWAYS_ASK" | "ALWAYS_ALLOW" | "FULL_ACCESS") {
        return Err(AppError::bad_request("不支持的工具执行模式"));
    }
    let (_, conversation_id) = ensure_conversation(&state, &req, user.id, &message).await?;
    let run_id = format!("run_{}", uuid::Uuid::new_v4().simple());
    let snapshot = json!({"modelId":model.id,"modelCode":model.code,"request":req});
    let inserted = sqlx::query("INSERT INTO afv_agent_run (run_id, conversation_id, active_conversation_id, user_id, project_id, agent_type, agent_definition_snapshot_json, status, started_at, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, ?, 'RUNNING', ?, ?, ?)")
        .bind(&run_id).bind(&conversation_id).bind(&conversation_id).bind(user.id).bind(req.project_id)
        .bind(req.agent_type.as_deref().unwrap_or("assistant")).bind(snapshot.to_string())
        .bind(now()).bind(now()).bind(now()).execute(&state.db).await;
    if let Err(error) = inserted {
        if error.as_database_error().is_some_and(|error| error.is_unique_violation()) {
            return Err(AppError::business("该会话仍有任务运行，请先等待完成或停止任务"));
        }
        return Err(error.into());
    }
    if let Err(error) = save_message(&state, &conversation_id, "user", &message, req.references_json.as_ref().and_then(|text| serde_json::from_str(text).ok())).await {
        append_event(&state,&run_id,json!({"outputType":"ERROR","error":error.to_string(),"finished":true})).await?;
        return Err(error);
    }
    sqlx::query("UPDATE afv_agent_conversation SET status = 'running', update_time = ? WHERE conversation_id = ?")
        .bind(now()).bind(&conversation_id).execute(&state.db).await?;
    let mut messages = vec![ChatMessage::system(build_system_prompt(&state, &req).await)];
    messages.extend(load_history(&state, &conversation_id, 50).await?);
    let tools = Arc::new(AssistantTools { state:state.clone(), user_id:user.id, run_id:run_id.clone(), execution_mode:mode });
    let runner = AgentRunner::new(provider, cfg, model.code, tools).with_max_steps(24);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (cancel, mut cancel_rx) = tokio::sync::watch::channel(false);
    state.pipeline_cancellations.lock().insert(run_id.clone(), cancel);
    let worker_state = state.clone();
    let worker_run = run_id.clone();
    tokio::spawn(async move {
        let work = runner.run(messages, tx.clone());
        tokio::pin!(work);
        let outcome = loop {
            tokio::select! {
                biased;
                _ = cancel_rx.changed() => break None,
                event = rx.recv() => {
                    if let Some(event) = event {
                        if let Err(error) = record_agent_event(&worker_state, &worker_run, event).await { break Some(Err(error)); }
                    }
                },
                result = &mut work => break Some(result),
            }
        };
        // Drain events emitted by the same final poll of the model future.
        while let Ok(event) = rx.try_recv() {
            if let Err(error) = record_agent_event(&worker_state, &worker_run, event).await { tracing::warn!("保存任务事件失败: {error}"); }
        }
        let event = match outcome {
            Some(Ok(content)) => match save_message(&worker_state, &conversation_id, "assistant", &content, None).await {
                Ok(()) => json!({"outputType":"DONE","finished":true}),
                Err(error) => json!({"outputType":"ERROR","error":error.to_string(),"finished":true}),
            },
            Some(Err(error)) => json!({"outputType":"ERROR","error":error.to_string(),"finished":true}),
            None => json!({"outputType":"CANCELLED","cancellationReason":"USER_CANCELLED","finished":true}),
        };
        if let Err(error) = append_event(&worker_state, &worker_run, event).await { tracing::warn!("保存任务终态失败: {error}"); }
        worker_state.pipeline_cancellations.lock().remove(&worker_run);
        worker_state.pipeline_confirmations.lock().retain(|_, pending| pending.run_id != worker_run);
    });
    Ok(journal_stream(state, run_id, 0))
}

#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub(super) struct ContinueQuery { conversation_id:String }
pub(super) async fn continue_pipeline(State(state):State<Arc<AppState>>, user:AuthUser, Query(q):Query<ContinueQuery>) -> Result<Sse<EventStream>, AppError> {
    let snapshot: Option<String> = sqlx::query_scalar("SELECT agent_definition_snapshot_json FROM afv_agent_run WHERE conversation_id = ? ORDER BY id DESC LIMIT 1")
        .bind(&q.conversation_id).fetch_optional(&state.db).await?.flatten();
    let mut req: ChatReq = snapshot.and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| serde_json::from_value(value["request"].clone()).ok()).unwrap_or_default();
    req.conversation_id=Some(q.conversation_id);
    req.message=Some("继续执行上次未完成的任务。先检查已经保存的结果，不得重复提交成功的生成任务。".into());
    run_pipeline(State(state), user, Json(req)).await
}

#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub(super) struct ReconnectQuery { run_id:String, #[serde(default)] after_sequence:i64 }
pub(super) async fn reconnect_pipeline(State(state):State<Arc<AppState>>, _user:AuthUser, Query(q):Query<ReconnectQuery>) -> Result<Sse<EventStream>, AppError> {
    let last: Option<i64> = sqlx::query_scalar("SELECT next_sequence - 1 FROM afv_agent_run WHERE run_id = ?").bind(&q.run_id).fetch_optional(&state.db).await?;
    let last=last.ok_or_else(||AppError::not_found("运行记录不存在"))?;
    if q.after_sequence<0 || q.after_sequence>last { return Err(AppError::bad_request("事件游标超出运行记录范围")); }
    Ok(journal_stream(state,q.run_id,q.after_sequence))
}

#[derive(Deserialize, Default)]
#[serde(rename_all="camelCase",default)]
pub(super) struct RunTarget { run_id:Option<String>, conversation_id:Option<String> }
async fn resolve_target(state:&AppState,target:&RunTarget) -> Result<String,AppError> {
    match (&target.run_id,&target.conversation_id) {
        (Some(run),None) if !run.trim().is_empty() => Ok(run.clone()),
        (None,Some(conversation)) => sqlx::query_scalar("SELECT run_id FROM afv_agent_run WHERE conversation_id = ? ORDER BY id DESC LIMIT 1")
            .bind(conversation).fetch_optional(&state.db).await?.ok_or_else(||AppError::not_found("该会话尚无运行记录")),
        _=>Err(AppError::bad_request("请指定 runId 或 conversationId")),
    }
}
pub(super) async fn cancel_pipeline(State(state):State<Arc<AppState>>, _user:AuthUser, Query(target):Query<RunTarget>) -> ApiResult<bool> {
    let run_id=resolve_target(&state,&target).await?;
    let changed=sqlx::query("UPDATE afv_agent_run SET status = 'CANCEL_REQUESTED', cancel_requested_at = ?, update_time = ? WHERE run_id = ? AND terminal_sequence IS NULL")
        .bind(now()).bind(now()).bind(&run_id).execute(&state.db).await?.rows_affected();
    let cancel=state.pipeline_cancellations.lock().get(&run_id).cloned();
    if changed>0 {
        if let Some(cancel)=cancel { let _=cancel.send(true); }
        else { append_event(&state,&run_id,json!({"outputType":"CANCELLED","cancellationReason":"USER_CANCELLED","finished":true})).await?; }
    }
    ok(true)
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all="camelCase")]
pub(super) struct Decision { tool_call_id:String, approved:bool }
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub(super) struct ConfirmReq { run_id:String,reply_id:String,decisions:Vec<Decision> }
pub(super) async fn confirm_pipeline(State(state):State<Arc<AppState>>, _user:AuthUser, Json(req):Json<ConfirmReq>) -> ApiResult<bool> {
    if req.decisions.len()!=1 { return Err(AppError::bad_request("请提交当前工具的明确审批决定")); }
    let decision=&req.decisions[0];
    let pending={
        let mut entries=state.pipeline_confirmations.lock();
        let pending=entries.get(&req.reply_id).ok_or_else(||AppError::business("审批已结束或已过期"))?;
        if pending.run_id!=req.run_id || pending.tool_call_id!=decision.tool_call_id { return Err(AppError::bad_request("审批与当前运行或工具不匹配")); }
        entries.remove(&req.reply_id).unwrap()
    };
    pending.sender.send(decision.approved).map_err(|_|AppError::business("审批等待已结束"))?;
    ok(true)
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub(super) struct ExpireReq { run_id:String,reply_id:String }
pub(super) async fn expire_confirmation(State(state):State<Arc<AppState>>, user:AuthUser, Json(req):Json<ExpireReq>) -> ApiResult<bool> {
    let matching=state.pipeline_confirmations.lock().get(&req.reply_id).is_some_and(|pending|pending.run_id==req.run_id);
    if !matching { return Err(AppError::business("审批已结束或已过期")); }
    cancel_pipeline(State(state),user,Query(RunTarget{run_id:Some(req.run_id),conversation_id:None})).await
}

pub(super) async fn pipeline_status(State(state):State<Arc<AppState>>, _user:AuthUser, Query(target):Query<RunTarget>) -> ApiResult<serde_json::Value> {
    let run_id=resolve_target(&state,&target).await?;
    let row=sqlx::query("SELECT status, next_sequence, terminal_sequence, waiting_reply_id FROM afv_agent_run WHERE run_id = ?")
        .bind(&run_id).fetch_optional(&state.db).await?.ok_or_else(||AppError::not_found("运行记录不存在"))?;
    let status:String=row.get("status");
    let terminal_sequence:Option<i64>=row.get("terminal_sequence");
    let terminal_event:Option<String>=match terminal_sequence {
        Some(sequence)=>sqlx::query_scalar("SELECT payload_json FROM afv_agent_event WHERE run_id = ? AND sequence_no = ?")
            .bind(&run_id).bind(sequence).fetch_optional(&state.db).await?,
        None=>None,
    };
    ok(json!({"runId":run_id,"status":normalized_status(&status),"lastSequence":row.get::<i64,_>("next_sequence")-1,
        "waitingReplyId":row.get::<Option<String>,_>("waiting_reply_id"),
        "terminalEvent":terminal_event.and_then(|text|serde_json::from_str::<serde_json::Value>(&text).ok())}))
}
pub(super) async fn running_pipelines(State(state):State<Arc<AppState>>, _user:AuthUser) -> ApiResult<Vec<serde_json::Value>> {
    let rows=sqlx::query("SELECT r.run_id,r.conversation_id,r.project_id,r.status,r.next_sequence,r.waiting_reply_id,r.started_at,c.title,c.category FROM afv_agent_run r JOIN afv_agent_conversation c ON c.conversation_id=r.conversation_id WHERE upper(r.status) IN ('RUNNING','WAITING_CONFIRMATION','WAITING_EXTERNAL','CANCEL_REQUESTED') AND c.deleted=0 ORDER BY r.id DESC LIMIT 50")
        .fetch_all(&state.db).await?;
    ok(rows.into_iter().map(|row|{
        let status:String=row.get("status");
        json!({"runId":row.get::<String,_>("run_id"),"conversationId":row.get::<String,_>("conversation_id"),
            "projectId":row.get::<Option<i64>,_>("project_id"),"title":row.get::<Option<String>,_>("title"),
            "category":row.get::<Option<String>,_>("category"),"status":normalized_status(&status),
            "lastSequence":row.get::<i64,_>("next_sequence")-1,"waitingReplyId":row.get::<Option<String>,_>("waiting_reply_id"),
            "startedAt":row.get::<Option<String>,_>("started_at")})
    }).collect())
}
pub(super) async fn recover_interrupted(state:&Arc<AppState>) -> Result<(),AppError> {
    let runs:Vec<String>=sqlx::query_scalar("SELECT run_id FROM afv_agent_run WHERE upper(status) IN ('RUNNING','PENDING','ACTIVE','WAITING_CONFIRMATION','WAITING_EXTERNAL','CANCEL_REQUESTED') AND terminal_sequence IS NULL")
        .fetch_all(&state.db).await?;
    for run_id in runs {
        append_event(state,&run_id,json!({"outputType":"ERROR","error":"应用在任务完成前关闭，本次运行已中断。请检查已保存结果后继续。","finished":true})).await?;
    }
    Ok(())
}
