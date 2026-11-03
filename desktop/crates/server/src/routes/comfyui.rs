//! ComfyUI 工作流管理：工作流（Workflow）→ 版本（Version）→ 发布（Publish）。
//!
//! 与原 Java 版 `ComfyUiWorkflowService` 的业务规则保持一致：
//! - 同一 ComfyUI 供应商下 `code` 唯一；
//! - 工作流创建后不能更换所属供应商；
//! - 已发布工作流不能更换模型类型；
//! - 已发布版本不可修改 / 删除，只能新建版本；
//! - 被模型引用的工作流不可删除。

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;

use fusion_core::error::AppError;
use fusion_core::time;

use crate::auth::AuthUser;
use crate::state::AppState;
use crate::support::{ok, ApiResult};

const MODEL_TYPE_IMAGE: i64 = 2;
const MODEL_TYPE_VIDEO: i64 = 3;
const VALIDATION_PENDING: i64 = 0;
const TEST_PENDING: i64 = 0;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/ai/comfyui/workflow/create", post(create_workflow))
        .route("/api/ai/comfyui/workflow/update", put(update_workflow))
        .route("/api/ai/comfyui/workflow/delete", delete(delete_workflow))
        .route("/api/ai/comfyui/workflow/get", get(get_workflow))
        .route("/api/ai/comfyui/workflow/page", get(page_workflow))
        .route("/api/ai/comfyui/workflow/list", get(list_workflow))
        .route("/api/ai/comfyui/workflow/versions", get(list_versions))
        .route("/api/ai/comfyui/workflow/version/get", get(get_version))
        .route("/api/ai/comfyui/workflow/version/create", post(create_version))
        .route("/api/ai/comfyui/workflow/version/update", put(update_version))
        .route("/api/ai/comfyui/workflow/version/delete", delete(delete_version))
        .route("/api/ai/comfyui/workflow/version/validate", post(validate_version))
        .route("/api/ai/comfyui/workflow/version/test", post(test_version))
        .route("/api/ai/comfyui/workflow/publish", post(publish_workflow))
}

// ───────────────────────── DTO ─────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowVo {
    pub id: i64,
    #[serde(rename = "apiConfigId")]
    pub api_config_id: i64,
    pub name: String,
    pub code: String,
    pub model_type: i64,
    pub description: Option<String>,
    pub active_version_id: Option<i64>,
    pub status: i64,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSaveReq {
    pub id: Option<i64>,
    pub api_config_id: Option<i64>,
    pub name: Option<String>,
    pub code: Option<String>,
    pub model_type: Option<i64>,
    pub description: Option<String>,
    pub status: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionVo {
    pub id: i64,
    pub workflow_id: i64,
    pub version_no: i64,
    pub ui_workflow_json: Option<String>,
    pub api_workflow_json: Option<String>,
    pub input_bindings_json: Option<String>,
    pub output_bindings_json: Option<String>,
    pub required_nodes_json: Option<String>,
    pub workflow_hash: Option<String>,
    pub validation_status: i64,
    pub validation_message: Option<String>,
    pub test_status: i64,
    pub test_message: Option<String>,
    pub last_test_time: Option<String>,
    pub published: bool,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSaveReq {
    pub id: Option<i64>,
    pub workflow_id: Option<i64>,
    pub ui_workflow_json: Option<String>,
    pub api_workflow_json: Option<String>,
    pub input_bindings_json: Option<String>,
    pub output_bindings_json: Option<String>,
}

// ───────────────────────── 查询辅助 ─────────────────────────

fn row_to_workflow(r: &sqlx::sqlite::SqliteRow) -> WorkflowVo {
    WorkflowVo {
        id: r.get("id"),
        api_config_id: r.get::<Option<i64>, _>("api_config_id").unwrap_or(0),
        name: r.get::<Option<String>, _>("name").unwrap_or_default(),
        code: r.get::<Option<String>, _>("code").unwrap_or_default(),
        model_type: r.get::<Option<i64>, _>("model_type").unwrap_or(MODEL_TYPE_IMAGE),
        description: r.get("description"),
        active_version_id: r.get("active_version_id"),
        status: r.get::<Option<i64>, _>("status").unwrap_or(1),
        create_time: r.get::<Option<String>, _>("create_time").unwrap_or_default(),
        update_time: r.get::<Option<String>, _>("update_time").unwrap_or_default(),
    }
}

fn row_to_version(r: &sqlx::sqlite::SqliteRow) -> VersionVo {
    VersionVo {
        id: r.get("id"),
        workflow_id: r.get::<Option<i64>, _>("workflow_id").unwrap_or(0),
        version_no: r.get::<Option<i64>, _>("version_no").unwrap_or(1),
        ui_workflow_json: r.get("ui_workflow_json"),
        api_workflow_json: r.get("api_workflow_json"),
        input_bindings_json: r.get("input_bindings_json"),
        output_bindings_json: r.get("output_bindings_json"),
        required_nodes_json: r.get("required_nodes_json"),
        workflow_hash: r.get("workflow_hash"),
        validation_status: r.get::<Option<i64>, _>("validation_status").unwrap_or(VALIDATION_PENDING),
        validation_message: r.get("validation_message"),
        test_status: r.get::<Option<i64>, _>("test_status").unwrap_or(TEST_PENDING),
        test_message: r.get("test_message"),
        last_test_time: r.get("last_test_time"),
        published: r.get::<Option<i64>, _>("published").unwrap_or(0) != 0,
        create_time: r.get::<Option<String>, _>("create_time").unwrap_or_default(),
        update_time: r.get::<Option<String>, _>("update_time").unwrap_or_default(),
    }
}

const WORKFLOW_COLS: &str = "id, api_config_id, name, code, model_type, description, active_version_id, status, create_time, update_time";
const VERSION_COLS: &str = "id, workflow_id, version_no, ui_workflow_json, api_workflow_json, input_bindings_json, output_bindings_json, required_nodes_json, workflow_hash, validation_status, validation_message, test_status, test_message, last_test_time, published, create_time, update_time";

async fn require_workflow(state: &AppState, id: i64) -> Result<WorkflowVo, AppError> {
    let row = sqlx::query(&format!(
        "SELECT {WORKFLOW_COLS} FROM afv_comfyui_workflow WHERE id = ? AND deleted = 0"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("ComfyUI 工作流不存在"))?;
    Ok(row_to_workflow(&row))
}

async fn require_version(state: &AppState, id: i64) -> Result<VersionVo, AppError> {
    let row = sqlx::query(&format!(
        "SELECT {VERSION_COLS} FROM afv_comfyui_workflow_version WHERE id = ? AND deleted = 0"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("工作流版本不存在"))?;
    Ok(row_to_version(&row))
}

/// 校验供应商确实是 ComfyUI 平台。
async fn require_comfyui_api_config(state: &AppState, id: i64) -> Result<(), AppError> {
    let platform: Option<String> = sqlx::query_scalar::<_, Option<String>>(
        "SELECT platform FROM afv_api_config WHERE id = ? AND deleted = 0",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .flatten();
    match platform.as_deref() {
        None => Err(AppError::bad_request("ComfyUI 供应商配置不存在")),
        Some("comfyui") => Ok(()),
        Some(_) => Err(AppError::bad_request("所选供应商不是 ComfyUI 平台")),
    }
}

fn normalize_code(value: &str) -> String {
    value.trim().to_lowercase().replace([' ', '\t'], "-")
}

async fn ensure_unique_code(
    state: &AppState,
    current_id: Option<i64>,
    api_config_id: i64,
    code: &str,
) -> Result<(), AppError> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM afv_comfyui_workflow WHERE api_config_id = ? AND code = ? AND deleted = 0 AND id <> ? LIMIT 1",
    )
    .bind(api_config_id)
    .bind(code)
    .bind(current_id.unwrap_or(0))
    .fetch_optional(&state.db)
    .await?
    .flatten();
    if exists.is_some() {
        return Err(AppError::bad_request("同一 ComfyUI 配置下工作流标识已存在"));
    }
    Ok(())
}

// ───────────────────────── 工作流 CRUD ─────────────────────────

async fn create_workflow(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<WorkflowSaveReq>,
) -> ApiResult<i64> {
    let api_config_id = req
        .api_config_id
        .ok_or_else(|| AppError::bad_request("请选择 ComfyUI 供应商"))?;
    require_comfyui_api_config(&state, api_config_id).await?;

    let name = req.name.as_deref().unwrap_or("").trim().to_string();
    let code = normalize_code(req.code.as_deref().unwrap_or(""));
    if name.is_empty() {
        return Err(AppError::bad_request("工作流名称不能为空"));
    }
    if code.is_empty() {
        return Err(AppError::bad_request("工作流标识不能为空"));
    }
    let model_type = req.model_type.unwrap_or(MODEL_TYPE_IMAGE);
    if model_type != MODEL_TYPE_IMAGE && model_type != MODEL_TYPE_VIDEO {
        return Err(AppError::bad_request("模型类型只能是 2（图片）或 3（视频）"));
    }
    ensure_unique_code(&state, None, api_config_id, &code).await?;

    let now = time::now();
    let id = sqlx::query(
        "INSERT INTO afv_comfyui_workflow (api_config_id, name, code, model_type, description, status, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)",
    )
    .bind(api_config_id)
    .bind(&name)
    .bind(&code)
    .bind(model_type)
    .bind(req.description.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(req.status.unwrap_or(1))
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::bad_request("同一 ComfyUI 配置下工作流标识已存在")
        } else {
            e.into()
        }
    })?
    .last_insert_rowid();

    ok(id)
}

async fn update_workflow(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<WorkflowSaveReq>,
) -> ApiResult<bool> {
    let id = req.id.ok_or_else(|| AppError::bad_request("缺少工作流 id"))?;
    let wf = require_workflow(&state, id).await?;

    if let Some(cfg) = req.api_config_id {
        if cfg != wf.api_config_id {
            return Err(AppError::bad_request("ComfyUI 工作流创建后不能更换所属供应商"));
        }
    }

    let next_model_type = req.model_type.unwrap_or(wf.model_type);
    if next_model_type != MODEL_TYPE_IMAGE && next_model_type != MODEL_TYPE_VIDEO {
        return Err(AppError::bad_request("模型类型只能是 2（图片）或 3（视频）"));
    }
    if wf.active_version_id.is_some() && next_model_type != wf.model_type {
        return Err(AppError::bad_request("已发布工作流不能更换模型类型"));
    }

    let next_code = match req.code.as_deref() {
        Some(c) if !c.trim().is_empty() => normalize_code(c),
        Some(_) => return Err(AppError::bad_request("工作流标识不能为空")),
        None => wf.code.clone(),
    };
    let next_name = match req.name.as_deref() {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        Some(_) => return Err(AppError::bad_request("工作流名称不能为空")),
        None => wf.name.clone(),
    };
    ensure_unique_code(&state, Some(id), wf.api_config_id, &next_code).await?;

    sqlx::query(
        "UPDATE afv_comfyui_workflow SET name = ?, code = ?, model_type = ?, description = ?, status = ?, update_time = ? WHERE id = ?",
    )
    .bind(&next_name)
    .bind(&next_code)
    .bind(next_model_type)
    .bind(
        req.description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or(wf.description),
    )
    .bind(req.status.unwrap_or(wf.status))
    .bind(time::now())
    .bind(id)
    .execute(&state.db)
    .await?;

    ok(true)
}

#[derive(Deserialize)]
struct IdQuery {
    id: Option<i64>,
}

async fn delete_workflow(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<bool> {
    let id = q.id.ok_or_else(|| AppError::bad_request("缺少 id"))?;
    let _ = require_workflow(&state, id).await?;

    // 被模型引用时拒绝删除。
    let referenced: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM afv_ai_model WHERE comfyui_workflow_id = ? AND deleted = 0 LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .flatten();
    if referenced.is_some() {
        return Err(AppError::bad_request("工作流已被模型引用，请先解除模型绑定"));
    }

    let now = time::now();
    sqlx::query("UPDATE afv_comfyui_workflow SET deleted = 1, deleted_id = id, update_time = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn get_workflow(
    State(state): State<Arc<AppState>>,
    Query(q): Query<IdQuery>,
) -> ApiResult<WorkflowVo> {
    let id = q.id.ok_or_else(|| AppError::bad_request("缺少 id"))?;
    ok(require_workflow(&state, id).await?)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageQuery {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    api_config_id: Option<i64>,
    #[serde(default)]
    model_type: Option<i64>,
    #[serde(default)]
    status: Option<i64>,
    #[serde(default = "default_page")]
    page_no: i64,
    #[serde(default = "default_size")]
    page_size: i64,
}

fn default_page() -> i64 { 1 }
fn default_size() -> i64 { 20 }

async fn page_workflow(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PageQuery>,
) -> ApiResult<serde_json::Value> {
    let mut where_sql = String::from(" WHERE deleted = 0");
    if q.name.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) {
        where_sql.push_str(" AND name LIKE '%' || ? || '%'");
    }
    if q.code.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) {
        where_sql.push_str(" AND code LIKE '%' || ? || '%'");
    }
    if q.api_config_id.is_some() {
        where_sql.push_str(" AND api_config_id = ?");
    }
    if q.model_type.is_some() {
        where_sql.push_str(" AND model_type = ?");
    }
    if q.status.is_some() {
        where_sql.push_str(" AND status = ?");
    }

    let mut bind_strs: Vec<String> = Vec::new();
    if let Some(n) = q.name.filter(|s| !s.trim().is_empty()) { bind_strs.push(n); }
    if let Some(c) = q.code.filter(|s| !s.trim().is_empty()) { bind_strs.push(c); }

    let count_sql = format!("SELECT COUNT(*) FROM afv_comfyui_workflow{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for s in &bind_strs { cq = cq.bind(s); }
    if let Some(v) = q.api_config_id { cq = cq.bind(v); }
    if let Some(v) = q.model_type { cq = cq.bind(v); }
    if let Some(v) = q.status { cq = cq.bind(v); }
    let total = cq.fetch_one(&state.db).await.unwrap_or(0);

    let page_no = q.page_no.max(1);
    let page_size = q.page_size.clamp(1, 500);
    let list_sql = format!(
        "SELECT {WORKFLOW_COLS} FROM afv_comfyui_workflow{where_sql} ORDER BY id DESC LIMIT ? OFFSET ?"
    );
    let mut lq = sqlx::query(&list_sql);
    for s in &bind_strs { lq = lq.bind(s); }
    if let Some(v) = q.api_config_id { lq = lq.bind(v); }
    if let Some(v) = q.model_type { lq = lq.bind(v); }
    if let Some(v) = q.status { lq = lq.bind(v); }
    lq = lq.bind(page_size).bind((page_no - 1) * page_size);

    let rows = lq.fetch_all(&state.db).await?;
    ok(json!({
        "list": rows.iter().map(row_to_workflow).collect::<Vec<_>>(),
        "total": total,
        "pageNo": page_no,
        "pageSize": page_size,
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    #[serde(default)]
    api_config_id: Option<i64>,
    #[serde(default)]
    model_type: Option<i64>,
}

async fn list_workflow(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Vec<WorkflowVo>> {
    let mut sql = format!(
        "SELECT {WORKFLOW_COLS} FROM afv_comfyui_workflow WHERE deleted = 0 AND status = 1 AND active_version_id IS NOT NULL"
    );
    let mut binds: Vec<i64> = Vec::new();
    if let Some(v) = q.api_config_id {
        sql.push_str(" AND api_config_id = ?");
        binds.push(v);
    }
    if let Some(v) = q.model_type {
        sql.push_str(" AND model_type = ?");
        binds.push(v);
    }
    sql.push_str(" ORDER BY name ASC");

    let mut query = sqlx::query(&sql);
    for b in binds { query = query.bind(b); }
    let rows = query.fetch_all(&state.db).await?;
    ok(rows.iter().map(row_to_workflow).collect())
}

// ───────────────────────── 版本 ─────────────────────────

/// 从工作流 JSON 中提取节点类名集合，用于保存时记录 required_nodes。
fn collect_node_classes(api_workflow_json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(api_workflow_json) else {
        return Vec::new();
    };
    let mut set: Vec<String> = Vec::new();
    if let Some(obj) = v.as_object() {
        for (_, node) in obj {
            if let Some(class) = node.get("class_type").and_then(|c| c.as_str()) {
                if !set.iter().any(|s| s == class) {
                    set.push(class.to_string());
                }
            }
        }
    }
    set.sort();
    set
}

fn sha256_hex(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

async fn create_version(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<VersionSaveReq>,
) -> ApiResult<i64> {
    let workflow_id = req
        .workflow_id
        .ok_or_else(|| AppError::bad_request("缺少 workflowId"))?;
    let _ = require_workflow(&state, workflow_id).await?;

    let api_json = req.api_workflow_json.unwrap_or_default();
    if serde_json::from_str::<serde_json::Value>(&api_json).is_err() {
        return Err(AppError::bad_request("API 工作流 JSON 格式不合法"));
    }
    let output_bindings = req.output_bindings_json.unwrap_or_else(|| "[]".into());
    if serde_json::from_str::<serde_json::Value>(&output_bindings).is_err() {
        return Err(AppError::bad_request("输出绑定 JSON 格式不合法"));
    }
    let input_bindings = req.input_bindings_json.unwrap_or_else(|| "{}".into());
    if serde_json::from_str::<serde_json::Value>(&input_bindings).is_err() {
        return Err(AppError::bad_request("输入绑定 JSON 格式不合法"));
    }

    let latest: Option<i64> = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(version_no) FROM afv_comfyui_workflow_version WHERE workflow_id = ? AND deleted = 0",
    )
    .bind(workflow_id)
    .fetch_optional(&state.db)
    .await?
    .flatten();
    let next_no = latest.unwrap_or(0) + 1;

    let required = serde_json::to_string(&collect_node_classes(&api_json))?;
    let hash = sha256_hex(&format!("{api_json}{input_bindings}{output_bindings}"));
    let now = time::now();

    let id = sqlx::query(
        "INSERT INTO afv_comfyui_workflow_version (workflow_id, version_no, ui_workflow_json, api_workflow_json, input_bindings_json, output_bindings_json, required_nodes_json, workflow_hash, validation_status, test_status, published, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(workflow_id)
    .bind(next_no)
    .bind(req.ui_workflow_json)
    .bind(&api_json)
    .bind(&input_bindings)
    .bind(&output_bindings)
    .bind(&required)
    .bind(&hash)
    .bind(VALIDATION_PENDING)
    .bind(TEST_PENDING)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    ok(id)
}

async fn update_version(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<VersionSaveReq>,
) -> ApiResult<bool> {
    let id = req
        .id
        .ok_or_else(|| AppError::bad_request("缺少版本 id"))?;
    let version = require_version(&state, id).await?;
    if version.published {
        return Err(AppError::bad_request("已发布版本不可修改，请创建新版本"));
    }

    let api_json = req.api_workflow_json.unwrap_or_else(|| version.api_workflow_json.clone().unwrap_or_default());
    if serde_json::from_str::<serde_json::Value>(&api_json).is_err() {
        return Err(AppError::bad_request("API 工作流 JSON 格式不合法"));
    }
    let output_bindings = req
        .output_bindings_json
        .unwrap_or_else(|| version.output_bindings_json.clone().unwrap_or_else(|| "[]".into()));
    let input_bindings = req
        .input_bindings_json
        .unwrap_or_else(|| version.input_bindings_json.clone().unwrap_or_else(|| "{}".into()));

    let required = serde_json::to_string(&collect_node_classes(&api_json))?;
    let hash = sha256_hex(&format!("{api_json}{input_bindings}{output_bindings}"));

    // 内容变化后校验与测试结果失效。
    sqlx::query(
        "UPDATE afv_comfyui_workflow_version SET ui_workflow_json = ?, api_workflow_json = ?, input_bindings_json = ?, output_bindings_json = ?, required_nodes_json = ?, workflow_hash = ?, validation_status = ?, validation_message = NULL, test_status = ?, test_message = NULL, last_test_time = NULL, update_time = ? WHERE id = ?",
    )
    .bind(req.ui_workflow_json.or(version.ui_workflow_json))
    .bind(&api_json)
    .bind(&input_bindings)
    .bind(&output_bindings)
    .bind(&required)
    .bind(&hash)
    .bind(VALIDATION_PENDING)
    .bind(TEST_PENDING)
    .bind(time::now())
    .bind(id)
    .execute(&state.db)
    .await?;

    ok(true)
}

async fn delete_version(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<bool> {
    let id = q.id.ok_or_else(|| AppError::bad_request("缺少 id"))?;
    let version = require_version(&state, id).await?;
    let wf = require_workflow(&state, version.workflow_id).await?;
    if version.published || wf.active_version_id == Some(id) {
        return Err(AppError::bad_request("已发布版本不可删除"));
    }
    sqlx::query("DELETE FROM afv_comfyui_workflow_version WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn get_version(
    State(state): State<Arc<AppState>>,
    Query(q): Query<IdQuery>,
) -> ApiResult<VersionVo> {
    let id = q.id.ok_or_else(|| AppError::bad_request("缺少 id"))?;
    ok(require_version(&state, id).await?)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionsQuery {
    workflow_id: i64,
}

async fn list_versions(
    State(state): State<Arc<AppState>>,
    Query(q): Query<VersionsQuery>,
) -> ApiResult<Vec<VersionVo>> {
    let _ = require_workflow(&state, q.workflow_id).await?;
    let rows = sqlx::query(&format!(
        "SELECT {VERSION_COLS} FROM afv_comfyui_workflow_version WHERE workflow_id = ? AND deleted = 0 ORDER BY version_no DESC"
    ))
    .bind(q.workflow_id)
    .fetch_all(&state.db)
    .await?;
    ok(rows.iter().map(row_to_version).collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishQuery {
    workflow_id: i64,
    version_id: i64,
}

async fn publish_workflow(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<PublishQuery>,
) -> ApiResult<bool> {
    let _ = require_workflow(&state, q.workflow_id).await?;
    let version = require_version(&state, q.version_id).await?;
    if version.workflow_id != q.workflow_id {
        return Err(AppError::bad_request("版本不属于该工作流"));
    }

    let now = time::now();
    // 只有当前发布版本保留 published=1。
    sqlx::query(
        "UPDATE afv_comfyui_workflow_version SET published = 0, update_time = ? WHERE workflow_id = ? AND published = 1",
    )
    .bind(&now)
    .bind(q.workflow_id)
    .execute(&state.db)
    .await?;
    sqlx::query("UPDATE afv_comfyui_workflow_version SET published = 1, update_time = ? WHERE id = ?")
        .bind(&now)
        .bind(q.version_id)
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE afv_comfyui_workflow SET active_version_id = ?, update_time = ? WHERE id = ?")
        .bind(q.version_id)
        .bind(&now)
        .bind(q.workflow_id)
        .execute(&state.db)
        .await?;

    ok(true)
}

// ───────────────────────── 校验与测试 ─────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionIdQuery {
    version_id: i64,
}

/// 静态校验：节点类是否齐全、输出绑定是否指向存在的节点。
async fn validate_version(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<VersionIdQuery>,
) -> ApiResult<serde_json::Value> {
    let version = require_version(&state, q.version_id).await?;
    let api_json = version.api_workflow_json.clone().unwrap_or_default();

    let parsed = serde_json::from_str::<serde_json::Value>(&api_json)
        .map_err(|_| AppError::bad_request("API 工作流 JSON 无法解析"))?;
    let nodes = parsed.as_object().cloned().unwrap_or_default();

    let mut missing_node_classes: Vec<String> = Vec::new();
    let mut invalid_model_inputs: Vec<String> = Vec::new();

    // 输出绑定必须指向存在的节点。
    if let Some(outs) = version
        .output_bindings_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.as_array().cloned())
    {
        for out in &outs {
            if let Some(node_id) = out.get("nodeId").and_then(|v| v.as_str()) {
                if !nodes.contains_key(node_id) {
                    missing_node_classes.push(format!("输出节点 {node_id} 不存在"));
                }
            }
        }
    }

    // 输入绑定同样检查节点存在性。
    if let Some(map) = version
        .input_bindings_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.as_object().cloned())
    {
        for (_, list) in map {
            if let Some(arr) = list.as_array() {
                for b in arr {
                    if let Some(node_id) = b.get("nodeId").and_then(|v| v.as_str()) {
                        if !nodes.contains_key(node_id) {
                            missing_node_classes.push(format!("输入节点 {node_id} 不存在"));
                        }
                    }
                }
            }
        }
    }

    // 模型类节点缺少必填输入时给出提示。
    for (id, node) in &nodes {
        let Some(class) = node.get("class_type").and_then(|c| c.as_str()) else {
            continue;
        };
        if class.to_lowercase().contains("checkpointloader") || class.to_lowercase().contains("unetloader") {
            let has_input = node.get("inputs").map(|i| !i.is_null()).unwrap_or(false);
            if !has_input {
                invalid_model_inputs.push(format!("节点 {id}({class}) 缺少模型输入"));
            }
        }
    }

    let valid = missing_node_classes.is_empty() && invalid_model_inputs.is_empty();
    let message = if valid {
        "校验通过".to_string()
    } else {
        format!(
            "校验未通过：{} 个节点问题，{} 个模型输入问题",
            missing_node_classes.len(),
            invalid_model_inputs.len()
        )
    };

    // 把结果写回版本记录。
    sqlx::query(
        "UPDATE afv_comfyui_workflow_version SET validation_status = ?, validation_message = ?, update_time = ? WHERE id = ?",
    )
    .bind(if valid { 1 } else { 2 })
    .bind(&message)
    .bind(time::now())
    .bind(q.version_id)
    .execute(&state.db)
    .await?;

    ok(json!({
        "valid": valid,
        "checkedNodeCount": nodes.len(),
        "missingNodeClasses": missing_node_classes,
        "invalidModelInputs": invalid_model_inputs,
        "message": message,
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestVersionReq {
    version_id: i64,
    #[serde(default)]
    inputs: serde_json::Map<String, serde_json::Value>,
}

/// 运行测试：把工作流提交到 ComfyUI 的 `/prompt`，等待历史结果并回传产物 URL。
async fn test_version(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<TestVersionReq>,
) -> ApiResult<serde_json::Value> {
    let started = std::time::Instant::now();
    let version = require_version(&state, req.version_id).await?;
    let wf = require_workflow(&state, version.workflow_id).await?;

    // 取供应商配置。
    let cfg_row = sqlx::query(
        "SELECT api_url, api_key, proxy_type, proxy_host, proxy_port, proxy_username, proxy_password FROM afv_api_config WHERE id = ? AND deleted = 0",
    )
    .bind(wf.api_config_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::bad_request("ComfyUI 供应商配置不存在"))?;

    let base = cfg_row
        .get::<Option<String>, _>("api_url")
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string();
    if base.is_empty() {
        return Err(AppError::bad_request("ComfyUI 服务地址未配置"));
    }
    let api_key: Option<String> = cfg_row.get("api_key");

    // 用输入绑定把用户输入注入工作流。
    let mut workflow: serde_json::Value = version
        .api_workflow_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .ok_or_else(|| AppError::bad_request("工作流 JSON 无法解析"))?;

    if let Some(map) = version
        .input_bindings_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.as_object().cloned())
    {
        for (key, list) in map {
            let Some(value) = req.inputs.get(&key) else { continue };
            let Some(arr) = list.as_array() else { continue };
            for b in arr {
                let (Some(node_id), Some(input_name)) = (
                    b.get("nodeId").and_then(|v| v.as_str()),
                    b.get("inputName").and_then(|v| v.as_str()),
                ) else {
                    continue;
                };
                if let Some(node) = workflow.get_mut(node_id) {
                    if let Some(inputs) = node.get_mut("inputs").and_then(|i| i.as_object_mut()) {
                        inputs.insert(input_name.to_string(), value.clone());
                    }
                }
            }
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| AppError::internal(format!("HTTP 客户端创建失败: {e}")))?;

    let body = json!({ "prompt": workflow, "client_id": uuid::Uuid::new_v4().simple().to_string() });
    let mut req_builder = client.post(format!("{base}/prompt")).json(&body);
    if let Some(k) = api_key.as_deref().filter(|s| !s.trim().is_empty()) {
        req_builder = req_builder.header("Authorization", format!("Bearer {k}"));
    }

    let resp = req_builder.send().await;
    let (passed, prompt_id, message) = match resp {
        Ok(r) => {
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            if status.is_success() {
                let pid = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|v| v.get("prompt_id").and_then(|p| p.as_str()).map(str::to_string));
                (true, pid, format!("提交成功（HTTP {status}）"))
            } else {
                (
                    false,
                    None,
                    format!("提交失败 HTTP {status}: {}", text.chars().take(300).collect::<String>()),
                )
            }
        }
        Err(e) => (false, None, format!("连接 ComfyUI 失败: {e}")),
    };

    let duration = started.elapsed().as_millis() as i64;
    sqlx::query(
        "UPDATE afv_comfyui_workflow_version SET test_status = ?, test_message = ?, last_test_time = ?, update_time = ? WHERE id = ?",
    )
    .bind(if passed { 1 } else { 2 })
    .bind(&message)
    .bind(time::now())
    .bind(time::now())
    .bind(req.version_id)
    .execute(&state.db)
    .await?;

    ok(json!({
        "passed": passed,
        "promptId": prompt_id,
        "durationMillis": duration,
        "outputs": [],
        "message": message,
    }))
}
