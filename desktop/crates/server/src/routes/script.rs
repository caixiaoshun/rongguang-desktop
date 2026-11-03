//! 剧本、分集与场次。

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};

use fusion_core::error::AppError;
use fusion_core::models::script::{
    Script, ScriptEpisode, ScriptEpisodeSaveReq, ScriptSceneItem, ScriptSceneSaveReq,
    ScriptSourceUpdateReq, ScriptUpdateReq,
};

use crate::auth::AuthUser;
use crate::state::AppState;
use crate::support::{now, ok, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/script", put(update))
        .route("/api/script/{id}", get(get_script).delete(delete_script))
        .route("/api/script/{id}/source", put(update_source))
        .route("/api/script/project/{project_id}", get(by_project))
        .route("/api/script/{script_id}/episodes", get(list_episodes))
        .route("/api/script/episode/{id}", get(get_episode).delete(delete_episode))
        .route("/api/script/episode", post(save_episode).put(save_episode))
        .route("/api/script/episode/{episode_id}/scenes", get(list_scenes))
        .route("/api/script/scene/{id}", get(get_scene).delete(delete_scene))
        .route("/api/script/scene", post(save_scene).put(save_scene))
}

pub async fn load_script(state: &AppState, id: i64) -> Result<Script, AppError> {
    sqlx::query_as::<_, Script>("SELECT * FROM afv_script WHERE id = ? AND deleted = 0")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("剧本不存在"))
}

async fn fetch_episode(state: &AppState, id: i64) -> Result<ScriptEpisode, AppError> {
    sqlx::query_as::<_, ScriptEpisode>(
        "SELECT * FROM afv_script_episode WHERE id = ? AND deleted = 0",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("分集不存在"))
}

async fn fetch_scene(state: &AppState, id: i64) -> Result<ScriptSceneItem, AppError> {
    sqlx::query_as::<_, ScriptSceneItem>(
        "SELECT * FROM afv_script_scene_item WHERE id = ? AND deleted = 0",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("场次不存在"))
}

async fn get_script(State(state): State<Arc<AppState>>, _u: AuthUser, Path(id): Path<i64>) -> ApiResult<Script> {
    ok(load_script(&state, id).await?)
}

async fn by_project(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(project_id): Path<i64>,
) -> ApiResult<Option<Script>> {
    let script = sqlx::query_as::<_, Script>(
        "SELECT * FROM afv_script WHERE project_id = ? AND deleted = 0 ORDER BY id ASC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    ok(script)
}

async fn update(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<ScriptUpdateReq>,
) -> ApiResult<Script> {
    let characters = req.characters_json.map(|v| v.to_string());
    sqlx::query(
        "UPDATE afv_script SET
            title = COALESCE(?, title),
            content = COALESCE(?, content),
            raw_content = COALESCE(?, raw_content),
            story_synopsis = COALESCE(?, story_synopsis),
            summary = COALESCE(?, summary),
            genre = COALESCE(?, genre),
            target_audience = COALESCE(?, target_audience),
            total_episodes = COALESCE(?, total_episodes),
            duration_estimate = COALESCE(?, duration_estimate),
            status = COALESCE(?, status),
            characters_json = COALESCE(?, characters_json),
            version = version + 1,
            update_time = ?
         WHERE id = ? AND deleted = 0",
    )
    .bind(req.title)
    .bind(req.content)
    .bind(req.raw_content)
    .bind(req.story_synopsis)
    .bind(req.summary)
    .bind(req.genre)
    .bind(req.target_audience)
    .bind(req.total_episodes)
    .bind(req.duration_estimate)
    .bind(req.status)
    .bind(characters)
    .bind(now())
    .bind(req.id)
    .execute(&state.db)
    .await?;
    ok(load_script(&state, req.id).await?)
}

async fn update_source(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<ScriptSourceUpdateReq>,
) -> ApiResult<Script> {
    sqlx::query(
        "UPDATE afv_script SET raw_content = COALESCE(?, raw_content), content = COALESCE(?, content),
            title = COALESCE(?, title), source_type = COALESCE(?, source_type), update_time = ?
         WHERE id = ? AND deleted = 0",
    )
    .bind(req.raw_content)
    .bind(req.content)
    .bind(req.title)
    .bind(req.source_type)
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;
    ok(load_script(&state, id).await?)
}

async fn delete_script(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_script SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE afv_script_episode SET deleted = 1, update_time = ? WHERE script_id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE afv_script_scene_item SET deleted = 1, update_time = ? WHERE script_id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn list_episodes(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(script_id): Path<i64>,
) -> ApiResult<Vec<ScriptEpisode>> {
    let rows = sqlx::query_as::<_, ScriptEpisode>(
        "SELECT * FROM afv_script_episode WHERE script_id = ? AND deleted = 0 ORDER BY sort_order ASC, id ASC",
    )
    .bind(script_id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn get_episode(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<ScriptEpisode> {
    ok(fetch_episode(&state, id).await?)
}

async fn save_episode(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<ScriptEpisodeSaveReq>,
) -> ApiResult<ScriptEpisode> {
    if let Some(id) = req.id {
        sqlx::query(
            "UPDATE afv_script_episode SET episode_number = COALESCE(?, episode_number), title = COALESCE(?, title),
                synopsis = COALESCE(?, synopsis), raw_content = COALESCE(?, raw_content),
                duration_estimate = COALESCE(?, duration_estimate), sort_order = COALESCE(?, sort_order),
                status = COALESCE(?, status), version = version + 1, update_time = ?
             WHERE id = ? AND deleted = 0",
        )
        .bind(req.episode_number)
        .bind(req.title)
        .bind(req.synopsis)
        .bind(req.raw_content)
        .bind(req.duration_estimate)
        .bind(req.sort_order)
        .bind(req.status)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
        return ok(fetch_episode(&state, id).await?);
    }

    let sort_order = req.sort_order.unwrap_or_else(|| 0);
    let sort_order = if sort_order == 0 {
        sqlx::query_scalar::<_, i64>(
            "SELECT IFNULL(MAX(sort_order), 0) + 1 FROM afv_script_episode WHERE script_id = ? AND deleted = 0",
        )
        .bind(req.script_id)
        .fetch_one(&state.db)
        .await?
    } else {
        sort_order
    };
    let episode_number = match req.episode_number {
        Some(n) => n,
        None => {
            sqlx::query_scalar::<_, i64>(
                "SELECT IFNULL(MAX(episode_number), 0) + 1 FROM afv_script_episode WHERE script_id = ? AND deleted = 0",
            )
            .bind(req.script_id)
            .fetch_one(&state.db)
            .await?
        }
    };

    let id = sqlx::query(
        "INSERT INTO afv_script_episode (script_id, episode_number, title, synopsis, raw_content, duration_estimate, total_scenes, source_type, sort_order, parsing_status, status, version, deleted, create_time, update_time)
         VALUES (?, ?, ?, ?, ?, ?, 0, 1, ?, 0, 0, 0, 0, ?, ?)",
    )
    .bind(req.script_id)
    .bind(episode_number)
    .bind(req.title)
    .bind(req.synopsis)
    .bind(req.raw_content)
    .bind(req.duration_estimate)
    .bind(sort_order)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    // 同步剧本集数字段
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM afv_script_episode WHERE script_id = ? AND deleted = 0",
    )
    .bind(req.script_id)
    .fetch_one(&state.db)
    .await?;
    let _ = sqlx::query("UPDATE afv_script SET total_episodes = ?, update_time = ? WHERE id = ?")
        .bind(total)
        .bind(now())
        .bind(req.script_id)
        .execute(&state.db)
        .await;

    ok(fetch_episode(&state, id).await?)
}

async fn delete_episode(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_script_episode SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE afv_script_scene_item SET deleted = 1, update_time = ? WHERE episode_id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn list_scenes(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(episode_id): Path<i64>,
) -> ApiResult<Vec<ScriptSceneItem>> {
    let rows = sqlx::query_as::<_, ScriptSceneItem>(
        "SELECT * FROM afv_script_scene_item WHERE episode_id = ? AND deleted = 0 ORDER BY sort_order ASC, id ASC",
    )
    .bind(episode_id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn get_scene(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<ScriptSceneItem> {
    ok(fetch_scene(&state, id).await?)
}

async fn save_scene(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<ScriptSceneSaveReq>,
) -> ApiResult<ScriptSceneItem> {
    let script_id = match req.script_id {
        Some(id) => id,
        None => sqlx::query_scalar("SELECT script_id FROM afv_script_episode WHERE id = ?")
            .bind(req.episode_id)
            .fetch_one(&state.db)
            .await?,
    };
    let characters = req.characters.as_ref().map(|v| v.to_string());
    let character_asset_ids = req.character_asset_ids.as_ref().map(|v| v.to_string());
    let prop_asset_ids = req.prop_asset_ids.as_ref().map(|v| v.to_string());
    let dialogues = req.dialogues.as_ref().map(|v| v.to_string());

    if let Some(id) = req.id {
        sqlx::query(
            "UPDATE afv_script_scene_item SET scene_number = COALESCE(?, scene_number), scene_heading = COALESCE(?, scene_heading),
                location = COALESCE(?, location), time_of_day = COALESCE(?, time_of_day), int_ext = COALESCE(?, int_ext),
                characters = COALESCE(?, characters), character_asset_ids = COALESCE(?, character_asset_ids),
                scene_asset_id = COALESCE(?, scene_asset_id), prop_asset_ids = COALESCE(?, prop_asset_ids),
                scene_description = COALESCE(?, scene_description), dialogues = COALESCE(?, dialogues),
                sort_order = COALESCE(?, sort_order), status = COALESCE(?, status), version = version + 1, update_time = ?
             WHERE id = ? AND deleted = 0",
        )
        .bind(req.scene_number)
        .bind(req.scene_heading)
        .bind(req.location)
        .bind(req.time_of_day)
        .bind(req.int_ext)
        .bind(characters)
        .bind(character_asset_ids)
        .bind(req.scene_asset_id)
        .bind(prop_asset_ids)
        .bind(req.scene_description)
        .bind(dialogues)
        .bind(req.sort_order)
        .bind(req.status)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
        return ok(fetch_scene(&state, id).await?);
    }

    let sort_order = match req.sort_order {
        Some(v) if v > 0 => v,
        _ => {
            sqlx::query_scalar::<_, i64>(
                "SELECT IFNULL(MAX(sort_order), 0) + 1 FROM afv_script_scene_item WHERE episode_id = ? AND deleted = 0",
            )
            .bind(req.episode_id)
            .fetch_one(&state.db)
            .await?
        }
    };

    let id = sqlx::query(
        "INSERT INTO afv_script_scene_item (episode_id, script_id, scene_number, scene_heading, location, time_of_day, int_ext,
            characters, character_asset_ids, scene_asset_id, prop_asset_ids, scene_description, dialogues, sort_order, status, version, deleted, create_time, update_time)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, 0, ?, ?)",
    )
    .bind(req.episode_id)
    .bind(script_id)
    .bind(req.scene_number)
    .bind(req.scene_heading)
    .bind(req.location)
    .bind(req.time_of_day)
    .bind(req.int_ext)
    .bind(characters)
    .bind(character_asset_ids)
    .bind(req.scene_asset_id)
    .bind(prop_asset_ids)
    .bind(req.scene_description)
    .bind(dialogues)
    .bind(sort_order)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM afv_script_scene_item WHERE episode_id = ? AND deleted = 0",
    )
    .bind(req.episode_id)
    .fetch_one(&state.db)
    .await?;
    let _ = sqlx::query("UPDATE afv_script_episode SET total_scenes = ?, update_time = ? WHERE id = ?")
        .bind(total)
        .bind(now())
        .bind(req.episode_id)
        .execute(&state.db)
        .await;

    ok(fetch_scene(&state, id).await?)
}

async fn delete_scene(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_script_scene_item SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    ok(true)
}
