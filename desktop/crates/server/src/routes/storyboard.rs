//! 分镜：分镜容器、分集、场次、条目与整集合成。

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use fusion_core::error::AppError;
use fusion_core::models::storyboard::{
    BatchSortReq, BindScriptEpisodeReq, Storyboard, StoryboardEpisode,
    StoryboardEpisodeSaveReq, StoryboardItem, StoryboardItemSaveReq, StoryboardScene,
    StoryboardSceneSaveReq, StoryboardStatistics, StoryboardUpdateReq, UpdateFrameReq,
};

use crate::auth::AuthUser;
use crate::routes::project::statistics_of;
use crate::state::AppState;
use crate::support::{ffmpeg, now, ok, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/storyboard", put(update))
        .route("/api/storyboard/{id}", get(get_storyboard))
        .route("/api/storyboard/project/{project_id}", get(by_project))
        .route("/api/storyboard/{id}/clearContent", post(clear_content))
        .route("/api/storyboard/{id}/statistics", get(statistics))
        .route("/api/storyboard/{storyboard_id}/episodes", get(list_episodes))
        .route(
            "/api/storyboard/episode/{id}",
            get(get_episode).put(save_episode).delete(delete_episode),
        )
        .route("/api/storyboard/episode", post(save_episode))
        .route(
            "/api/storyboard/episode/{id}/bindScriptEpisode",
            put(bind_script_episode),
        )
        .route("/api/storyboard/episode/{id}/clearContent", post(clear_episode))
        .route(
            "/api/storyboard/episode/{id}/compose-video",
            post(compose_video),
        )
        .route("/api/storyboard/episode/{episode_id}/scenes", get(list_scenes_by_episode))
        .route("/api/storyboard/{storyboard_id}/scenes", get(list_scenes))
        .route(
            "/api/storyboard/scene/{id}",
            get(get_scene).put(save_scene).delete(delete_scene),
        )
        .route("/api/storyboard/scene", post(save_scene))
        .route("/api/storyboard/{storyboard_id}/items", get(list_items))
        .route("/api/storyboard/scene/{scene_id}/items", get(list_items_by_scene))
        .route(
            "/api/storyboard/item/{id}",
            get(get_item).put(save_item).delete(delete_item),
        )
        .route("/api/storyboard/item", post(save_item))
        .route("/api/storyboard/item/{id}/updateFrame", put(update_frame))
        .route("/api/storyboard/item/{id}/assets", patch(update_item_assets))
        .route(
            "/api/storyboard/{storyboard_id}/items/batch",
            post(batch_items),
        )
        .route("/api/storyboard/items/batch-sort", post(batch_sort))
}

async fn load_storyboard(state: &AppState, id: i64) -> Result<Storyboard, AppError> {
    sqlx::query_as::<_, Storyboard>("SELECT * FROM afv_storyboard WHERE id = ? AND deleted = 0")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("分镜不存在"))
}

async fn load_episode(state: &AppState, id: i64) -> Result<StoryboardEpisode, AppError> {
    sqlx::query_as::<_, StoryboardEpisode>(
        "SELECT * FROM afv_storyboard_episode WHERE id = ? AND deleted = 0",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("分集不存在"))
}

async fn load_scene(state: &AppState, id: i64) -> Result<StoryboardScene, AppError> {
    sqlx::query_as::<_, StoryboardScene>(
        "SELECT * FROM afv_storyboard_scene WHERE id = ? AND deleted = 0",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("场次不存在"))
}

async fn load_item(state: &AppState, id: i64) -> Result<StoryboardItem, AppError> {
    sqlx::query_as::<_, StoryboardItem>(
        "SELECT * FROM afv_storyboard_item WHERE id = ? AND deleted = 0",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("分镜条目不存在"))
}

async fn get_storyboard(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<Storyboard> {
    ok(load_storyboard(&state, id).await?)
}

async fn by_project(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(project_id): Path<i64>,
) -> ApiResult<Option<Storyboard>> {
    let sb = sqlx::query_as::<_, Storyboard>(
        "SELECT * FROM afv_storyboard WHERE project_id = ? AND deleted = 0 ORDER BY id ASC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    ok(sb)
}

async fn update(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<StoryboardUpdateReq>,
) -> ApiResult<Storyboard> {
    sqlx::query(
        "UPDATE afv_storyboard SET title = COALESCE(?, title), description = COALESCE(?, description),
            script_id = COALESCE(?, script_id), status = COALESCE(?, status),
            custom_columns = COALESCE(?, custom_columns), update_time = ?
         WHERE id = ? AND deleted = 0",
    )
    .bind(req.title)
    .bind(req.description)
    .bind(req.script_id)
    .bind(req.status)
    .bind(req.custom_columns)
    .bind(now())
    .bind(req.id)
    .execute(&state.db)
    .await?;
    ok(load_storyboard(&state, req.id).await?)
}

async fn statistics(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<StoryboardStatistics> {
    ok(statistics_of(&state, id).await?)
}

async fn clear_content(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    clear_storyboard_content(&state, id).await?;
    ok(true)
}

async fn clear_storyboard_content(state: &AppState, storyboard_id: i64) -> Result<(), AppError> {
    sqlx::query("UPDATE afv_storyboard_item SET deleted = 1, update_time = ? WHERE storyboard_id = ?")
        .bind(now())
        .bind(storyboard_id)
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE afv_storyboard_scene SET deleted = 1, update_time = ? WHERE storyboard_id = ?")
        .bind(now())
        .bind(storyboard_id)
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE afv_storyboard_episode SET deleted = 1, update_time = ? WHERE storyboard_id = ?")
        .bind(now())
        .bind(storyboard_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn list_episodes(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(storyboard_id): Path<i64>,
) -> ApiResult<Vec<StoryboardEpisode>> {
    let rows = sqlx::query_as::<_, StoryboardEpisode>(
        "SELECT * FROM afv_storyboard_episode WHERE storyboard_id = ? AND deleted = 0 ORDER BY sort_order ASC, id ASC",
    )
    .bind(storyboard_id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn get_episode(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<StoryboardEpisode> {
    ok(load_episode(&state, id).await?)
}

async fn save_episode(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<StoryboardEpisodeSaveReq>,
) -> ApiResult<StoryboardEpisode> {
    if let Some(id) = req.id {
        sqlx::query(
            "UPDATE afv_storyboard_episode SET script_episode_id = COALESCE(?, script_episode_id),
                episode_number = COALESCE(?, episode_number), title = COALESCE(?, title),
                synopsis = COALESCE(?, synopsis), sort_order = COALESCE(?, sort_order),
                status = COALESCE(?, status), update_time = ?
             WHERE id = ? AND deleted = 0",
        )
        .bind(req.script_episode_id)
        .bind(req.episode_number)
        .bind(req.title)
        .bind(req.synopsis)
        .bind(req.sort_order)
        .bind(req.status)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
        return ok(load_episode(&state, id).await?);
    }

    let sort_order = match req.sort_order {
        Some(v) if v > 0 => v,
        _ => {
            sqlx::query_scalar::<_, i64>(
                "SELECT IFNULL(MAX(sort_order), 0) + 1 FROM afv_storyboard_episode WHERE storyboard_id = ? AND deleted = 0",
            )
            .bind(req.storyboard_id)
            .fetch_one(&state.db)
            .await?
        }
    };
    let episode_number = match req.episode_number {
        Some(v) => v,
        None => {
            sqlx::query_scalar::<_, i64>(
                "SELECT IFNULL(MAX(episode_number), 0) + 1 FROM afv_storyboard_episode WHERE storyboard_id = ? AND deleted = 0",
            )
            .bind(req.storyboard_id)
            .fetch_one(&state.db)
            .await?
        }
    };

    let id = sqlx::query(
        "INSERT INTO afv_storyboard_episode (storyboard_id, script_episode_id, episode_number, title, synopsis, sort_order, status, deleted, create_time, update_time, compose_status)
         VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?, ?, 0)",
    )
    .bind(req.storyboard_id)
    .bind(req.script_episode_id)
    .bind(episode_number)
    .bind(req.title)
    .bind(req.synopsis)
    .bind(sort_order)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    ok(load_episode(&state, id).await?)
}

async fn delete_episode(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_storyboard_episode SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE afv_storyboard_scene SET deleted = 1, update_time = ? WHERE episode_id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    sqlx::query(
        "UPDATE afv_storyboard_item SET deleted = 1, update_time = ? WHERE storyboard_episode_id = ?",
    )
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;
    ok(true)
}

async fn bind_script_episode(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<BindScriptEpisodeReq>,
) -> ApiResult<StoryboardEpisode> {
    sqlx::query(
        "UPDATE afv_storyboard_episode SET script_episode_id = ?, update_time = ? WHERE id = ?",
    )
    .bind(req.script_episode_id)
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;
    ok(load_episode(&state, id).await?)
}

async fn clear_episode(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_storyboard_scene SET deleted = 1, update_time = ? WHERE episode_id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    sqlx::query(
        "UPDATE afv_storyboard_item SET deleted = 1, update_time = ? WHERE storyboard_episode_id = ?",
    )
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;
    sqlx::query(
        "UPDATE afv_storyboard_episode SET composed_video_url = NULL, compose_status = 0, compose_error_msg = NULL, composed_at = NULL, update_time = ? WHERE id = ?",
    )
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;
    ok(true)
}

async fn list_scenes_by_episode(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(episode_id): Path<i64>,
) -> ApiResult<Vec<StoryboardScene>> {
    let rows = sqlx::query_as::<_, StoryboardScene>(
        "SELECT * FROM afv_storyboard_scene WHERE episode_id = ? AND deleted = 0 ORDER BY sort_order ASC, id ASC",
    )
    .bind(episode_id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn list_scenes(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(storyboard_id): Path<i64>,
) -> ApiResult<Vec<StoryboardScene>> {
    let rows = sqlx::query_as::<_, StoryboardScene>(
        "SELECT * FROM afv_storyboard_scene WHERE storyboard_id = ? AND deleted = 0 ORDER BY episode_id ASC, sort_order ASC, id ASC",
    )
    .bind(storyboard_id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn get_scene(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<StoryboardScene> {
    ok(load_scene(&state, id).await?)
}

async fn save_scene(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<StoryboardSceneSaveReq>,
) -> ApiResult<StoryboardScene> {
    if let Some(id) = req.id {
        sqlx::query(
            "UPDATE afv_storyboard_scene SET scene_number = COALESCE(?, scene_number),
                scene_heading = COALESCE(?, scene_heading), location = COALESCE(?, location),
                time_of_day = COALESCE(?, time_of_day), int_ext = COALESCE(?, int_ext),
                sort_order = COALESCE(?, sort_order), status = COALESCE(?, status), update_time = ?
             WHERE id = ? AND deleted = 0",
        )
        .bind(req.scene_number)
        .bind(req.scene_heading)
        .bind(req.location)
        .bind(req.time_of_day)
        .bind(req.int_ext)
        .bind(req.sort_order)
        .bind(req.status)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
        return ok(load_scene(&state, id).await?);
    }

    let sort_order = match req.sort_order {
        Some(v) if v > 0 => v,
        _ => {
            sqlx::query_scalar::<_, i64>(
                "SELECT IFNULL(MAX(sort_order), 0) + 1 FROM afv_storyboard_scene WHERE episode_id = ? AND deleted = 0",
            )
            .bind(req.episode_id)
            .fetch_one(&state.db)
            .await?
        }
    };

    let id = sqlx::query(
        "INSERT INTO afv_storyboard_scene (episode_id, storyboard_id, scene_number, scene_heading, location, time_of_day, int_ext, sort_order, status, deleted, create_time, update_time)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(req.episode_id)
    .bind(req.storyboard_id)
    .bind(req.scene_number)
    .bind(req.scene_heading)
    .bind(req.location)
    .bind(req.time_of_day)
    .bind(req.int_ext)
    .bind(sort_order)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    ok(load_scene(&state, id).await?)
}

async fn delete_scene(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_storyboard_scene SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn list_items(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(storyboard_id): Path<i64>,
) -> ApiResult<Vec<StoryboardItem>> {
    let rows = sqlx::query_as::<_, StoryboardItem>(
        "SELECT * FROM afv_storyboard_item WHERE storyboard_id = ? AND deleted = 0 ORDER BY storyboard_episode_id ASC, sort_order ASC, id ASC",
    )
    .bind(storyboard_id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn list_items_by_scene(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(scene_id): Path<i64>,
) -> ApiResult<Vec<StoryboardItem>> {
    let rows = sqlx::query_as::<_, StoryboardItem>(
        "SELECT * FROM afv_storyboard_item WHERE storyboard_scene_id = ? AND deleted = 0 ORDER BY sort_order ASC, id ASC",
    )
    .bind(scene_id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn get_item(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<StoryboardItem> {
    ok(load_item(&state, id).await?)
}

async fn save_item(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<StoryboardItemSaveReq>,
) -> ApiResult<StoryboardItem> {
    if let Some(id) = req.id {
        apply_item_update(&state, id, &req).await?;
        return ok(load_item(&state, id).await?);
    }

    let sort_order = match req.sort_order {
        Some(v) if v > 0 => v,
        _ => {
            sqlx::query_scalar::<_, i64>(
                "SELECT IFNULL(MAX(sort_order), 0) + 1 FROM afv_storyboard_item WHERE storyboard_id = ? AND deleted = 0",
            )
            .bind(req.storyboard_id)
            .fetch_one(&state.db)
            .await?
        }
    };

    let id = sqlx::query(
        "INSERT INTO afv_storyboard_item (storyboard_id, storyboard_episode_id, storyboard_scene_id, sort_order, shot_number, content, duration, status, deleted, create_time, update_time, ai_generated)
         VALUES (?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?, 0)",
    )
    .bind(req.storyboard_id)
    .bind(req.storyboard_episode_id)
    .bind(req.storyboard_scene_id)
    .bind(sort_order)
    .bind(req.shot_number.clone())
    .bind(req.content.clone())
    .bind(req.duration)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    // 其余可选字段一次性补齐
    let _ = apply_item_update(&state, id, &req).await;

    ok(load_item(&state, id).await?)
}

async fn apply_item_update(
    state: &AppState,
    id: i64,
    req: &StoryboardItemSaveReq,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE afv_storyboard_item SET
            storyboard_episode_id = COALESCE(?, storyboard_episode_id),
            storyboard_scene_id = COALESCE(?, storyboard_scene_id),
            sort_order = COALESCE(NULLIF(?, 0), sort_order),
            shot_number = COALESCE(?, shot_number),
            auto_shot_number = COALESCE(?, auto_shot_number),
            image_url = COALESCE(?, image_url),
            reference_image_url = COALESCE(?, reference_image_url),
            video_url = COALESCE(?, video_url),
            generated_image_url = COALESCE(?, generated_image_url),
            first_frame_image_url = COALESCE(?, first_frame_image_url),
            last_frame_image_url = COALESCE(?, last_frame_image_url),
            first_frame_prompt = COALESCE(?, first_frame_prompt),
            last_frame_prompt = COALESCE(?, last_frame_prompt),
            generated_video_url = COALESCE(?, generated_video_url),
            video_prompt = COALESCE(?, video_prompt),
            shot_type = COALESCE(?, shot_type),
            duration = COALESCE(?, duration),
            content = COALESCE(?, content),
            scene_expectation = COALESCE(?, scene_expectation),
            sound = COALESCE(?, sound),
            dialogue = COALESCE(?, dialogue),
            sound_effect = COALESCE(?, sound_effect),
            music = COALESCE(?, music),
            camera_movement = COALESCE(?, camera_movement),
            camera_angle = COALESCE(?, camera_angle),
            camera_equipment = COALESCE(?, camera_equipment),
            focal_length = COALESCE(?, focal_length),
            transition = COALESCE(?, transition),
            character_ids = COALESCE(?, character_ids),
            scene_asset_item_id = COALESCE(?, scene_asset_item_id),
            prop_ids = COALESCE(?, prop_ids),
            remark = COALESCE(?, remark),
            custom_data = COALESCE(?, custom_data),
            ai_generated = COALESCE(?, ai_generated),
            status = COALESCE(?, status),
            update_time = ?
         WHERE id = ? AND deleted = 0",
    )
    .bind(req.storyboard_episode_id)
    .bind(req.storyboard_scene_id)
    .bind(req.sort_order)
    .bind(req.shot_number.clone())
    .bind(req.auto_shot_number.clone())
    .bind(req.image_url.clone())
    .bind(req.reference_image_url.clone())
    .bind(req.video_url.clone())
    .bind(req.generated_image_url.clone())
    .bind(req.first_frame_image_url.clone())
    .bind(req.last_frame_image_url.clone())
    .bind(req.first_frame_prompt.clone())
    .bind(req.last_frame_prompt.clone())
    .bind(req.generated_video_url.clone())
    .bind(req.video_prompt.clone())
    .bind(req.shot_type.clone())
    .bind(req.duration)
    .bind(req.content.clone())
    .bind(req.scene_expectation.clone())
    .bind(req.sound.clone())
    .bind(req.dialogue.clone())
    .bind(req.sound_effect.clone())
    .bind(req.music.clone())
    .bind(req.camera_movement.clone())
    .bind(req.camera_angle.clone())
    .bind(req.camera_equipment.clone())
    .bind(req.focal_length.clone())
    .bind(req.transition.clone())
    .bind(req.character_ids.clone())
    .bind(req.scene_asset_item_id)
    .bind(req.prop_ids.clone())
    .bind(req.remark.clone())
    .bind(req.custom_data.clone())
    .bind(req.ai_generated)
    .bind(req.status)
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// 区分「字段缺省」与「显式 null」：缺省返回 None，null 返回 Some(None)。
fn double_opt_i64<'de, D>(de: D) -> Result<Option<Option<i64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    Ok(Some(Option::<i64>::deserialize(de)?))
}

/// 局部更新分镜条目的资产关联：字段缺省=不修改，空数组=清空，null=清空。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAssetsReq {
    #[serde(default)]
    character_ids: Option<Vec<i64>>,
    #[serde(default, deserialize_with = "double_opt_i64")]
    scene_asset_item_id: Option<Option<i64>>,
    #[serde(default)]
    prop_ids: Option<Vec<i64>>,
}

async fn update_item_assets(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<UpdateAssetsReq>,
) -> ApiResult<StoryboardItem> {
    // 逐个字段拼 UPDATE，避免「没传的字段被清空」。
    let mut sets: Vec<&str> = Vec::new();
    let mut char_json: Option<String> = None;
    let mut prop_json: Option<String> = None;
    let mut scene_asset: Option<Option<i64>> = req.scene_asset_item_id;

    if let Some(ids) = &req.character_ids {
        sets.push("character_ids = ?");
        char_json = Some(if ids.is_empty() { "[]".to_string() } else { serde_json::to_string(ids)? });
    }
    if req.prop_ids.is_some() {
        sets.push("prop_ids = ?");
        prop_json = Some(match &req.prop_ids {
            Some(ids) if !ids.is_empty() => serde_json::to_string(ids)?,
            _ => "[]".to_string(),
        });
    }
    if scene_asset.is_some() {
        sets.push("scene_asset_item_id = ?");
    }

    if sets.is_empty() {
        return ok(load_item(&state, id).await?);
    }
    sets.push("update_time = ?");

    let sql = format!(
        "UPDATE afv_storyboard_item SET {} WHERE id = ?",
        sets.join(", ")
    );
    let mut q = sqlx::query(&sql);
    if let Some(v) = char_json { q = q.bind(v); }
    if let Some(v) = prop_json { q = q.bind(v); }
    if let Some(v) = scene_asset.take() { q = q.bind(v); }
    q.bind(now()).bind(id).execute(&state.db).await?;

    ok(load_item(&state, id).await?)
}

async fn update_frame(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<UpdateFrameReq>,
) -> ApiResult<StoryboardItem> {
    let frame_type = req.frame_type.unwrap_or_else(|| "first".into());
    if frame_type == "last" {
        sqlx::query(
            "UPDATE afv_storyboard_item SET last_frame_image_url = ?, last_frame_prompt = COALESCE(?, last_frame_prompt), update_time = ? WHERE id = ?",
        )
        .bind(req.image_url)
        .bind(req.prompt)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query(
            "UPDATE afv_storyboard_item SET first_frame_image_url = ?, first_frame_prompt = COALESCE(?, first_frame_prompt), update_time = ? WHERE id = ?",
        )
        .bind(req.image_url)
        .bind(req.prompt)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    }
    ok(load_item(&state, id).await?)
}

async fn delete_item(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_storyboard_item SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn batch_items(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(_storyboard_id): Path<i64>,
    Json(req): Json<Vec<StoryboardItemSaveReq>>,
) -> ApiResult<bool> {
    for item in req {
        if let Some(id) = item.id {
            apply_item_update(&state, id, &item).await?;
        }
    }
    ok(true)
}

async fn batch_sort(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<BatchSortReq>,
) -> ApiResult<bool> {
    for (idx, id) in req.ids.iter().enumerate() {
        sqlx::query("UPDATE afv_storyboard_item SET sort_order = ?, update_time = ? WHERE id = ?")
            .bind(idx as i64 + 1)
            .bind(now())
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    ok(true)
}

/// 整集合成：按分镜顺序拼接各条目视频。
async fn compose_video(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(episode_id): Path<i64>,
) -> ApiResult<String> {
    let _episode = load_episode(&state, episode_id).await?;
    let rows = sqlx::query_as::<_, StoryboardItem>(
        "SELECT * FROM afv_storyboard_item WHERE storyboard_episode_id = ? AND deleted = 0 ORDER BY sort_order ASC, id ASC",
    )
    .bind(episode_id)
    .fetch_all(&state.db)
    .await?;

    let mut inputs = Vec::new();
    for item in &rows {
        let url = item
            .generated_video_url
            .clone()
            .or_else(|| item.video_url.clone());
        if let Some(url) = url {
            if let Some(path) = state.media.local_path(&url) {
                if path.exists() {
                    inputs.push(path);
                }
            }
        }
    }

    if inputs.is_empty() {
        return Err(AppError::bad_request("该集还没有可合成的视频片段，请先生成分镜视频"));
    }

    sqlx::query("UPDATE afv_storyboard_episode SET compose_status = 1, compose_error_msg = NULL, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(episode_id)
        .execute(&state.db)
        .await?;

    let ffmpeg_bin = ffmpeg::ffmpeg_bin(&state.config.read().ffmpeg_path);
    let file_name = format!("episode-{}-{}.mp4", episode_id, uuid::Uuid::new_v4().simple());
    let output = state.media.root().join("videos").join(&file_name);

    let result = ffmpeg::concat_videos(&ffmpeg_bin, &inputs, &output).await;
    match result {
        Ok(()) => {
            let public_url = format!("/media/videos/{file_name}");
            let ffprobe = ffmpeg::ffprobe_bin(&state.config.read().ffmpeg_path);
            let duration = ffmpeg::probe_duration(&ffprobe, &output).await.unwrap_or(0.0) as i64;
            sqlx::query(
                "UPDATE afv_storyboard_episode SET composed_video_url = ?, compose_status = 2, composed_at = ?, update_time = ? WHERE id = ?",
            )
            .bind(&public_url)
            .bind(now())
            .bind(now())
            .bind(episode_id)
            .execute(&state.db)
            .await?;
            let _ = sqlx::query(
                "UPDATE afv_storyboard_episode SET synopsis = COALESCE(synopsis, ''), update_time = ? WHERE id = ?",
            )
            .bind(now())
            .bind(episode_id)
            .execute(&state.db)
            .await;
            let _ = duration;
            ok(public_url)
        }
        Err(err) => {
            sqlx::query(
                "UPDATE afv_storyboard_episode SET compose_status = 3, compose_error_msg = ?, update_time = ? WHERE id = ?",
            )
            .bind(err.to_string())
            .bind(now())
            .bind(episode_id)
            .execute(&state.db)
            .await?;
            Err(err)
        }
    }
}
