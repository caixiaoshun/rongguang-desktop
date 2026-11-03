use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::json;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: i64,
    pub user_id: Option<i64>,
    pub project_id: Option<i64>,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub asset_type: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    #[serde(serialize_with = "json::serialize_opt")]
    pub properties: Option<String>,
    #[serde(serialize_with = "json::serialize_opt_default_array")]
    pub tags: Option<String>,
    pub source_type: i64,
    pub ai_prompt: Option<String>,
    pub owner_type: Option<i64>,
    pub owner_id: Option<i64>,
    pub status: i64,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AssetItem {
    pub id: i64,
    pub asset_id: i64,
    pub item_type: Option<String>,
    pub name: Option<String>,
    pub image_url: Option<String>,
    pub thumbnail_url: Option<String>,
    #[serde(serialize_with = "json::serialize_opt")]
    pub properties: Option<String>,
    pub sort_order: i64,
    pub source_type: i64,
    pub ai_prompt: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetWithItems {
    #[serde(flatten)]
    pub asset: Asset,
    pub items: Vec<AssetItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AssetCreateReq {
    pub project_id: i64,
    #[serde(rename = "type")]
    pub asset_type: String,
    pub name: String,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub properties: Option<String>,
    pub tags: Option<Vec<String>>,
    pub ai_prompt: Option<String>,
    pub source_type: Option<i64>,
}

impl Default for AssetCreateReq {
    fn default() -> Self {
        Self {
            project_id: 0,
            asset_type: String::new(),
            name: String::new(),
            description: None,
            cover_url: None,
            properties: None,
            tags: None,
            ai_prompt: None,
            source_type: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AssetUpdateReq {
    pub id: i64,
    #[serde(rename = "type")]
    pub asset_type: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub properties: Option<String>,
    pub tags: Option<Vec<String>>,
    pub status: Option<i64>,
    pub ai_prompt: Option<String>,
}

impl Default for AssetUpdateReq {
    fn default() -> Self {
        Self {
            id: 0,
            asset_type: None,
            name: None,
            description: None,
            cover_url: None,
            properties: None,
            tags: None,
            status: None,
            ai_prompt: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AssetItemSaveReq {
    pub id: Option<i64>,
    pub asset_id: i64,
    pub item_type: Option<String>,
    pub name: Option<String>,
    pub image_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub properties: Option<String>,
    pub sort_order: Option<i64>,
    pub source_type: Option<i64>,
    pub ai_prompt: Option<String>,
}

impl Default for AssetItemSaveReq {
    fn default() -> Self {
        Self {
            id: None,
            asset_id: 0,
            item_type: None,
            name: None,
            image_url: None,
            thumbnail_url: None,
            properties: None,
            sort_order: None,
            source_type: None,
            ai_prompt: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetMetadata {
    pub asset_type: String,
    pub fields: Vec<MetadataField>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataField {
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub options: Vec<String>,
}
