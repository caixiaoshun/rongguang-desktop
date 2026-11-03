//! 存储配置与文件上传。
//!
//! 桌面版默认使用本地媒体库；S3 兼容存储保留配置能力，上传仍走本地（离线优先）。

use std::sync::Arc;

use axum::extract::{Multipart, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use fusion_core::error::AppError;
use fusion_core::models::storage::{StorageConfig, StorageConfigSaveReq, StorageTestResult};

use crate::auth::AuthUser;
use crate::state::AppState;
use crate::support::{now, ok, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/storage/upload", post(upload))
        .route("/api/storage/assistant-upload", post(assistant_upload))
        .route("/api/storage/config/create", post(config_create))
        .route("/api/storage/config/update", put(config_update))
        .route(
            "/api/storage/config/delete",
            axum::routing::delete(config_delete),
        )
        .route("/api/storage/config/get", get(config_get))
        .route("/api/storage/config/list", get(config_list))
        .route("/api/storage/config/set-default", put(set_default))
        .route("/api/storage/config/test", post(config_test))
}

fn ext_from_name(name: &str, mime: &str) -> String {
    if let Some(ext) = std::path::Path::new(name).extension().and_then(|e| e.to_str()) {
        if !ext.is_empty() {
            return ext.to_lowercase();
        }
    }
    fusion_ai::media::ext_from_mime(mime).to_string()
}

async fn read_upload(mut multipart: Multipart) -> Result<(String, String, Vec<u8>, String), AppError> {
    let mut file_name = String::from("upload.bin");
    let mut content_type = String::from("application/octet-stream");
    let mut sub_dir = String::from("uploads");
    let mut data: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("解析上传内容失败: {e}")))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "file" => {
                if let Some(fname) = field.file_name() {
                    file_name = fname.to_string();
                }
                if let Some(ctype) = field.content_type() {
                    content_type = ctype.to_string();
                }
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::bad_request(format!("读取文件失败: {e}")))?;
                data = Some(bytes.to_vec());
            }
            "subDir" => {
                sub_dir = field.text().await.unwrap_or_else(|_| "uploads".into());
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let bytes = data.ok_or_else(|| AppError::bad_request("未收到文件"))?;
    if bytes.is_empty() {
        return Err(AppError::bad_request("文件内容为空"));
    }
    Ok((file_name, content_type, bytes, sub_dir))
}

async fn upload(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    multipart: Multipart,
) -> ApiResult<String> {
    let (file_name, content_type, bytes, sub_dir) = read_upload(multipart).await?;
    if bytes.len() > 200 * 1024 * 1024 {
        return Err(AppError::bad_request("文件超过 200MB 上限"));
    }
    let ext = ext_from_name(&file_name, &content_type);
    let kind = match content_type.as_str() {
        t if t.starts_with("video/") => "videos",
        t if t.starts_with("audio/") => "audios",
        _ => "images",
    };
    let _ = sub_dir;
    let url = state.media.save_dedup(kind, &ext, &bytes)?;
    ok(url)
}

async fn assistant_upload(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    multipart: Multipart,
) -> ApiResult<String> {
    let (file_name, content_type, bytes, _) = read_upload(multipart).await?;
    let ext = ext_from_name(&file_name, &content_type);
    let kind = match content_type.as_str() {
        t if t.starts_with("video/") => "videos",
        t if t.starts_with("audio/") => "audios",
        _ => "images",
    };
    let url = state.media.save_dedup(kind, &ext, &bytes)?;
    ok(url)
}

async fn config_create(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<StorageConfigSaveReq>,
) -> ApiResult<i64> {
    let id = sqlx::query(
        "INSERT INTO afv_storage_config (name, type, provider, endpoint, bucket_name, access_key, secret_key, region, base_path, custom_domain, options, is_default, status, remark, deleted, create_time, update_time)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
    )
    .bind(&req.name)
    .bind(req.storage_type.clone().unwrap_or_else(|| "local".into()))
    .bind(req.provider.clone().unwrap_or_else(|| "local".into()))
    .bind(req.endpoint.clone())
    .bind(req.bucket_name.clone())
    .bind(req.access_key.clone())
    .bind(req.secret_key.clone())
    .bind(req.region.clone())
    .bind(req.base_path.clone())
    .bind(req.custom_domain.clone())
    .bind(req.options.as_ref().map(|o| o.to_string()))
    .bind(req.is_default.unwrap_or(false))
    .bind(req.status.unwrap_or(1))
    .bind(req.remark.clone())
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();
    ok(id)
}

async fn config_update(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<StorageConfigSaveReq>,
) -> ApiResult<bool> {
    let id = req.id.ok_or_else(|| AppError::bad_request("缺少配置 ID"))?;
    sqlx::query(
        "UPDATE afv_storage_config SET name = COALESCE(?, name), type = COALESCE(?, type),
            provider = COALESCE(?, provider), endpoint = COALESCE(?, endpoint),
            bucket_name = COALESCE(?, bucket_name), access_key = COALESCE(?, access_key),
            secret_key = COALESCE(?, secret_key), region = COALESCE(?, region),
            base_path = COALESCE(?, base_path), custom_domain = COALESCE(?, custom_domain),
            options = COALESCE(?, options), is_default = COALESCE(?, is_default),
            status = COALESCE(?, status), remark = COALESCE(?, remark), update_time = ?
         WHERE id = ?",
    )
    .bind(&req.name)
    .bind(req.storage_type.clone())
    .bind(req.provider.clone())
    .bind(req.endpoint.clone())
    .bind(req.bucket_name.clone())
    .bind(req.access_key.clone())
    .bind(req.secret_key.clone())
    .bind(req.region.clone())
    .bind(req.base_path.clone())
    .bind(req.custom_domain.clone())
    .bind(req.options.as_ref().map(|o| o.to_string()))
    .bind(req.is_default)
    .bind(req.status)
    .bind(req.remark.clone())
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;
    ok(true)
}

#[derive(Deserialize)]
struct IdQuery {
    id: i64,
}

async fn config_delete(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_storage_config SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(q.id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn config_get(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<StorageConfig> {
    let config = sqlx::query_as::<_, StorageConfig>(
        "SELECT * FROM afv_storage_config WHERE id = ? AND deleted = 0",
    )
    .bind(q.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("存储配置不存在"))?;
    ok(config)
}

async fn config_list(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
) -> ApiResult<Vec<StorageConfig>> {
    let rows = sqlx::query_as::<_, StorageConfig>(
        "SELECT * FROM afv_storage_config WHERE deleted = 0 ORDER BY is_default DESC, id ASC",
    )
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn set_default(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_storage_config SET is_default = 0, update_time = ? WHERE is_default = 1")
        .bind(now())
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE afv_storage_config SET is_default = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(q.id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn config_test(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<StorageConfigSaveReq>,
) -> ApiResult<StorageTestResult> {
    let storage_type = req.storage_type.clone().unwrap_or_else(|| "local".into());
    if storage_type == "local" {
        let dir = req
            .base_path
            .clone()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(|| state.paths.media_dir.to_string_lossy().to_string());
        let probe = std::path::Path::new(&dir).join(".rg-write-test");
        return Ok(Json(fusion_core::error::ApiResponse::ok(
            match std::fs::write(&probe, b"ok") {
                Ok(()) => {
                    let _ = std::fs::remove_file(&probe);
                    StorageTestResult {
                        success: true,
                        message: "本地目录可写".into(),
                        public_url: Some("/media".into()),
                    }
                }
                Err(err) => StorageTestResult {
                    success: false,
                    message: format!("目录不可写: {err}"),
                    public_url: None,
                },
            },
        )));
    }

    Ok(Json(fusion_core::error::ApiResponse::ok(
        StorageTestResult {
            success: false,
            message: "桌面版未启用 S3 直传，请使用本地媒体库".into(),
            public_url: None,
        },
    )))
}
