//! 本机用户信息接口。
//!
//! 桌面版取消了登录体系：每次启动只准备一个本地账号，所有请求都以它身份执行。
//! 因此这里只保留「读取当前用户」与「修改个人资料」两个接口，界面用于展示与编辑。

use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, put};
use axum::{Json, Router};
use sqlx::Row;

use fusion_core::error::AppError;
use fusion_core::models::system::{ProfileUpdateReq, UserVo};

use crate::auth::AuthUser;
use crate::state::AppState;
use crate::support::{now, ok, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/auth/user-info", get(user_info))
        .route("/api/auth/profile", put(update_profile))
}

async fn user_info(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<UserVo> {
    let row = sqlx::query(
        "SELECT id, username, nickname, avatar, email, phone, status, create_time
           FROM sys_user WHERE id = ? AND deleted = 0",
    )
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("用户不存在"))?;

    ok(UserVo {
        id: row.get("id"),
        username: row.get("username"),
        nickname: row.get::<Option<String>, _>("nickname").unwrap_or_default(),
        avatar: row.get("avatar"),
        email: row.get("email"),
        phone: row.get("phone"),
        status: row.get("status"),
        create_time: row.get("create_time"),
        roles: vec!["admin".into()],
    })
}

async fn update_profile(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<ProfileUpdateReq>,
) -> ApiResult<bool> {
    sqlx::query(
        "UPDATE sys_user SET nickname = COALESCE(?, nickname), email = COALESCE(?, email),
            phone = COALESCE(?, phone), avatar = COALESCE(?, avatar), update_time = ?
         WHERE id = ?",
    )
    .bind(req.nickname)
    .bind(req.email)
    .bind(req.phone)
    .bind(req.avatar)
    .bind(now())
    .bind(user.id)
    .execute(&state.db)
    .await?;
    ok(true)
}
