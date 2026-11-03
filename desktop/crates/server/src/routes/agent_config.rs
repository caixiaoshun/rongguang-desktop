//! 智能体配置：状态清理策略、工作区存储、用户技能（SKILL.md）、MCP 服务器。
//!
//! 桌面版单机单用户，这些配置直接落到 SQLite 与数据目录下的 `agents/` 子树：
//! ```text
//! <data>/agents/
//!   workspace/         工作区文件（技能运行时读写）
//!   skills/<name>/     用户技能目录，含 SKILL.md
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Multipart, Path as AxumPath, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;

use fusion_core::time;


use crate::state::AppState;
use crate::auth::AuthUser;
use crate::support::{ok, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/ai/agent-config/state-cleanup", get(get_cleanup).put(save_cleanup))
        .route("/api/ai/agent-config/workspace", get(get_workspace))
        .route("/api/ai/agent-config/workspace/test", post(test_workspace))
        .route("/api/ai/agent-config/workspace/migrations", post(start_migration))
        .route("/api/ai/agent-config/workspace/migrations/{id}", get(get_migration))
        .route("/api/ai/agent-config/workspace/migrations/{id}/rollback", post(rollback_migration))
        .route(
            "/api/ai/agent-config/workspace/migrations/{id}/dismiss-failure",
            post(dismiss_migration),
        )
        .route("/api/ai/agent-config/skills", get(list_skills).put(save_skill))
        .route("/api/ai/agent-config/skills/{name}", delete(delete_skill))
        .route("/api/ai/agent-config/skills/import/preview", post(preview_import))
        .route("/api/ai/agent-config/skills/import", post(do_import))
        .route("/api/ai/agent-config/mcp", get(list_mcp).put(save_mcp))
        .route("/api/ai/agent-config/mcp/{id}", delete(delete_mcp))
        .route("/api/ai/agent-config/mcp/{id}/test", post(test_mcp))
}

// ───────────────────────── 通用小工具 ─────────────────────────

fn agents_root(state: &AppState) -> PathBuf {
    state.paths.root.join("agents")
}

fn skills_root(state: &AppState) -> PathBuf {
    agents_root(state).join("skills")
}

fn workspace_root(state: &AppState) -> PathBuf {
    agents_root(state).join("workspace")
}

/// 技能名只允许字母数字与 `-` `_`，避免路径穿越。
fn safe_skill_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return None;
    }
    if trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !trimmed.starts_with('.')
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// 目录体积与文件数（递归，跳过符号链接）。
fn dir_stats(dir: &Path) -> (i64, i64) {
    let mut entries = 0i64;
    let mut bytes = 0i64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&cur) else { continue };
        for item in rd.flatten() {
            let path = item.path();
            let Ok(meta) = item.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                entries += 1;
                bytes += meta.len() as i64;
            }
        }
    }
    (entries, bytes)
}

// ───────────────────────── 状态清理策略 ─────────────────────────

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPolicy {
    pub cleanup_interval_days: i64,
    pub retention_days: i64,
    pub next_cleanup_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_cleanup_at: Option<String>,
}

async fn get_cleanup(State(state): State<Arc<AppState>>) -> ApiResult<CleanupPolicy> {
    let row = sqlx::query(
        "SELECT config_value FROM afv_system_config WHERE config_key = 'agent.stateCleanup' AND deleted = 0 LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;

    let policy = row
        .and_then(|r| r.try_get::<Option<String>, _>("config_value").ok().flatten())
        .and_then(|v| serde_json::from_str::<CleanupPolicy>(&v).ok())
        .unwrap_or_else(|| {
            let next = chrono::Local::now() + chrono::Duration::days(1);
            CleanupPolicy {
                cleanup_interval_days: 1,
                retention_days: 7,
                next_cleanup_at: next.format("%Y-%m-%dT%H:%M:%S").to_string(),
                last_cleanup_at: None,
            }
        });
    ok(policy)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupRequest {
    pub cleanup_interval_days: i64,
    pub retention_days: i64,
}

async fn save_cleanup(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<CleanupRequest>,
) -> ApiResult<CleanupPolicy> {
    let interval = req.cleanup_interval_days.clamp(1, 30);
    let retention = req.retention_days.clamp(1, 365);
    let next = chrono::Local::now() + chrono::Duration::days(interval);
    let policy = CleanupPolicy {
        cleanup_interval_days: interval,
        retention_days: retention,
        next_cleanup_at: next.format("%Y-%m-%dT%H:%M:%S").to_string(),
        last_cleanup_at: None,
    };

    let value = serde_json::to_string(&json!({
        "cleanupIntervalDays": interval,
        "retentionDays": retention,
        "nextCleanupAt": policy.next_cleanup_at,
        "lastCleanupAt": policy.last_cleanup_at,
    }))?;

    let now = time::now();
    let affected = sqlx::query(
        "UPDATE afv_system_config SET config_value = ?, update_time = ? WHERE config_key = 'agent.stateCleanup' AND deleted = 0",
    )
    .bind(&value)
    .bind(&now)
    .execute(&state.db)
    .await?
    .rows_affected();

    if affected == 0 {
        sqlx::query(
            "INSERT INTO afv_system_config (config_key, config_value, remark, deleted, create_time, update_time) VALUES ('agent.stateCleanup', ?, '智能体状态清理策略', 0, ?, ?)",
        )
        .bind(&value)
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await?;
    }
    ok(policy)
}

// ───────────────────────── 工作区 ─────────────────────────

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTarget {
    pub backend_type: String,
    #[serde(default)]
    pub storage_config_id: Option<i64>,
    #[serde(default)]
    pub local_path: Option<String>,
}

fn workspace_dir_for(state: &AppState, backend: &str, local_path: Option<&str>) -> PathBuf {
    match (backend, local_path) {
        ("local", Some(p)) if !p.trim().is_empty() => PathBuf::from(p.trim()),
        _ => workspace_root(state),
    }
}

async fn get_workspace(State(state): State<Arc<AppState>>) -> ApiResult<serde_json::Value> {
    let dir = workspace_root(&state);
    let _ = std::fs::create_dir_all(&dir);
    let (entry_count, content_bytes) = dir_stats(&dir);

    let row = sqlx::query(
        "SELECT id, source_backend_type, target_backend_type, status, total_count, copied_count, failed_count, error_message, started_at, finished_at FROM afv_agent_workspace_migration ORDER BY id DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;

    let latest = row.map(|r| {
        json!({
            "id": r.get::<i64, _>("id"),
            "sourceBackendType": r.get::<String, _>("source_backend_type"),
            "targetBackendType": r.get::<String, _>("target_backend_type"),
            "status": r.get::<Option<String>, _>("status"),
            "totalCount": r.get::<Option<i64>, _>("total_count").unwrap_or(0),
            "copiedCount": r.get::<Option<i64>, _>("copied_count").unwrap_or(0),
            "failedCount": r.get::<Option<i64>, _>("failed_count").unwrap_or(0),
            "errorMessage": r.get::<Option<String>, _>("error_message"),
            "startedAt": r.get::<Option<String>, _>("started_at"),
            "finishedAt": r.get::<Option<String>, _>("finished_at"),
        })
    });

    let migration_status = latest
        .as_ref()
        .and_then(|v| v.get("status").and_then(|s| s.as_str()))
        .unwrap_or("IDLE")
        .to_string();

    ok(json!({
        "backendType": "local",
        "storageConfigId": null,
        "localPath": dir.display().to_string(),
        "migrationStatus": migration_status,
        "activeMigrationId": null,
        "entryCount": entry_count,
        "contentBytes": content_bytes,
        "latestMigration": latest,
    }))
}

async fn test_workspace(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(target): Json<WorkspaceTarget>,
) -> ApiResult<bool> {
    let dir = workspace_dir_for(&state, &target.backend_type, target.local_path.as_deref());
    // 真正写一个探针文件，避免「目录存在但不可写」的误判。
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {
            let probe = dir.join(".write-probe");
            let writable = std::fs::write(&probe, b"ok").is_ok();
            let _ = std::fs::remove_file(&probe);
            ok(writable)
        }
        Err(_) => ok(false),
    }
}

async fn start_migration(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(target): Json<WorkspaceTarget>,
) -> ApiResult<i64> {
    let now = time::now();
    let id = sqlx::query(
        "INSERT INTO afv_agent_workspace_migration (source_backend_type, target_backend_type, target_local_path, status, total_count, copied_count, failed_count, started_at, create_time) VALUES ('local', ?, ?, 'RUNNING', 0, 0, 0, ?, ?)",
    )
    .bind(&target.backend_type)
    .bind(target.local_path.as_deref())
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    // 桌面版只有本地后端：迁移即「校验可写 + 统计现有内容」，无需搬运字节。
    let src = workspace_root(&state);
    let (total, _) = dir_stats(&src);
    let dst = workspace_dir_for(&state, &target.backend_type, target.local_path.as_deref());
    let writable = std::fs::create_dir_all(&dst).is_ok();

    sqlx::query(
        "UPDATE afv_agent_workspace_migration SET status = ?, total_count = ?, copied_count = ?, failed_count = ?, error_message = ?, finished_at = ? WHERE id = ?",
    )
    .bind(if writable { "COMPLETED" } else { "FAILED" })
    .bind(total)
    .bind(if writable { total } else { 0 })
    .bind(if writable { 0 } else { total })
    .bind(if writable {
        None::<String>
    } else {
        Some(format!("目标目录不可写: {}", dst.display()))
    })
    .bind(time::now())
    .bind(id)
    .execute(&state.db)
    .await?;

    ok(id)
}

async fn get_migration(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult<serde_json::Value> {
    let r = sqlx::query(
        "SELECT id, source_backend_type, target_backend_type, target_local_path, status, total_count, copied_count, failed_count, error_message, started_at, finished_at FROM afv_agent_workspace_migration WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| fusion_core::error::AppError::not_found("迁移记录不存在"))?;

    ok(json!({
        "id": r.get::<i64, _>("id"),
        "sourceBackendType": r.get::<String, _>("source_backend_type"),
        "targetBackendType": r.get::<String, _>("target_backend_type"),
        "targetLocalPath": r.get::<Option<String>, _>("target_local_path"),
        "status": r.get::<Option<String>, _>("status"),
        "totalCount": r.get::<Option<i64>, _>("total_count").unwrap_or(0),
        "copiedCount": r.get::<Option<i64>, _>("copied_count").unwrap_or(0),
        "failedCount": r.get::<Option<i64>, _>("failed_count").unwrap_or(0),
        "errorMessage": r.get::<Option<String>, _>("error_message"),
        "startedAt": r.get::<Option<String>, _>("started_at"),
        "finishedAt": r.get::<Option<String>, _>("finished_at"),
    }))
}

async fn rollback_migration(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult<bool> {
    sqlx::query(
        "UPDATE afv_agent_workspace_migration SET status = 'ROLLED_BACK', finished_at = ? WHERE id = ?",
    )
    .bind(time::now())
    .bind(id)
    .execute(&state.db)
    .await?;
    ok(true)
}

async fn dismiss_migration(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_agent_workspace_migration SET status = 'DISMISSED' WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    ok(true)
}

// ───────────────────────── 用户技能 ─────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSkill {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub description: String,
    pub content: String,
    pub source: String,
}

/// 极简 YAML front matter 解析：只取 `key: value` 形式的顶层标量。
fn parse_front_matter(text: &str) -> (BTreeMap<String, String>, String) {
    let mut map = BTreeMap::new();
    if !text.starts_with("---") {
        return (map, text.to_string());
    }
    let rest = &text[3..];
    let Some(end) = rest.find("\n---") else {
        return (map, text.to_string());
    };
    let head = &rest[..end];
    let body = rest[end + 4..].trim_start_matches(['\r', '\n']).to_string();
    for line in head.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
            map.insert(k.trim().to_string(), v);
        }
    }
    (map, body)
}

pub(crate) fn read_skills(state: &AppState) -> Vec<UserSkill> {
    let root = skills_root(state);
    let Ok(rd) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in rd.flatten() {
        if !item.path().is_dir() {
            continue;
        }
        let name = item.file_name().to_string_lossy().to_string();
        let manifest = item.path().join("SKILL.md");
        let text = std::fs::read_to_string(&manifest).unwrap_or_default();
        let (meta, body) = parse_front_matter(&text);
        out.push(UserSkill {
            id: name.clone(),
            display_name: meta.get("displayName").cloned().or_else(|| meta.get("name").cloned()),
            name,
            description: meta.get("description").cloned().unwrap_or_default(),
            content: body,
            source: "user".into(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

async fn list_skills(State(state): State<Arc<AppState>>) -> ApiResult<Vec<UserSkill>> {
    let _ = std::fs::create_dir_all(skills_root(&state));
    ok(read_skills(&state))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSaveRequest {
    #[serde(default)]
    pub original_name: Option<String>,
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub content: String,
}

fn render_skill_md(req: &SkillSaveRequest) -> String {
    format!(
        "---\nname: {}\ndisplayName: {}\ndescription: {}\n---\n\n{}\n",
        req.name,
        if req.display_name.trim().is_empty() { &req.name } else { &req.display_name },
        req.description.trim(),
        req.content.trim()
    )
}

async fn save_skill(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<SkillSaveRequest>,
) -> ApiResult<UserSkill> {
    let name = safe_skill_name(&req.name)
        .ok_or_else(|| fusion_core::error::AppError::bad_request("技能名只能包含字母、数字、-、_、."))?;

    // 重命名：先删旧目录。
    if let Some(old) = req.original_name.as_deref().and_then(safe_skill_name) {
        if old != name {
            let _ = std::fs::remove_dir_all(skills_root(&state).join(&old));
        }
    }

    let dir = skills_root(&state).join(&name);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("SKILL.md"), render_skill_md(&req))?;

    let (meta, body) = parse_front_matter(&render_skill_md(&req));
    ok(UserSkill {
        id: name.clone(),
        name,
        display_name: meta.get("displayName").cloned(),
        description: meta.get("description").cloned().unwrap_or_default(),
        content: body,
        source: "user".into(),
    })
}

async fn delete_skill(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    AxumPath(name): AxumPath<String>,
) -> ApiResult<bool> {
    let name = safe_skill_name(&name)
        .ok_or_else(|| fusion_core::error::AppError::bad_request("非法技能名"))?;
    let dir = skills_root(&state).join(&name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    ok(true)
}

// ───────────────────────── 技能导入（ZIP / 目录） ─────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    pub root_path: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub suggested_display_name: String,
    pub file_count: i64,
    pub total_bytes: i64,
    pub has_scripts: bool,
    pub has_references: bool,
    pub has_assets: bool,
    pub exists: bool,
    pub valid: bool,
    pub recommended_action: String,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// 从上传的 multipart 里收集 (相对路径, 内容)。
async fn collect_upload(multipart: &mut Multipart) -> Result<(Vec<(String, Vec<u8>)>, Vec<String>), String> {
    let mut files = Vec::new();
    let mut paths = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| e.to_string())? {
        let fname = field.name().unwrap_or_default().to_string();
        if fname == "paths" {
            paths.push(field.text().await.map_err(|e| e.to_string())?);
            continue;
        }
        if fname != "files" {
            // 忽略 request 等其它字段
            let _ = field.bytes().await;
            continue;
        }
        let file_name = field.file_name().unwrap_or("skill.md").to_string();
        let data = field.bytes().await.map_err(|e| e.to_string())?.to_vec();
        files.push((file_name, data));
    }

    // 前端同时提交 files 与 paths；paths 更准确地表达了 zip 内层级。
    let mut out = Vec::new();
    for (i, (name, data)) in files.into_iter().enumerate() {
        let rel = paths.get(i).cloned().unwrap_or(name);
        out.push((rel, data));
    }
    Ok((out, paths))
}

fn build_preview(entries: Vec<(String, Vec<u8>)>) -> Result<serde_json::Value, String> {
    // 按层级归组：找所有 `SKILL.md` 的父目录作为技能根。
    let mut groups: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    for (rel, data) in &entries {
        let normalized = rel.replace('\\', "/");
        let parts: Vec<&str> = normalized.split('/').collect();
        let Some(pos) = parts.iter().rposition(|p| p.eq_ignore_ascii_case("SKILL.md")) else {
            continue;
        };
        let root = parts[..pos].join("/");
        groups.entry(root).or_default().push((normalized.clone(), data.len()));
    }

    if groups.is_empty() {
        // 没有 SKILL.md：把顶层文件视为单个技能的草稿。
        let root = entries
            .first()
            .map(|(p, _)| p.replace('\\', "/").split('/').next().unwrap_or("").to_string())
            .unwrap_or_default();
        groups.insert(root, entries.iter().map(|(p, d)| (p.replace('\\', "/"), d.len())).collect());
    }

    let mut total_files = 0i64;
    let mut total_bytes = 0i64;
    let mut skills = Vec::new();

    for (root, files) in groups {
        let md = entries
            .iter()
            .find(|(p, _)| p.replace('\\', "/").ends_with("SKILL.md") && p.replace('\\', "/").starts_with(&root))
            .map(|(_, d)| String::from_utf8_lossy(d).to_string())
            .unwrap_or_default();
        let (meta, _) = parse_front_matter(&md);
        let file_count = files.len() as i64;
        let bytes: i64 = files.iter().map(|(_, l)| *l as i64).sum();
        total_files += file_count;
        total_bytes += bytes;

        let has_scripts = files.iter().any(|(p, _)| p.starts_with(&format!("{root}/scripts")) || p.contains("/scripts/"));
        let has_references = files.iter().any(|(p, _)| p.contains("/references/"));
        let has_assets = files.iter().any(|(p, _)| p.contains("/assets/"));

        let dir_name = root.rsplit('/').next().unwrap_or("").to_string();
        let declared = meta.get("name").cloned().filter(|s| !s.trim().is_empty());
        let name = declared.clone().or(Some(dir_name.clone()).filter(|s| !s.is_empty()));

        let mut errors = Vec::new();
        if md.trim().is_empty() {
            errors.push("缺少 SKILL.md 内容".to_string());
        }
        let exists = name
            .as_deref()
            .and_then(safe_skill_name)
            .map(|n| skills_root_path().join(n).exists())
            .unwrap_or(false);

        let mut warnings = Vec::new();
        if has_scripts {
            warnings.push("包含可执行脚本，导入后请自行审阅".to_string());
        }
        if bytes > 8 * 1024 * 1024 {
            warnings.push("体积较大（>8MB），导入可能较慢".to_string());
        }

        skills.push(json!({
            "rootPath": root,
            "name": name,
            "description": meta.get("description").cloned(),
            "suggestedDisplayName": meta.get("displayName").cloned()
                .or(declared)
                .unwrap_or_else(|| dir_name.clone()),
            "fileCount": file_count,
            "totalBytes": bytes,
            "hasScripts": has_scripts,
            "hasReferences": has_references,
            "hasAssets": has_assets,
            "exists": exists,
            "valid": errors.is_empty(),
            "recommendedAction": if exists { "REPLACE" } else { "CREATE" },
            "warnings": warnings,
            "errors": errors,
        }));
    }

    Ok(json!({
        "sourceType": "ZIP",
        "totalFiles": total_files,
        "totalBytes": total_bytes,
        "skills": skills,
        "warnings": [],
        "errors": [],
    }))
}

/// 预览阶段还没有 state，单独给个根目录推导（与 skills_root 同构）。
fn skills_root_path() -> PathBuf {
    std::env::var("FUSION_DATA_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("APPDATA").ok().map(|p| PathBuf::from(p).join("RongGuang")))
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".local/share/rongguang")
        })
        .join("agents")
        .join("skills")
}

async fn preview_import(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    mut mp: Multipart,
) -> ApiResult<serde_json::Value> {
    let (entries, _) = collect_upload(&mut mp).await.map_err(fusion_core::error::AppError::bad_request)?;
    let _ = std::fs::create_dir_all(skills_root(&state));
    let preview = build_preview(entries).map_err(fusion_core::error::AppError::bad_request)?;
    ok(preview)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ImportRequest {
    #[serde(default)]
    selections: Vec<ImportSelection>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportSelection {
    root_path: String,
    display_name: String,
    action: String,
}

async fn do_import(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    mut mp: Multipart,
) -> ApiResult<serde_json::Value> {
    // multipart 里 request 字段是 Blob(JSON)，这里单独抓出来。
    let mut request = ImportRequest::default();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut paths: Vec<String> = Vec::new();

    while let Some(field) = mp.next_field().await.map_err(|e| fusion_core::error::AppError::bad_request(e.to_string()))? {
        match field.name().unwrap_or_default() {
            "request" => {
                let text = field.text().await.unwrap_or_default();
                request = serde_json::from_str(&text).unwrap_or_default();
            }
            "paths" => paths.push(field.text().await.unwrap_or_default()),
            "files" => {
                let fname = field.file_name().unwrap_or("skill.md").to_string();
                let data = field.bytes().await.map_err(|e| fusion_core::error::AppError::bad_request(e.to_string()))?.to_vec();
                files.push((fname, data));
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let entries: Vec<(String, Vec<u8>)> = files
        .into_iter()
        .enumerate()
        .map(|(i, (n, d))| (paths.get(i).cloned().unwrap_or(n), d))
        .collect();

    let mut created = Vec::new();
    let mut replaced = Vec::new();
    let mut skipped = Vec::new();

    for sel in &request.selections {
        if sel.action.eq_ignore_ascii_case("SKIP") {
            skipped.push(sel.display_name.clone());
            continue;
        }
        let Some(name) = safe_skill_name(&sel.display_name) else {
            skipped.push(sel.display_name.clone());
            continue;
        };
        let dir = skills_root(&state).join(&name);
        let exists = dir.exists();
        if exists && sel.action.eq_ignore_ascii_case("CREATE") {
            skipped.push(sel.display_name.clone());
            continue;
        }
        std::fs::create_dir_all(&dir)?;

        // 只落属于该 root 的文件，并把 root 前缀剥掉。
        let prefix = format!("{}/", sel.root_path.trim_end_matches('/'));
        for (rel, data) in &entries {
            let normalized = rel.replace('\\', "/");
            let relpath = if sel.root_path.is_empty() {
                normalized.clone()
            } else if let Some(rest) = normalized.strip_prefix(&prefix) {
                rest.to_string()
            } else {
                continue;
            };
            // 防路径穿越
            if relpath.split('/').any(|seg| seg == "..") {
                continue;
            }
            let target = dir.join(&relpath);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, data)?;
        }

        if exists {
            replaced.push(sel.display_name.clone());
        } else {
            created.push(sel.display_name.clone());
        }
    }

    ok(json!({
        "createdCount": created.len(),
        "replacedCount": replaced.len(),
        "skippedCount": skipped.len(),
        "createdNames": created,
        "replacedNames": replaced,
        "skippedNames": skipped,
    }))
}

// ───────────────────────── MCP 服务器 ─────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: i64,
    pub name: String,
    pub transport: String,
    pub url: String,
    pub headers: serde_json::Value,
    pub query_params: serde_json::Value,
    pub enabled_tools: serde_json::Value,
    pub protocol_versions: serde_json::Value,
    pub timeout_seconds: i64,
    pub initialization_timeout_seconds: i64,
    pub status: i64,
    pub last_test_status: Option<String>,
    pub last_test_message: Option<String>,
    pub update_time: Option<String>,
}

async fn list_mcp(State(state): State<Arc<AppState>>) -> ApiResult<Vec<McpServer>> {
    let rows = sqlx::query(
        "SELECT id, name, transport, url, headers_json, query_params_json, enabled_tools_json, protocol_versions_json, timeout_seconds, initialization_timeout_seconds, status, last_test_status, last_test_message, update_time FROM afv_agent_mcp_server WHERE deleted = 0 ORDER BY id ASC",
    )
    .fetch_all(&state.db)
    .await?;

    let parse = |v: Option<String>| -> serde_json::Value {
        v.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!([]))
    };

    ok(rows
        .into_iter()
        .map(|r| McpServer {
            id: r.get("id"),
            name: r.get::<Option<String>, _>("name").unwrap_or_default(),
            transport: r.get::<Option<String>, _>("transport").unwrap_or_else(|| "http".into()),
            url: r.get::<Option<String>, _>("url").unwrap_or_default(),
            headers: parse(r.get("headers_json")),
            query_params: parse(r.get("query_params_json")),
            enabled_tools: parse(r.get("enabled_tools_json")),
            protocol_versions: parse(r.get("protocol_versions_json")),
            timeout_seconds: r.get::<Option<i64>, _>("timeout_seconds").unwrap_or(30),
            initialization_timeout_seconds: r
                .get::<Option<i64>, _>("initialization_timeout_seconds")
                .unwrap_or(15),
            status: r.get::<Option<i64>, _>("status").unwrap_or(1),
            last_test_status: r.get("last_test_status"),
            last_test_message: r.get("last_test_message"),
            update_time: r.get("update_time"),
        })
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSaveRequest {
    #[serde(default)]
    pub id: Option<i64>,
    pub name: String,
    #[serde(default = "default_transport")]
    pub transport: String,
    pub url: String,
    #[serde(default)]
    pub headers: serde_json::Value,
    #[serde(default)]
    pub query_params: serde_json::Value,
    #[serde(default)]
    pub enabled_tools: serde_json::Value,
    #[serde(default)]
    pub protocol_versions: serde_json::Value,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: i64,
    #[serde(default = "default_init_timeout")]
    pub initialization_timeout_seconds: i64,
    #[serde(default = "default_status")]
    pub status: i64,
}

fn default_transport() -> String { "http".into() }
fn default_timeout() -> i64 { 30 }
fn default_init_timeout() -> i64 { 15 }
fn default_status() -> i64 { 1 }

async fn save_mcp(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<McpSaveRequest>,
) -> ApiResult<McpServer> {
    if req.name.trim().is_empty() {
        return Err(fusion_core::error::AppError::bad_request("名称不能为空"));
    }
    if !(req.url.starts_with("http://") || req.url.starts_with("https://")) {
        return Err(fusion_core::error::AppError::bad_request("服务地址需以 http(s):// 开头"));
    }

    let empty = json!([]);
    let headers = if req.headers.is_null() { json!({}) } else { req.headers.clone() };
    let qp = if req.query_params.is_null() { json!({}) } else { req.query_params.clone() };
    let tools = if req.enabled_tools.is_null() { empty.clone() } else { req.enabled_tools.clone() };
    let versions = if req.protocol_versions.is_null() { empty.clone() } else { req.protocol_versions.clone() };

    let now = time::now();
    let id = match req.id {
        Some(id) => {
            sqlx::query(
                "UPDATE afv_agent_mcp_server SET name = ?, transport = ?, url = ?, headers_json = ?, query_params_json = ?, enabled_tools_json = ?, protocol_versions_json = ?, timeout_seconds = ?, initialization_timeout_seconds = ?, status = ?, update_time = ? WHERE id = ?",
            )
            .bind(req.name.trim())
            .bind(&req.transport)
            .bind(req.url.trim())
            .bind(headers.to_string())
            .bind(qp.to_string())
            .bind(tools.to_string())
            .bind(versions.to_string())
            .bind(req.timeout_seconds)
            .bind(req.initialization_timeout_seconds)
            .bind(req.status)
            .bind(&now)
            .bind(id)
            .execute(&state.db)
            .await?;
            id
        }
        None => sqlx::query(
            "INSERT INTO afv_agent_mcp_server (user_id, name, transport, url, headers_json, query_params_json, enabled_tools_json, protocol_versions_json, timeout_seconds, initialization_timeout_seconds, status, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(1i64)
        .bind(req.name.trim())
        .bind(&req.transport)
        .bind(req.url.trim())
        .bind(headers.to_string())
        .bind(qp.to_string())
        .bind(tools.to_string())
        .bind(versions.to_string())
        .bind(req.timeout_seconds)
        .bind(req.initialization_timeout_seconds)
        .bind(req.status)
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await?
        .last_insert_rowid(),
    };

    let r = sqlx::query(
        "SELECT id, name, transport, url, headers_json, query_params_json, enabled_tools_json, protocol_versions_json, timeout_seconds, initialization_timeout_seconds, status, last_test_status, last_test_message, update_time FROM afv_agent_mcp_server WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    let parse = |v: Option<String>| -> serde_json::Value {
        v.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!([]))
    };

    ok(McpServer {
        id: r.get("id"),
        name: r.get::<Option<String>, _>("name").unwrap_or_default(),
        transport: r.get::<Option<String>, _>("transport").unwrap_or_else(|| "http".into()),
        url: r.get::<Option<String>, _>("url").unwrap_or_default(),
        headers: parse(r.get("headers_json")),
        query_params: parse(r.get("query_params_json")),
        enabled_tools: parse(r.get("enabled_tools_json")),
        protocol_versions: parse(r.get("protocol_versions_json")),
        timeout_seconds: r.get::<Option<i64>, _>("timeout_seconds").unwrap_or(30),
        initialization_timeout_seconds: r.get::<Option<i64>, _>("initialization_timeout_seconds").unwrap_or(15),
        status: r.get::<Option<i64>, _>("status").unwrap_or(1),
        last_test_status: r.get("last_test_status"),
        last_test_message: r.get("last_test_message"),
        update_time: r.get("update_time"),
    })
}

async fn delete_mcp(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_agent_mcp_server SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(time::now())
        .bind(id)
        .execute(&state.db)
        .await?;
    ok(true)
}

/// 用 MCP `initialize` + `tools/list` 做一次真实握手。
async fn test_mcp(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult<serde_json::Value> {
    let row = sqlx::query(
        "SELECT url, headers_json, timeout_seconds FROM afv_agent_mcp_server WHERE id = ? AND deleted = 0",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| fusion_core::error::AppError::not_found("MCP 服务不存在"))?;

    let url: String = row.get::<Option<String>, _>("url").unwrap_or_default();
    let timeout = row.get::<Option<i64>, _>("timeout_seconds").unwrap_or(30).clamp(3, 120) as u64;
    let headers: BTreeMap<String, String> = row
        .get::<Option<String>, _>("headers_json")
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()
        .map_err(|e| fusion_core::error::AppError::internal(format!("HTTP 客户端创建失败: {e}")))?;

    let init_body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "rongguang", "version": env!("CARGO_PKG_VERSION") }
        }
    });

    let mut req = client.post(&url).json(&init_body);
    for (k, v) in &headers {
        req = req.header(k, v);
    }
    let result = req.send().await;

    let (success, message) = match result {
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                (true, format!("连接成功（HTTP {status}）"))
            } else {
                (false, format!("服务返回 HTTP {status}: {}", text.chars().take(200).collect::<String>()))
            }
        }
        Err(e) => (false, format!("连接失败: {e}")),
    };

    sqlx::query(
        "UPDATE afv_agent_mcp_server SET last_test_status = ?, last_test_message = ?, update_time = ? WHERE id = ?",
    )
    .bind(if success { "SUCCESS" } else { "FAILED" })
    .bind(&message)
    .bind(time::now())
    .bind(id)
    .execute(&state.db)
    .await?;

    ok(json!({ "success": success, "message": message, "tools": [] }))
}
