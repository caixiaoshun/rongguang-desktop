//! 图像与视频生成：任务入库、后台执行、产物落地与状态查询。

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};

use fusion_ai::provider::{ImageRequest, VideoPoll, VideoSubmit};
use fusion_core::error::AppError;
use fusion_core::models::generation::{
    task_status, ImageItem, ImageSubmitReq, ImageTask, VideoItem, VideoSubmitReq, VideoTask,
};
use fusion_core::models::{PageQuery, PageResult};

use crate::auth::AuthUser;
use crate::routes::ai::{load_model, resolve_provider_config};
use crate::state::AppState;
use crate::support::{new_task_id, now, ok, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/generation/image/submit", post(submit_image))
        .route("/api/generation/image/{id}", get(get_image_task))
        .route("/api/generation/image/{id}/items", get(image_items))
        .route("/api/generation/image/page", get(image_page))
        .route("/api/generation/image/{id}/cancel", post(cancel_image))
        .route("/api/generation/video/submit", post(submit_video))
        .route("/api/generation/video/{id}", get(get_video_task))
        .route("/api/generation/video/{id}/items", get(video_items))
        .route("/api/generation/video/page", get(video_page))
        .route("/api/generation/video/{id}/cancel", post(cancel_video))
}

fn size_from_request(width: Option<i64>, height: Option<i64>, ratio: Option<&str>) -> Option<String> {
    if let (Some(w), Some(h)) = (width, height) {
        if w > 0 && h > 0 {
            return Some(format!("{w}x{h}"));
        }
    }
    let ratio = ratio.unwrap_or("1:1").trim();
    let size = match ratio {
        "16:9" => (1280, 720),
        "9:16" => (720, 1280),
        "4:3" => (1152, 864),
        "3:4" => (864, 1152),
        "3:2" => (1152, 768),
        "2:3" => (768, 1152),
        "21:9" => (1344, 576),
        _ => (1024, 1024),
    };
    Some(format!("{}x{}", size.0, size.1))
}

// ---------- 图像 ----------

async fn submit_image(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<ImageSubmitReq>,
) -> ApiResult<ImageTask> {
    if req.prompt.trim().is_empty() {
        return Err(AppError::bad_request("提示词不能为空"));
    }
    let model = load_model(&state, req.model_id).await?;
    if model.model_type != 2 {
        return Err(AppError::bad_request("所选模型不是图像模型"));
    }

    let task_id = new_task_id("img");
    let count = req.count.unwrap_or(1).clamp(1, 8);
    let aspect_ratio = req.aspect_ratio.clone().or_else(|| req.ratio.clone());

    let id = sqlx::query(
        "INSERT INTO afv_image_task (task_id, user_id, project_id, prompt, ref_image_urls, ratio, resolution, aspect_ratio, width, height, count, success_count, status, model_id, category, owner_type, owner_id, deleted, create_time, update_time)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?, 1, ?, 0, ?, ?)",
    )
    .bind(&task_id)
    .bind(user.id)
    .bind(req.project_id)
    .bind(req.prompt.trim())
    .bind(req.ref_image_urls.clone())
    .bind(req.ratio.clone())
    .bind(req.resolution.clone())
    .bind(aspect_ratio.clone())
    .bind(req.width)
    .bind(req.height)
    .bind(count)
    .bind(req.model_id)
    .bind(req.category.clone())
    .bind(user.id)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    let task = load_image_task(&state, id).await?;
    let state_clone = state.clone();
    let task_row_id = id;
    let prompt = req.prompt.clone();
    let refs = crate::support::parse_string_list(&req.ref_image_urls);
    let size = size_from_request(req.width, req.height, aspect_ratio.as_deref());

    tokio::spawn(async move {
        run_image_task(state_clone, task_row_id, prompt, refs, size, count).await;
    });

    ok(task)
}

async fn run_image_task(
    state: Arc<AppState>,
    task_row_id: i64,
    prompt: String,
    refs: Vec<String>,
    size: Option<String>,
    count: i64,
) {
    let started = async {
        let task = load_image_task(&state, task_row_id).await?;
        let model_id = task.model_id.ok_or_else(|| AppError::business("任务缺少模型"))?;
        let model = load_model(&state, model_id).await?;
        let (platform, cfg) = resolve_provider_config(&state, &model).await?;
        let provider = state.registry.image(&platform)?;

        set_image_status(&state, task_row_id, task_status::RUNNING, None).await?;

        let request = ImageRequest {
            model: model.code.clone(),
            prompt,
            reference_images: refs,
            size,
            count,
            extra: model
                .config
                .as_ref()
                .and_then(|c| serde_json::from_str(c).ok())
                .unwrap_or(serde_json::Value::Null),
        };

        let result = provider.generate(&cfg, request).await?;

        let client = state.http.clone();
        let mut success = 0i64;
        for url in result.urls {
            if state.is_canceled(&task.task_id) {
                break;
            }
            match state
                .media
                .persist(&client, &url, "images", Some(&cfg.api_key))
                .await
            {
                Ok(local) => {
                    sqlx::query(
                        "INSERT INTO afv_image_item (task_id, platform_task_id, image_url, thumbnail_url, status, deleted, create_time, update_time)
                         VALUES (?, NULL, ?, ?, 2, 0, ?, ?)",
                    )
                    .bind(task_row_id)
                    .bind(&local)
                    .bind(&local)
                    .bind(now())
                    .bind(now())
                    .execute(&state.db)
                    .await?;
                    success += 1;
                }
                Err(err) => {
                    tracing::warn!("图片落盘失败: {err}");
                }
            }
        }

        if success == 0 {
            set_image_status(&state, task_row_id, task_status::FAILED, Some("图片生成失败或落盘失败")).await?;
        } else {
            sqlx::query(
                "UPDATE afv_image_task SET success_count = ?, status = ?, update_time = ? WHERE id = ?",
            )
            .bind(success)
            .bind(task_status::SUCCESS)
            .bind(now())
            .bind(task_row_id)
            .execute(&state.db)
            .await?;
        }
        Ok::<(), AppError>(())
    }
    .await;

    if let Err(err) = started {
        let _ = set_image_status(&state, task_row_id, task_status::FAILED, Some(&err.to_string())).await;
        tracing::warn!("图像任务失败: {err}");
    }
    if let Ok(task) = load_image_task(&state, task_row_id).await {
        state.clear_canceled(&task.task_id);
    }
}

async fn set_image_status(
    state: &AppState,
    id: i64,
    status: i64,
    error: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE afv_image_task SET status = ?, error_msg = ?, update_time = ? WHERE id = ?")
        .bind(status)
        .bind(error)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn load_image_task(state: &AppState, id: i64) -> Result<ImageTask, AppError> {
    sqlx::query_as::<_, ImageTask>("SELECT * FROM afv_image_task WHERE id = ? AND deleted = 0")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("图像任务不存在"))
}

/// 按业务 taskId 或数据库自增 ID 解析任务。
/// 前端 `/{taskId}` 传的是字符串业务 ID，原 Java 版签名即为 `@PathVariable String taskId`，
/// 这里必须收 String，否则业务 ID 会在路由提取阶段直接 400。
async fn resolve_image_task(state: &AppState, key: &str) -> Result<ImageTask, AppError> {
    if let Some(task) = sqlx::query_as::<_, ImageTask>(
        "SELECT * FROM afv_image_task WHERE task_id = ? AND deleted = 0 LIMIT 1",
    )
    .bind(key)
    .fetch_optional(&state.db)
    .await?
    {
        return Ok(task);
    }
    if let Ok(id) = key.parse::<i64>() {
        return load_image_task(state, id).await;
    }
    Err(AppError::not_found("图像任务不存在"))
}

async fn get_image_task(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(key): Path<String>,
) -> ApiResult<ImageTask> {
    ok(resolve_image_task(&state, &key).await?)
}

async fn image_items(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<Vec<ImageItem>> {
    let rows = sqlx::query_as::<_, ImageItem>(
        "SELECT * FROM afv_image_item WHERE task_id = ? AND deleted = 0 ORDER BY id ASC",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn image_page(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<PageQuery>,
) -> ApiResult<PageResult<ImageTask>> {
    let page_no = q.page_no.max(1);
    let page_size = q.page_size.max(1);
    let total: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM afv_image_task WHERE deleted = 0")
        .fetch_one(&state.db)
        .await?;
    let rows = sqlx::query_as::<_, ImageTask>(
        "SELECT * FROM afv_image_task WHERE deleted = 0 ORDER BY id DESC LIMIT ? OFFSET ?",
    )
    .bind(page_size)
    .bind(q.offset())
    .fetch_all(&state.db)
    .await?;
    ok(PageResult::new(rows, total, page_no, page_size))
}

async fn cancel_image(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(key): Path<String>,
) -> ApiResult<bool> {
    let task = resolve_image_task(&state, &key).await?;
    state.mark_canceled(&task.task_id);
    sqlx::query("UPDATE afv_image_task SET status = ?, update_time = ? WHERE id = ? AND status IN (0, 1)")
        .bind(task_status::CANCELED)
        .bind(now())
        .bind(task.id)
        .execute(&state.db)
        .await?;
    ok(true)
}

// ---------- 视频 ----------

async fn submit_video(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<VideoSubmitReq>,
) -> ApiResult<VideoTask> {
    if req.prompt.trim().is_empty() && req.first_frame_image_url.is_none() {
        return Err(AppError::bad_request("提示词或首帧图至少提供一个"));
    }
    let model = load_model(&state, req.model_id).await?;
    if model.model_type != 3 {
        return Err(AppError::bad_request("所选模型不是视频模型"));
    }

    let task_id = new_task_id("vid");
    let count = req.count.unwrap_or(1).clamp(1, 4);
    let id = sqlx::query(
        "INSERT INTO afv_video_task (task_id, user_id, project_id, prompt, generate_mode, first_frame_image_url, last_frame_image_url,
            reference_image_urls, reference_video_urls, reference_audio_urls, ratio, resolution, duration, watermark, generate_audio, seed, camera_fixed,
            count, success_count, status, model_id, category, owner_type, owner_id, deleted, create_time, update_time)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?, 1, ?, 0, ?, ?)",
    )
    .bind(&task_id)
    .bind(user.id)
    .bind(req.project_id)
    .bind(req.prompt.clone())
    .bind(req.generate_mode.clone())
    .bind(req.first_frame_image_url.clone())
    .bind(req.last_frame_image_url.clone())
    .bind(req.reference_image_urls.clone())
    .bind(req.reference_video_urls.clone())
    .bind(req.reference_audio_urls.clone())
    .bind(req.ratio.clone())
    .bind(req.resolution.clone())
    .bind(req.duration)
    .bind(req.watermark.unwrap_or(false))
    .bind(req.generate_audio.unwrap_or(false))
    .bind(req.seed)
    .bind(req.camera_fixed.unwrap_or(false))
    .bind(count)
    .bind(req.model_id)
    .bind(req.category.clone())
    .bind(user.id)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    let task = load_video_task(&state, id).await?;

    let state_clone = state.clone();
    let prompt = req.prompt.clone();
    let first_frame = req.first_frame_image_url.clone();
    let ratio = req.ratio.clone();
    let duration = req.duration;
    let resolution = req.resolution.clone();

    tokio::spawn(async move {
        run_video_task(
            state_clone,
            id,
            prompt,
            first_frame,
            ratio,
            duration,
            resolution,
        )
        .await;
    });

    ok(task)
}

async fn run_video_task(
    state: Arc<AppState>,
    task_row_id: i64,
    prompt: String,
    first_frame: Option<String>,
    ratio: Option<String>,
    duration: Option<i64>,
    resolution: Option<String>,
) {
    let result = async {
        let task = load_video_task(&state, task_row_id).await?;
        let model_id = task.model_id.ok_or_else(|| AppError::business("任务缺少模型"))?;
        let model = load_model(&state, model_id).await?;
        let (platform, cfg) = resolve_provider_config(&state, &model).await?;
        let provider = state.registry.video(&platform)?;

        set_video_status(&state, task_row_id, task_status::RUNNING, None).await?;

        let submit = VideoSubmit {
            model: model.code.clone(),
            prompt,
            image_url: first_frame,
            duration,
            aspect_ratio: ratio,
            resolution,
            extra: serde_json::Value::Null,
        };
        let platform_task_id = provider.submit(&cfg, submit).await?;
        tracing::info!("视频任务已提交: {platform_task_id}");

        // 先落一条占位 item，前端可立即看到"生成中"
        let item_id = sqlx::query(
            "INSERT INTO afv_video_item (task_id, platform_task_id, status, deleted, create_time, update_time)
             VALUES (?, ?, 1, 0, ?, ?)",
        )
        .bind(task_row_id)
        .bind(&platform_task_id)
        .bind(now())
        .bind(now())
        .execute(&state.db)
        .await?
        .last_insert_rowid();

        // 轮询节奏：前 30 秒 2 秒一轮（快速捕捉早完成的任务），之后 5 秒一轮。
        // 用固定节奏而非「请求后再 sleep」，避免单次请求抖动把周期越拖越长。
        let poll_started = std::time::Instant::now();
        for round in 0..480 {
            let round_started = std::time::Instant::now();
            if state.is_canceled(&task.task_id) {
                sqlx::query(
                    "UPDATE afv_video_item SET status = 4, error_msg = '已取消', update_time = ? WHERE id = ?",
                )
                .bind(now())
                .bind(item_id)
                .execute(&state.db)
                .await?;
                set_video_status(&state, task_row_id, task_status::CANCELED, Some("已取消")).await?;
                return Ok::<(), AppError>(());
            }

            match provider.poll(&cfg, &platform_task_id).await {
                Ok(VideoPoll::Done { url, duration: video_duration, .. }) => {
                    let content = provider.content_url(&cfg, &platform_task_id, &url);
                    let local = state
                        .media
                        .persist(&state.http, &content, "videos", Some(&cfg.api_key))
                        .await?;
                    let cover = extract_cover(&state, &local).await;
                    sqlx::query(
                        "UPDATE afv_video_item SET video_url = ?, cover_url = ?, duration = ?, status = 2, update_time = ? WHERE id = ?",
                    )
                    .bind(&local)
                    .bind(&cover)
                    .bind(video_duration)
                    .bind(now())
                    .bind(item_id)
                    .execute(&state.db)
                    .await?;
                    sqlx::query(
                        "UPDATE afv_video_task SET success_count = 1, status = ?, error_msg = NULL, update_time = ? WHERE id = ?",
                    )
                    .bind(task_status::SUCCESS)
                    .bind(now())
                    .bind(task_row_id)
                    .execute(&state.db)
                    .await?;
                    return Ok(());
                }
                Ok(VideoPoll::Failed { message }) => {
                    sqlx::query(
                        "UPDATE afv_video_item SET status = 3, error_msg = ?, update_time = ? WHERE id = ?",
                    )
                    .bind(&message)
                    .bind(now())
                    .bind(item_id)
                    .execute(&state.db)
                    .await?;
                    set_video_status(&state, task_row_id, task_status::FAILED, Some(&message)).await?;
                    return Ok(());
                }
                Ok(VideoPoll::Pending { .. }) => {
                    if round % 10 == 0 {
                        sqlx::query("UPDATE afv_video_task SET update_time = ? WHERE id = ?")
                            .bind(now())
                            .bind(task_row_id)
                            .execute(&state.db)
                            .await?;
                    }
                }
                Err(err) => {
                    tracing::warn!("视频轮询失败: {err}");
                }
            }
            // 固定节奏：下一轮的时间点 = 本轮开始时间 + tick。
            // 请求本身耗时（高丢包链路上偶发数秒）被 tick 吸收，而不是叠加到周期上。
            let tick = if poll_started.elapsed() < Duration::from_secs(30) {
                Duration::from_secs(2)
            } else {
                Duration::from_secs(5)
            };
            if let Some(next_at) = round_started.checked_add(tick) {
                let now = std::time::Instant::now();
                if next_at > now {
                    tokio::time::sleep(next_at - now).await;
                }
            }
            if round % 12 == 0 {
                tracing::info!(
                    "视频轮询第 {round} 轮，已等待 {}s",
                    poll_started.elapsed().as_secs()
                );
            }
        }

        set_video_status(&state, task_row_id, task_status::FAILED, Some("视频生成超时")).await?;
        Ok(())
    }
    .await;

    if let Err(err) = result {
        let _ = set_video_status(&state, task_row_id, task_status::FAILED, Some(&err.to_string())).await;
        tracing::error!("视频任务失败: {err}");
    }
    if let Ok(task) = load_video_task(&state, task_row_id).await {
        state.clear_canceled(&task.task_id);
    }
}

async fn extract_cover(state: &AppState, video_url: &str) -> Option<String> {
    let input = state.media.local_path(video_url)?;
    if !input.exists() {
        return None;
    }
    let name = format!("cover-{}.jpg", uuid::Uuid::new_v4().simple());
    let output = state.media.root().join("covers").join(&name);
    let bin = crate::support::ffmpeg::ffmpeg_bin(&state.config.read().ffmpeg_path);
    match crate::support::ffmpeg::extract_cover(&bin, &input, &output).await {
        Ok(()) => Some(format!("/media/covers/{name}")),
        Err(_) => None,
    }
}

async fn set_video_status(
    state: &AppState,
    id: i64,
    status: i64,
    error: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE afv_video_task SET status = ?, error_msg = ?, update_time = ? WHERE id = ?")
        .bind(status)
        .bind(error)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn load_video_task(state: &AppState, id: i64) -> Result<VideoTask, AppError> {
    sqlx::query_as::<_, VideoTask>("SELECT * FROM afv_video_task WHERE id = ? AND deleted = 0")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("视频任务不存在"))
}

async fn resolve_video_task(state: &AppState, key: &str) -> Result<VideoTask, AppError> {
    if let Some(task) = sqlx::query_as::<_, VideoTask>(
        "SELECT * FROM afv_video_task WHERE task_id = ? AND deleted = 0 LIMIT 1",
    )
    .bind(key)
    .fetch_optional(&state.db)
    .await?
    {
        return Ok(task);
    }
    if let Ok(id) = key.parse::<i64>() {
        return load_video_task(state, id).await;
    }
    Err(AppError::not_found("视频任务不存在"))
}

async fn get_video_task(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(key): Path<String>,
) -> ApiResult<VideoTask> {
    ok(resolve_video_task(&state, &key).await?)
}

async fn video_items(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<Vec<VideoItem>> {
    let rows = sqlx::query_as::<_, VideoItem>(
        "SELECT * FROM afv_video_item WHERE task_id = ? AND deleted = 0 ORDER BY id ASC",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn video_page(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<PageQuery>,
) -> ApiResult<PageResult<VideoTask>> {
    let page_no = q.page_no.max(1);
    let page_size = q.page_size.max(1);
    let total: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM afv_video_task WHERE deleted = 0")
        .fetch_one(&state.db)
        .await?;
    let rows = sqlx::query_as::<_, VideoTask>(
        "SELECT * FROM afv_video_task WHERE deleted = 0 ORDER BY id DESC LIMIT ? OFFSET ?",
    )
    .bind(page_size)
    .bind(q.offset())
    .fetch_all(&state.db)
    .await?;
    ok(PageResult::new(rows, total, page_no, page_size))
}

async fn cancel_video(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(key): Path<String>,
) -> ApiResult<bool> {
    let task = resolve_video_task(&state, &key).await?;
    state.mark_canceled(&task.task_id);
    set_video_status(&state, task.id, task_status::CANCELED, Some("已取消")).await?;
    ok(true)
}

/// 供 AI 工具调用：直接复用后台任务逻辑。
pub async fn run_image_task_public(
    state: Arc<AppState>,
    task_row_id: i64,
    prompt: String,
    refs: Vec<String>,
    width: Option<i64>,
    height: Option<i64>,
    count: i64,
) {
    let size = size_from_request(width, height, None);
    run_image_task(state, task_row_id, prompt, refs, size, count).await;
}

pub async fn run_video_task_public(
    state: Arc<AppState>,
    task_row_id: i64,
    prompt: String,
    first_frame: Option<String>,
    ratio: Option<String>,
    duration: Option<i64>,
) {
    run_video_task(state, task_row_id, prompt, first_frame, ratio, duration, None).await;
}
