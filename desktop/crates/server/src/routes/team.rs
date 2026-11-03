//! 团队与成员（单机版保留结构，默认单成员）。

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use fusion_core::error::AppError;
use fusion_core::models::team::{Team, TeamCreateReq, TeamMember, TeamMemberAddReq, TeamUpdateReq};

use crate::auth::AuthUser;
use crate::state::AppState;
use crate::support::{now, ok, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/team/create", post(create))
        .route("/api/team/get", get(get_team))
        .route("/api/team/page", get(page))
        .route("/api/team/update", put(update))
        .route("/api/team/delete", axum::routing::delete(remove))
        .route("/api/team/member/add", post(member_add))
        .route("/api/team/member/remove", post(member_remove))
        .route("/api/team/member/change-role", post(member_change_role))
        .route("/api/team/member/list", get(member_list))
        .route("/api/team/current", get(current))
}

async fn create(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<TeamCreateReq>,
) -> ApiResult<Team> {
    if req.name.trim().is_empty() {
        return Err(AppError::bad_request("团队名称不能为空"));
    }
    let id = sqlx::query(
        "INSERT INTO afv_team (name, description, logo, owner_user_id, status, deleted, create_time, update_time)
         VALUES (?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(req.name.trim())
    .bind(req.description)
    .bind(req.logo)
    .bind(user.id)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    sqlx::query(
        "INSERT INTO afv_team_member (team_id, user_id, role, status, deleted, create_time, update_time) VALUES (?, ?, 1, 0, 0, ?, ?)",
    )
    .bind(id)
    .bind(user.id)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?;

    ok(load_team(&state, id).await?)
}

#[derive(Deserialize)]
struct IdQuery {
    id: i64,
}

async fn load_team(state: &AppState, id: i64) -> Result<Team, AppError> {
    sqlx::query_as::<_, Team>(
        "SELECT t.*, (SELECT COUNT(1) FROM afv_team_member m WHERE m.team_id = t.id AND m.deleted = 0) AS member_count
         FROM afv_team t WHERE t.id = ? AND t.deleted = 0",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("团队不存在"))
}

async fn get_team(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<Team> {
    ok(load_team(&state, q.id).await?)
}

async fn page(State(state): State<Arc<AppState>>, _u: AuthUser) -> ApiResult<Vec<Team>> {
    let rows = sqlx::query_as::<_, Team>(
        "SELECT t.*, (SELECT COUNT(1) FROM afv_team_member m WHERE m.team_id = t.id AND m.deleted = 0) AS member_count
         FROM afv_team t WHERE t.deleted = 0 ORDER BY t.id ASC",
    )
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn update(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<TeamUpdateReq>,
) -> ApiResult<Team> {
    sqlx::query(
        "UPDATE afv_team SET name = COALESCE(?, name), description = COALESCE(?, description),
            logo = COALESCE(?, logo), status = COALESCE(?, status), update_time = ?
         WHERE id = ? AND deleted = 0",
    )
    .bind(req.name)
    .bind(req.description)
    .bind(req.logo)
    .bind(req.status)
    .bind(now())
    .bind(req.id)
    .execute(&state.db)
    .await?;
    ok(load_team(&state, req.id).await?)
}

async fn remove(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_team SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(q.id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn member_add(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<TeamMemberAddReq>,
) -> ApiResult<bool> {
    let user_id = match (req.user_id, req.username.clone()) {
        (Some(id), _) => id,
        (None, Some(username)) => sqlx::query_scalar::<_, i64>(
            "SELECT id FROM sys_user WHERE username = ? AND deleted = 0",
        )
        .bind(username)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("用户不存在"))?,
        _ => return Err(AppError::bad_request("缺少成员标识")),
    };

    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM afv_team_member WHERE team_id = ? AND user_id = ? AND deleted = 0",
    )
    .bind(req.team_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    if exists > 0 {
        return Err(AppError::bad_request("该成员已在团队中"));
    }

    sqlx::query(
        "INSERT INTO afv_team_member (team_id, user_id, role, status, deleted, create_time, update_time) VALUES (?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(req.team_id)
    .bind(user_id)
    .bind(req.role.unwrap_or(3))
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?;
    ok(true)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemberRemoveQuery {
    team_id: i64,
    user_id: i64,
}

async fn member_remove(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<MemberRemoveQuery>,
) -> ApiResult<bool> {
    sqlx::query(
        "UPDATE afv_team_member SET deleted = 1, update_time = ? WHERE team_id = ? AND user_id = ?",
    )
    .bind(now())
    .bind(q.team_id)
    .bind(q.user_id)
    .execute(&state.db)
    .await?;
    ok(true)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangeRoleQuery {
    team_id: i64,
    user_id: i64,
    role: i64,
}

async fn member_change_role(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<ChangeRoleQuery>,
) -> ApiResult<bool> {
    sqlx::query(
        "UPDATE afv_team_member SET role = ?, update_time = ? WHERE team_id = ? AND user_id = ?",
    )
    .bind(q.role)
    .bind(now())
    .bind(q.team_id)
    .bind(q.user_id)
    .execute(&state.db)
    .await?;
    ok(true)
}

async fn member_list(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<Vec<TeamMember>> {
    let rows = sqlx::query_as::<_, TeamMember>(
        "SELECT m.id, m.team_id, m.user_id, m.role, m.status, m.create_time,
                u.username, u.nickname, u.avatar
         FROM afv_team_member m LEFT JOIN sys_user u ON u.id = m.user_id
         WHERE m.team_id = ? AND m.deleted = 0 ORDER BY m.role ASC, m.id ASC",
    )
    .bind(q.id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn current(State(state): State<Arc<AppState>>, _u: AuthUser) -> ApiResult<Option<Team>> {
    let id: Option<i64> =
        sqlx::query_scalar("SELECT id FROM afv_team WHERE deleted = 0 ORDER BY id ASC LIMIT 1")
            .fetch_optional(&state.db)
            .await?;
    match id {
        Some(id) => ok(Some(load_team(&state, id).await?)),
        None => ok(None),
    }
}
