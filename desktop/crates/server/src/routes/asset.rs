//! 资产库：角色/场景/道具及其子项。

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;

use fusion_core::error::AppError;
use fusion_core::models::asset::{Asset, AssetCreateReq, AssetItem, AssetItemSaveReq, AssetUpdateReq};

use crate::auth::AuthUser;
use crate::state::AppState;
use crate::support::{now, ok, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/asset", post(create).put(update))
        .route("/api/asset/{id}", get(get_asset).delete(delete_asset))
        .route("/api/asset/list", get(list))
        .route("/api/asset/list-with-items", get(list_with_items))
        .route("/api/asset/all", get(list_all))
        .route("/api/asset/metadata/{asset_type}", get(metadata))
        .route("/api/asset/item/{id}", get(get_item).delete(delete_item))
        .route("/api/asset/{asset_id}/items", get(list_items))
        .route("/api/asset/item", post(save_item).put(save_item))
}

async fn load_asset(state: &AppState, id: i64) -> Result<Asset, AppError> {
    sqlx::query_as::<_, Asset>("SELECT * FROM afv_asset WHERE id = ? AND deleted = 0")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("资产不存在"))
}

async fn load_item(state: &AppState, id: i64) -> Result<AssetItem, AppError> {
    sqlx::query_as::<_, AssetItem>("SELECT * FROM afv_asset_item WHERE id = ? AND deleted = 0")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("子资产不存在"))
}

async fn create(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<AssetCreateReq>,
) -> ApiResult<Asset> {
    if req.name.trim().is_empty() {
        return Err(AppError::bad_request("资产名称不能为空"));
    }
    if req.asset_type.trim().is_empty() {
        return Err(AppError::bad_request("资产类型不能为空"));
    }
    let tags = req.tags.map(|t| serde_json::to_string(&t).unwrap_or_else(|_| "[]".into()));
    let id = sqlx::query(
        "INSERT INTO afv_asset (user_id, project_id, type, name, description, cover_url, properties, tags, source_type, ai_prompt, owner_type, owner_id, status, deleted, create_time, update_time)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, 0, 0, ?, ?)",
    )
    .bind(user.id)
    .bind(req.project_id)
    .bind(req.asset_type.trim())
    .bind(req.name.trim())
    .bind(req.description)
    .bind(req.cover_url)
    .bind(req.properties)
    .bind(tags)
    .bind(req.source_type.unwrap_or(0))
    .bind(req.ai_prompt)
    .bind(user.id)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();
    ok(load_asset(&state, id).await?)
}

async fn update(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<AssetUpdateReq>,
) -> ApiResult<Asset> {
    let tags = req.tags.map(|t| serde_json::to_string(&t).unwrap_or_else(|_| "[]".into()));
    sqlx::query(
        "UPDATE afv_asset SET type = COALESCE(?, type), name = COALESCE(?, name),
            description = COALESCE(?, description), cover_url = COALESCE(?, cover_url),
            properties = COALESCE(?, properties), tags = COALESCE(?, tags),
            status = COALESCE(?, status), ai_prompt = COALESCE(?, ai_prompt), update_time = ?
         WHERE id = ? AND deleted = 0",
    )
    .bind(req.asset_type)
    .bind(req.name)
    .bind(req.description)
    .bind(req.cover_url)
    .bind(req.properties)
    .bind(tags)
    .bind(req.status)
    .bind(req.ai_prompt)
    .bind(now())
    .bind(req.id)
    .execute(&state.db)
    .await?;
    ok(load_asset(&state, req.id).await?)
}

async fn get_asset(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<Asset> {
    ok(load_asset(&state, id).await?)
}

async fn delete_asset(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_asset SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE afv_asset_item SET deleted = 1, update_time = ? WHERE asset_id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    ok(true)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct AssetListQuery {
    project_id: Option<i64>,
    #[serde(rename = "type")]
    asset_type: Option<String>,
    keyword: Option<String>,
}

impl Default for AssetListQuery {
    fn default() -> Self {
        Self {
            project_id: None,
            asset_type: None,
            keyword: None,
        }
    }
}

async fn list(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<AssetListQuery>,
) -> ApiResult<Vec<Asset>> {
    let keyword = format!("%{}%", q.keyword.unwrap_or_default());
    let rows = sqlx::query_as::<_, Asset>(
        "SELECT * FROM afv_asset WHERE deleted = 0
           AND (? IS NULL OR project_id = ?)
           AND (? IS NULL OR type = ?)
           AND (name LIKE ? OR IFNULL(description,'') LIKE ?)
         ORDER BY id ASC",
    )
    .bind(q.project_id)
    .bind(q.project_id)
    .bind(q.asset_type.clone())
    .bind(q.asset_type.clone())
    .bind(&keyword)
    .bind(&keyword)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn list_with_items(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<AssetListQuery>,
) -> ApiResult<Vec<serde_json::Value>> {
    let assets = sqlx::query_as::<_, Asset>(
        "SELECT * FROM afv_asset WHERE deleted = 0 AND (? IS NULL OR project_id = ?) ORDER BY id ASC",
    )
    .bind(q.project_id)
    .bind(q.project_id)
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(assets.len());
    for asset in assets {
        let items = sqlx::query_as::<_, AssetItem>(
            "SELECT * FROM afv_asset_item WHERE asset_id = ? AND deleted = 0 ORDER BY sort_order ASC, id ASC",
        )
        .bind(asset.id)
        .fetch_all(&state.db)
        .await?;
        let mut value = serde_json::to_value(&asset)?;
        if let Some(map) = value.as_object_mut() {
            map.insert("items".into(), serde_json::to_value(&items)?);
        }
        result.push(value);
    }
    ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct AssetAllQuery {
    project_id: Option<i64>,
    #[serde(rename = "type")]
    asset_type: Option<String>,
    keyword: Option<String>,
    page: Option<i64>,
    size: Option<i64>,
}

impl Default for AssetAllQuery {
    fn default() -> Self {
        Self {
            project_id: None,
            asset_type: None,
            keyword: None,
            page: None,
            size: None,
        }
    }
}

async fn list_all(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Query(q): Query<AssetAllQuery>,
) -> ApiResult<serde_json::Value> {
    let page = q.page.unwrap_or(1).max(1);
    let size = q.size.unwrap_or(20).clamp(1, 200);
    let offset = (page - 1) * size;
    let keyword = format!("%{}%", q.keyword.unwrap_or_default());

    let rows = sqlx::query_as::<_, Asset>(
        "SELECT * FROM afv_asset WHERE deleted = 0
           AND (? IS NULL OR project_id = ?)
           AND (? IS NULL OR type = ?)
           AND (name LIKE ? OR IFNULL(description,'') LIKE ?)
         ORDER BY update_time DESC, id DESC LIMIT ? OFFSET ?",
    )
    .bind(q.project_id)
    .bind(q.project_id)
    .bind(q.asset_type.clone())
    .bind(q.asset_type.clone())
    .bind(&keyword)
    .bind(&keyword)
    .bind(size)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(1) FROM afv_asset WHERE deleted = 0
           AND (? IS NULL OR project_id = ?)
           AND (? IS NULL OR type = ?)
           AND (name LIKE ? OR IFNULL(description,'') LIKE ?)",
    )
    .bind(q.project_id)
    .bind(q.project_id)
    .bind(q.asset_type.clone())
    .bind(q.asset_type.clone())
    .bind(&keyword)
    .bind(&keyword)
    .fetch_one(&state.db)
    .await?;

    let count_rows = sqlx::query("SELECT type, COUNT(1) AS c FROM afv_asset WHERE deleted = 0 GROUP BY type")
        .fetch_all(&state.db)
        .await?;
    let mut type_counts = serde_json::Map::new();
    for row in count_rows {
        let key: Option<String> = row.get("type");
        let count: i64 = row.get("c");
        type_counts.insert(key.unwrap_or_else(|| "unknown".into()), json!(count));
    }

    ok(json!({
        "records": rows,
        "total": total,
        "page": page,
        "size": size,
        "typeCounts": type_counts,
    }))
}

async fn metadata(Path(asset_type): Path<String>) -> ApiResult<serde_json::Value> {
    let fields = match asset_type.as_str() {
        "character" | "role" => json!([
            {"key": "gender", "label": "性别", "type": "select", "required": false, "options": [
                {"value": "male", "label": "男"}, {"value": "female", "label": "女"}, {"value": "other", "label": "其他"}
            ]},
            {"key": "age", "label": "年龄", "type": "text", "required": false},
            {"key": "personality", "label": "性格", "type": "text", "required": false},
            {"key": "appearance", "label": "外形描述", "type": "text", "required": false}
        ]),
        "scene" => json!([
            {"key": "location", "label": "地点", "type": "text", "required": false},
            {"key": "timeOfDay", "label": "时间", "type": "select", "required": false, "options": [
                {"value": "day", "label": "日"}, {"value": "night", "label": "夜"}, {"value": "dusk", "label": "黄昏"}
            ]}
        ]),
        "prop" => json!([
            {"key": "material", "label": "材质", "type": "text", "required": false},
            {"key": "scale", "label": "尺寸", "type": "text", "required": false}
        ]),
        _ => json!([]),
    };

    ok(json!({ "assetType": asset_type, "fields": fields }))
}

async fn get_item(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<AssetItem> {
    ok(load_item(&state, id).await?)
}

async fn list_items(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(asset_id): Path<i64>,
) -> ApiResult<Vec<AssetItem>> {
    let rows = sqlx::query_as::<_, AssetItem>(
        "SELECT * FROM afv_asset_item WHERE asset_id = ? AND deleted = 0 ORDER BY sort_order ASC, id ASC",
    )
    .bind(asset_id)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn save_item(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Json(req): Json<AssetItemSaveReq>,
) -> ApiResult<AssetItem> {
    if let Some(id) = req.id {
        sqlx::query(
            "UPDATE afv_asset_item SET item_type = COALESCE(?, item_type), name = COALESCE(?, name),
                image_url = COALESCE(?, image_url), thumbnail_url = COALESCE(?, thumbnail_url),
                properties = COALESCE(?, properties), sort_order = COALESCE(?, sort_order),
                source_type = COALESCE(?, source_type), ai_prompt = COALESCE(?, ai_prompt), update_time = ?
             WHERE id = ? AND deleted = 0",
        )
        .bind(req.item_type)
        .bind(req.name)
        .bind(req.image_url)
        .bind(req.thumbnail_url)
        .bind(req.properties)
        .bind(req.sort_order)
        .bind(req.source_type)
        .bind(req.ai_prompt)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
        return ok(load_item(&state, id).await?);
    }

    let sort_order = match req.sort_order {
        Some(v) if v > 0 => v,
        _ => {
            sqlx::query_scalar::<_, i64>(
                "SELECT IFNULL(MAX(sort_order), 0) + 1 FROM afv_asset_item WHERE asset_id = ? AND deleted = 0",
            )
            .bind(req.asset_id)
            .fetch_one(&state.db)
            .await?
        }
    };

    let id = sqlx::query(
        "INSERT INTO afv_asset_item (asset_id, item_type, name, image_url, thumbnail_url, properties, sort_order, source_type, ai_prompt, deleted, create_time, update_time)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
    )
    .bind(req.asset_id)
    .bind(req.item_type)
    .bind(req.name)
    .bind(req.image_url)
    .bind(req.thumbnail_url)
    .bind(req.properties)
    .bind(sort_order)
    .bind(req.source_type.unwrap_or(0))
    .bind(req.ai_prompt)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    // 首个带图子项自动成为资产封面
    if let Some(item) = sqlx::query("SELECT image_url FROM afv_asset_item WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
    {
        let image: Option<String> = item.get("image_url");
        if let Some(image) = image {
            let _ = sqlx::query(
                "UPDATE afv_asset SET cover_url = COALESCE(NULLIF(cover_url,''), ?), update_time = ? WHERE id = ?",
            )
            .bind(image)
            .bind(now())
            .bind(req.asset_id)
            .execute(&state.db)
            .await;
        }
    }

    ok(load_item(&state, id).await?)
}

async fn delete_item(
    State(state): State<Arc<AppState>>,
    _u: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_asset_item SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    ok(true)
}
