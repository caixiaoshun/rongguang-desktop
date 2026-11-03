//! 任务流状态查询（前端用于恢复进行中的任务 UI）。

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use serde_json::json;

use fusion_core::error::AppError;
use fusion_core::models::agent::AgentConversation;

use crate::auth::AuthUser;
use crate::state::AppState;
use crate::support::{ok, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/task-stream/status", get(status))
        .route("/api/task-stream/running", get(running))
        .route("/api/task-stream/reconnect", get(reconnect))
}

#[derive(Deserialize)]
struct TaskQuery {
    #[serde(rename = "taskId")]
    task_id: String,
}

/// 返回任务的状态描述：`image_task` / `video_task` / `agent_run` / `unknown`。
async fn status(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<TaskQuery>,
) -> ApiResult<String> {
    let image: Option<i64> = sqlx::query_scalar(
        "SELECT status FROM afv_image_task WHERE task_id = ? AND deleted = 0 LIMIT 1",
    )
    .bind(&q.task_id)
    .fetch_optional(&state.db)
    .await?;
    if let Some(status) = image {
        return ok(format!("image_task:{status}"));
    }

    let video: Option<i64> = sqlx::query_scalar(
        "SELECT status FROM afv_video_task WHERE task_id = ? AND deleted = 0 LIMIT 1",
    )
    .bind(&q.task_id)
    .fetch_optional(&state.db)
    .await?;
    if let Some(status) = video {
        return ok(format!("video_task:{status}"));
    }

    let run: Option<String> =
        sqlx::query_scalar("SELECT status FROM afv_agent_run WHERE run_id = ? LIMIT 1")
            .bind(&q.task_id)
            .fetch_optional(&state.db)
            .await?
            .flatten();
    if let Some(status) = run {
        return ok(format!("agent_run:{status}"));
    }

    ok("unknown".into())
}

async fn running(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
) -> ApiResult<Vec<AgentConversation>> {
    let rows = sqlx::query_as::<_, AgentConversation>(
        "SELECT * FROM afv_agent_conversation WHERE status = 1 AND deleted = 0 ORDER BY update_time DESC",
    )
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

/// SSE 重连：桌面端任务状态由前端轮询任务接口即可，这里返回一次性事件流并结束。
async fn reconnect(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<TaskQuery>,
) -> Result<axum::response::Response, AppError> {
    use axum::response::IntoResponse;

    let payload = json!({
        "outputType": "DONE",
        "taskId": q.task_id,
        "finished": true,
    });
    let body = format!("data: {}\n\n", payload);
    let _ = state;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/event-stream"),
            (axum::http::header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response())
}
