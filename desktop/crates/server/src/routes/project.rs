//! 项目、项目成员与画风预设。

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use fusion_core::error::AppError;
use fusion_core::models::project::{
    ArtStylePreset, Project, ProjectCreateReq, ProjectMember, ProjectMemberAddReq, ProjectUpdateReq,
};
use fusion_core::models::script::Script;
use fusion_core::models::storyboard::{Storyboard, StoryboardStatistics};
use fusion_core::models::{PageQuery, PageResult};

use crate::auth::AuthUser;
use crate::state::AppState;
use crate::support::{now, ok, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/project", post(create).put(update))
        .route("/api/project/list", get(list))
        .route("/api/project/page", get(page))
        .route("/api/project/{id}", get(detail).delete(remove))
        .route("/api/project/{id}/workspace-overview", get(workspace_overview))
        .route(
            "/api/project/{id}/workspace/initialize",
            post(initialize_workspace),
        )
        .route("/api/project/{project_id}/members", get(member_list).post(member_add))
        .route(
            "/api/project/{project_id}/members/{user_id}",
            axum::routing::delete(member_remove),
        )
        .route("/api/project/presets/art-styles", get(art_styles))
}

/// 内置画风预设，与 Java 版 `art-styles` 静态资源一致。
pub const ART_STYLES: &[(&str, &str, &str, &str)] = &[
    ("anime_cn", "国漫风", "中国动画电影质感，细腻光影与东方审美", "Chinese animation film style, delicate lighting, oriental aesthetics"),
    ("anime_jp", "日系动画", "日本 TV 动画质感，清晰线条与明快色彩", "Japanese anime style, clean line art, bright colors"),
    ("cg", "CG 写实", "电影级 CG 渲染，真实材质与全局光照", "cinematic CG render, realistic materials, global illumination"),
    ("realistic", "真人写实", "写实摄影质感，自然肤色与真实景深", "photorealistic, natural skin tones, realistic depth of field"),
    ("ink", "水墨国风", "中国水墨画质感，留白与晕染", "Chinese ink wash painting, negative space, soft bleeding edges"),
    ("pixel", "像素风", "复古像素游戏美术", "retro pixel art game style"),
];

async fn create(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<ProjectCreateReq>,
) -> ApiResult<Project> {
    if req.name.trim().is_empty() {
        return Err(AppError::bad_request("项目名称不能为空"));
    }
    let id = sqlx::query(
        "INSERT INTO afv_project (name, description, cover_url, scope, owner_type, owner_id, status, properties, art_style, art_style_description, art_style_image_prompt, art_style_image_url, deleted, create_time, update_time)
         VALUES (?, ?, ?, 2, 1, ?, 0, ?, ?, ?, ?, ?, 0, ?, ?)",
    )
    .bind(req.name.trim())
    .bind(req.description)
    .bind(req.cover_url)
    .bind(user.id)
    .bind(req.properties)
    .bind(req.art_style)
    .bind(req.art_style_description)
    .bind(req.art_style_image_prompt)
    .bind(req.art_style_image_url)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    ensure_workspace(&state, id, user.id).await?;
    let _ = sqlx::query(
        "INSERT INTO afv_project_member (project_id, user_id, role, deleted, create_time, update_time) VALUES (?, ?, 1, 0, ?, ?)",
    )
    .bind(id)
    .bind(user.id)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await;

    ok(load_project(&state, id).await?)
}

async fn update(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<ProjectUpdateReq>,
) -> ApiResult<Project> {
    sqlx::query(
        "UPDATE afv_project SET
            name = COALESCE(?, name),
            description = COALESCE(?, description),
            cover_url = COALESCE(?, cover_url),
            status = COALESCE(?, status),
            properties = COALESCE(?, properties),
            art_style = COALESCE(?, art_style),
            art_style_description = COALESCE(?, art_style_description),
            art_style_image_prompt = COALESCE(?, art_style_image_prompt),
            art_style_image_url = COALESCE(?, art_style_image_url),
            update_time = ?
         WHERE id = ? AND deleted = 0",
    )
    .bind(req.name)
    .bind(req.description)
    .bind(req.cover_url)
    .bind(req.status)
    .bind(req.properties)
    .bind(req.art_style)
    .bind(req.art_style_description)
    .bind(req.art_style_image_prompt)
    .bind(req.art_style_image_url)
    .bind(now())
    .bind(req.id)
    .execute(&state.db)
    .await?;
    ok(load_project(&state, req.id).await?)
}

async fn list(State(state): State<Arc<AppState>>, _user: AuthUser) -> ApiResult<Vec<Project>> {
    let rows = sqlx::query_as::<_, Project>(
        "SELECT * FROM afv_project WHERE deleted = 0 ORDER BY update_time DESC, id DESC",
    )
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn page(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<PageQuery>,
) -> ApiResult<PageResult<Project>> {
    let page_no = q.page_no.max(1);
    let page_size = q.page_size.max(1);
    let keyword = format!("%{}%", q.keyword.clone().unwrap_or_default());
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(1) FROM afv_project WHERE deleted = 0 AND name LIKE ?")
            .bind(&keyword)
            .fetch_one(&state.db)
            .await?;
    let rows = sqlx::query_as::<_, Project>(
        "SELECT * FROM afv_project WHERE deleted = 0 AND name LIKE ? ORDER BY update_time DESC LIMIT ? OFFSET ?",
    )
    .bind(&keyword)
    .bind(page_size)
    .bind(q.offset())
    .fetch_all(&state.db)
    .await?;
    ok(PageResult::new(rows, total, page_no, page_size))
}

async fn detail(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<Project> {
    ok(load_project(&state, id).await?)
}

async fn remove(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_project SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    for table in [
        "afv_script",
        "afv_storyboard",
        "afv_asset",
    ] {
        let sql = format!("UPDATE {table} SET deleted = 1, update_time = ? WHERE project_id = ?");
        let _ = sqlx::query(&sql).bind(now()).bind(id).execute(&state.db).await;
    }
    ok(true)
}

pub async fn load_project(state: &AppState, id: i64) -> Result<Project, AppError> {
    sqlx::query_as::<_, Project>("SELECT * FROM afv_project WHERE id = ? AND deleted = 0")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))
}

/// 幂等补齐项目的剧本与分镜容器。
pub async fn ensure_workspace(state: &AppState, project_id: i64, owner_id: i64) -> Result<(), AppError> {
    let project = load_project(state, project_id).await?;

    let script_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM afv_script WHERE project_id = ? AND deleted = 0 ORDER BY id ASC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;

    let script_id = match script_id {
        Some(id) => id,
        None => {
            sqlx::query(
                "INSERT INTO afv_script (project_id, title, total_episodes, source_type, parsing_status, scope, owner_type, owner_id, ai_generated, version, status, deleted, create_time, update_time)
                 VALUES (?, ?, 0, 0, 0, 3, 1, ?, 0, 0, 0, 0, ?, ?)",
            )
            .bind(project_id)
            .bind(format!("{} 剧本", project.name))
            .bind(owner_id)
            .bind(now())
            .bind(now())
            .execute(&state.db)
            .await?
            .last_insert_rowid()
        }
    };

    let storyboard_exists: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM afv_storyboard WHERE project_id = ? AND deleted = 0 ORDER BY id ASC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;

    if storyboard_exists.is_none() {
        sqlx::query(
            "INSERT INTO afv_storyboard (project_id, script_id, title, scope, owner_type, owner_id, total_duration, status, deleted, create_time, update_time)
             VALUES (?, ?, ?, 3, 1, ?, 0, 0, 0, ?, ?)",
        )
        .bind(project_id)
        .bind(script_id)
        .bind(format!("{} 分镜", project.name))
        .bind(owner_id)
        .bind(now())
        .bind(now())
        .execute(&state.db)
        .await?;
    }

    Ok(())
}

async fn initialize_workspace(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    ensure_workspace(&state, id, user.id).await?;
    ok(workspace_payload(&state, id).await?)
}

async fn workspace_overview(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    // 老项目缺容器时自动补齐，保证前端一次请求即可拿到完整工作区。
    let script_missing: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM afv_script WHERE project_id = ? AND deleted = 0",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if script_missing == 0 {
        ensure_workspace(&state, id, 1).await?;
    }
    ok(workspace_payload(&state, id).await?)
}

async fn workspace_payload(state: &AppState, project_id: i64) -> Result<serde_json::Value, AppError> {
    let script = sqlx::query_as::<_, Script>(
        "SELECT * FROM afv_script WHERE project_id = ? AND deleted = 0 ORDER BY id ASC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;

    let storyboard = sqlx::query_as::<_, Storyboard>(
        "SELECT * FROM afv_storyboard WHERE project_id = ? AND deleted = 0 ORDER BY id ASC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;

    let mut episode_count = 0;
    let mut scene_count = 0;
    if let Some(script) = &script {
        episode_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(1) FROM afv_script_episode WHERE script_id = ? AND deleted = 0",
        )
        .bind(script.id)
        .fetch_one(&state.db)
        .await?;
        scene_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(1) FROM afv_script_scene_item WHERE script_id = ? AND deleted = 0",
        )
        .bind(script.id)
        .fetch_one(&state.db)
        .await?;
    }

    let statistics = match &storyboard {
        Some(sb) => statistics_of(state, sb.id).await?,
        None => StoryboardStatistics {
            episode_count: 0,
            scene_count: 0,
            item_count: 0,
        },
    };

    Ok(json!({
        "script": script,
        "scriptEpisodeCount": episode_count,
        "scriptSceneCount": scene_count,
        "storyboard": storyboard,
        "storyboardStatistics": statistics,
    }))
}

pub async fn statistics_of(state: &AppState, storyboard_id: i64) -> Result<StoryboardStatistics, AppError> {
    let episode_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM afv_storyboard_episode WHERE storyboard_id = ? AND deleted = 0",
    )
    .bind(storyboard_id)
    .fetch_one(&state.db)
    .await?;
    let scene_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM afv_storyboard_scene WHERE storyboard_id = ? AND deleted = 0",
    )
    .bind(storyboard_id)
    .fetch_one(&state.db)
    .await?;
    let item_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM afv_storyboard_item WHERE storyboard_id = ? AND deleted = 0",
    )
    .bind(storyboard_id)
    .fetch_one(&state.db)
    .await?;
    Ok(StoryboardStatistics {
        episode_count,
        scene_count,
        item_count,
    })
}

async fn member_list(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Path(project_id): Path<i64>,
) -> ApiResult<Vec<ProjectMember>> {
    let rows = sqlx::query_as::<_, ProjectMember>(
        "SELECT m.id, m.project_id, m.user_id, m.role, m.create_time,
                u.username, u.nickname, u.avatar
         FROM afv_project_member m
         LEFT JOIN sys_user u ON u.id = m.user_id
         WHERE m.project_id = ? AND m.deleted = 0 ORDER BY m.role ASC",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn member_add(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Path(project_id): Path<i64>,
    Json(req): Json<ProjectMemberAddReq>,
) -> ApiResult<bool> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM afv_project_member WHERE project_id = ? AND user_id = ? AND deleted = 0",
    )
    .bind(project_id)
    .bind(req.user_id)
    .fetch_one(&state.db)
    .await?;
    if exists > 0 {
        return Err(AppError::bad_request("该成员已在项目中"));
    }
    sqlx::query(
        "INSERT INTO afv_project_member (project_id, user_id, role, deleted, create_time, update_time) VALUES (?, ?, ?, 0, ?, ?)",
    )
    .bind(project_id)
    .bind(req.user_id)
    .bind(req.role.unwrap_or(3))
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?;
    ok(true)
}

async fn member_remove(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Path((project_id, user_id)): Path<(i64, i64)>,
) -> ApiResult<bool> {
    sqlx::query(
        "UPDATE afv_project_member SET deleted = 1, update_time = ? WHERE project_id = ? AND user_id = ?",
    )
    .bind(now())
    .bind(project_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;
    ok(true)
}

async fn art_styles() -> ApiResult<Vec<ArtStylePreset>> {
    ok(ART_STYLES
        .iter()
        .map(|(key, name, description, prompt)| ArtStylePreset {
            key: key.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            image_prompt: prompt.to_string(),
            image_url: format!("/static/art-styles/{key}.jpg"),
        })
        .collect())
}

#[allow(dead_code)]
async fn row_exists(state: &AppState, sql: &str, id: i64) -> Result<bool, AppError> {
    let row = sqlx::query(sql).bind(id).fetch_optional(&state.db).await?;
    Ok(row.is_some())
}
