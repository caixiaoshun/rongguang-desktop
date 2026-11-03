//! 系统初始化、版本信息、用户与角色、系统配置。

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use sqlx::Row;

use fusion_core::error::AppError;
use fusion_core::models::system::{
    InitStatusResp, Role, SystemRuntimeVersion,
};

use crate::auth::AuthUser;
use crate::routes::ai::{seed_demote_other_providers, seed_grok_models, seed_storage};
use crate::state::AppState;
use crate::support::{now, ok, ApiResult};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/system/init/status", get(init_status))
        .route("/api/system/version", get(version))
        .route("/api/system/version/runtime", get(runtime_version))
        .route("/api/system/config", get(list_config).put(save_config))
        .route("/api/system/config/{key}", get(get_config))
        .route("/api/system/user/page", get(user_page))
        .route("/api/system/user/get", get(get_user))
        .route("/api/system/user/update", put(update_user))
        .route("/api/system/user/delete", axum::routing::delete(delete_user))
        .route("/api/system/user/assign-role", post(assign_role))
        .route("/api/system/user/remove-role", post(remove_role))
        .route("/api/system/role/list", get(role_list))
        .route("/api/system/role/create", post(role_create))
        .route("/api/system/role/update", put(role_update))
        .route("/api/system/role/delete", axum::routing::delete(role_delete))
        .route("/api/system/role/get", get(role_get))
}

async fn user_count(state: &AppState) -> Result<i64, AppError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM sys_user WHERE deleted = 0")
        .fetch_one(&state.db)
        .await?;
    Ok(count)
}

async fn init_status(State(state): State<Arc<AppState>>) -> ApiResult<InitStatusResp> {
    let count = user_count(&state).await?;
    ok(InitStatusResp {
        initialized: count > 0,
        version: APP_VERSION.to_string(),
    })
}

/// 桌面版首次启动时准备的本地账号与默认供应商。
///
/// 已去掉登录体系：所有请求都归属内置账号 `local`（id=1），
/// 这里只保证账号、角色、默认团队与 Grok 模型种子数据存在，且可重复执行。
pub async fn ensure_local_bootstrap(state: &AppState) -> Result<(), AppError> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM sys_user WHERE id = ? LIMIT 1")
        .bind(super::super::state::LOCAL_USER_ID)
        .fetch_optional(&state.db)
        .await?;

    let user_id = match exists {
        Some(id) => id,
        None => {
            sqlx::query(
                "INSERT INTO sys_user (id, username, password, nickname, status, deleted, create_time, update_time)
                 VALUES (?, 'local', '', '本地用户', 0, 0, ?, ?)",
            )
            .bind(super::super::state::LOCAL_USER_ID)
            .bind(now())
            .bind(now())
            .execute(&state.db)
            .await?
            .last_insert_rowid()
        }
    };

    // 管理员角色
    let admin_role: Option<i64> =
        sqlx::query_scalar("SELECT id FROM sys_role WHERE code = 'admin' LIMIT 1")
            .fetch_optional(&state.db)
            .await?;
    let role_id = match admin_role {
        Some(id) => id,
        None => sqlx::query(
            "INSERT INTO sys_role (name, code, sort, status, remark, deleted, create_time, update_time)
             VALUES ('管理员', 'admin', 1, 0, '系统内置角色', 0, ?, ?)",
        )
        .bind(now())
        .bind(now())
        .execute(&state.db)
        .await?
        .last_insert_rowid(),
    };
    let _ = sqlx::query("INSERT OR IGNORE INTO sys_user_role (user_id, role_id, deleted, create_time, update_time) VALUES (?, ?, 0, ?, ?)")
        .bind(user_id)
        .bind(role_id)
        .bind(now())
        .bind(now())
        .execute(&state.db)
        .await;

    // 默认团队
    let team_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM afv_team WHERE owner_user_id = ? AND deleted = 0 ORDER BY id ASC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    if team_id.is_none() {
        let new_team = sqlx::query(
            "INSERT INTO afv_team (name, description, owner_user_id, status, deleted, create_time, update_time)
             VALUES (?, ?, ?, 0, 0, ?, ?)",
        )
        .bind("我的空间")
        .bind("桌面版本地工作区")
        .bind(user_id)
        .bind(now())
        .bind(now())
        .execute(&state.db)
        .await?
        .last_insert_rowid();
        let _ = sqlx::query(
            "INSERT OR IGNORE INTO afv_team_member (team_id, user_id, role, status, deleted, create_time, update_time)
             VALUES (?, ?, 1, 0, 0, ?, ?)",
        )
        .bind(new_team)
        .bind(user_id)
        .bind(now())
        .bind(now())
        .execute(&state.db)
        .await;
    }

    seed_grok_models(state).await?;
    seed_demote_other_providers(state).await?;
    seed_storage(state).await?;
    Ok(())
}

/// 版本检测：真实请求发布平台，结果缓存 10 分钟。
/// `force=true` 用于用户手动点「检查更新」，跳过缓存。
async fn version(Query(q): Query<VersionQuery>) -> ApiResult<super::update::VersionInfo> {
    ok(super::update::check_version(q.force.unwrap_or(false)).await)
}

#[derive(Deserialize, Default)]
pub struct VersionQuery {
    #[serde(default)]
    pub force: Option<bool>,
}

async fn runtime_version() -> ApiResult<SystemRuntimeVersion> {
    ok(SystemRuntimeVersion {
        current_version: APP_VERSION.to_string(),
        current_version_display: APP_VERSION.to_string(),
        app_version: APP_VERSION.to_string(),
        build_time: option_env!("BUILD_TIME").unwrap_or("dev").to_string(),
        runtime: format!("rust {}", rustc_version()),
        rust_version: rustc_version(),
    })
}

fn rustc_version() -> String {
    option_env!("RUSTC_VERSION").unwrap_or("1.93").to_string()
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SystemConfigVo {
    pub id: i64,
    pub config_key: String,
    pub config_value: Option<String>,
    pub remark: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

/// 读取全部系统配置，供设置页回填。
async fn list_config(State(state): State<Arc<AppState>>, _u: AuthUser) -> ApiResult<Vec<SystemConfigVo>> {
    let rows = sqlx::query_as::<_, SystemConfigVo>(
        "SELECT id, config_key, config_value, remark, create_time, update_time
           FROM afv_system_config WHERE deleted = 0 ORDER BY id ASC",
    )
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

/// 批量保存配置（键不存在则创建），接口形状与 Java 版一致：`{ 键: 值 }`。
async fn save_config(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(configs): Json<std::collections::BTreeMap<String, Option<String>>>,
) -> ApiResult<bool> {
    for (key, value) in configs {
        let key = key.trim().to_string();
        if key.is_empty() {
            continue;
        }
        let value = value.unwrap_or_default();
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT id FROM afv_system_config WHERE config_key = ? LIMIT 1")
                .bind(&key)
                .fetch_optional(&state.db)
                .await?;
        match existing {
            Some(id) => {
                sqlx::query("UPDATE afv_system_config SET config_value = ?, update_time = ? WHERE id = ?")
                    .bind(&value)
                    .bind(now())
                    .bind(id)
                    .execute(&state.db)
                    .await?;
            }
            None => {
                sqlx::query(
                    "INSERT INTO afv_system_config (config_key, config_value, deleted, create_time, update_time)
                     VALUES (?, ?, 0, ?, ?)",
                )
                .bind(&key)
                .bind(&value)
                .bind(now())
                .bind(now())
                .execute(&state.db)
                .await?;
            }
        }
    }
    ok(true)
}

#[derive(Deserialize)]
struct KeyPath {
    key: String,
}

async fn get_config(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(key): axum::extract::Path<KeyPath>,
) -> ApiResult<String> {
    let value: Option<String> = sqlx::query_scalar(
        "SELECT config_value FROM afv_system_config WHERE config_key = ? AND deleted = 0 LIMIT 1",
    )
    .bind(&key.key)
    .fetch_optional(&state.db)
    .await?
    .flatten();
    ok(value.unwrap_or_default())
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UserPageQuery {
    pub page_no: i64,
    pub page_size: i64,
    pub keyword: Option<String>,
}

async fn user_page(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<UserPageQuery>,
) -> ApiResult<fusion_core::models::PageResult<fusion_core::models::system::UserVo>> {
    let page_no = if q.page_no <= 0 { 1 } else { q.page_no };
    let page_size = if q.page_size <= 0 { 20 } else { q.page_size };
    let offset = (page_no - 1) * page_size;
    let keyword = format!("%{}%", q.keyword.unwrap_or_default());

    let rows = sqlx::query(
        "SELECT id, username, nickname, avatar, email, phone, status, create_time FROM sys_user
         WHERE deleted = 0 AND (username LIKE ? OR IFNULL(nickname,'') LIKE ?)
         ORDER BY id ASC LIMIT ? OFFSET ?",
    )
    .bind(&keyword)
    .bind(&keyword)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    let list = rows
        .into_iter()
        .map(|row| fusion_core::models::system::UserVo {
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
        .collect();

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM sys_user WHERE deleted = 0 AND (username LIKE ? OR IFNULL(nickname,'') LIKE ?)",
    )
    .bind(&keyword)
    .bind(&keyword)
    .fetch_one(&state.db)
    .await?;

    ok(fusion_core::models::PageResult::new(list, total, page_no, page_size))
}

#[derive(Deserialize)]
struct IdQuery {
    id: i64,
}

async fn get_user(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<fusion_core::models::system::UserVo> {
    let row = sqlx::query(
        "SELECT id, username, nickname, avatar, email, phone, status, create_time FROM sys_user WHERE id = ? AND deleted = 0",
    )
    .bind(q.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("用户不存在"))?;
    ok(fusion_core::models::system::UserVo {
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct UserUpdateReq {
    id: i64,
    nickname: Option<String>,
    email: Option<String>,
    phone: Option<String>,
    status: Option<i64>,
}

impl Default for UserUpdateReq {
    fn default() -> Self {
        Self {
            id: 0,
            nickname: None,
            email: None,
            phone: None,
            status: None,
        }
    }
}

async fn update_user(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<UserUpdateReq>,
) -> ApiResult<bool> {
    sqlx::query(
        "UPDATE sys_user SET nickname = COALESCE(?, nickname), email = COALESCE(?, email), phone = COALESCE(?, phone), status = COALESCE(?, status), update_time = ? WHERE id = ?",
    )
    .bind(req.nickname)
    .bind(req.email)
    .bind(req.phone)
    .bind(req.status)
    .bind(now())
    .bind(req.id)
    .execute(&state.db)
    .await?;
    ok(true)
}

async fn delete_user(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<bool> {
    if q.id == user.id {
        return Err(AppError::bad_request("不能删除当前登录账号"));
    }
    sqlx::query("UPDATE sys_user SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(q.id)
        .execute(&state.db)
        .await?;
    ok(true)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoleAssignReq {
    user_id: i64,
    role_id: i64,
}

async fn assign_role(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<RoleAssignReq>,
) -> ApiResult<bool> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM sys_user_role WHERE user_id = ? AND role_id = ?",
    )
    .bind(req.user_id)
    .bind(req.role_id)
    .fetch_one(&state.db)
    .await?;
    if exists == 0 {
        sqlx::query("INSERT INTO sys_user_role (user_id, role_id) VALUES (?, ?)")
            .bind(req.user_id)
            .bind(req.role_id)
            .execute(&state.db)
            .await?;
    }
    ok(true)
}

async fn remove_role(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<RoleAssignReq>,
) -> ApiResult<bool> {
    sqlx::query("DELETE FROM sys_user_role WHERE user_id = ? AND role_id = ?")
        .bind(req.user_id)
        .bind(req.role_id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn role_list(State(state): State<Arc<AppState>>, _user: AuthUser) -> ApiResult<Vec<Role>> {
    let rows = sqlx::query_as::<_, Role>(
        "SELECT id, name, code, sort, status, remark, create_time, update_time FROM sys_role WHERE deleted = 0 ORDER BY sort ASC",
    )
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RoleSaveReq {
    id: Option<i64>,
    name: String,
    code: String,
    sort: Option<i64>,
    status: Option<i64>,
    remark: Option<String>,
}

impl Default for RoleSaveReq {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            code: String::new(),
            sort: None,
            status: None,
            remark: None,
        }
    }
}

async fn role_create(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<RoleSaveReq>,
) -> ApiResult<i64> {
    let id = sqlx::query(
        "INSERT INTO sys_role (name, code, sort, status, remark, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, 0, ?, ?)",
    )
    .bind(&req.name)
    .bind(&req.code)
    .bind(req.sort.unwrap_or(0))
    .bind(req.status.unwrap_or(0))
    .bind(&req.remark)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();
    ok(id)
}

async fn role_update(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<RoleSaveReq>,
) -> ApiResult<bool> {
    sqlx::query(
        "UPDATE sys_role SET name = ?, code = ?, sort = ?, status = ?, remark = ?, update_time = ? WHERE id = ?",
    )
    .bind(&req.name)
    .bind(&req.code)
    .bind(req.sort.unwrap_or(0))
    .bind(req.status.unwrap_or(0))
    .bind(&req.remark)
    .bind(now())
    .bind(req.id.unwrap_or(0))
    .execute(&state.db)
    .await?;
    ok(true)
}

async fn role_delete(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE sys_role SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(q.id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn role_get(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<Role> {
    let role = sqlx::query_as::<_, Role>(
        "SELECT id, name, code, sort, status, remark, create_time, update_time FROM sys_role WHERE id = ?",
    )
    .bind(q.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("角色不存在"))?;
    ok(role)
}
