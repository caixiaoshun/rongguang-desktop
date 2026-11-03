use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::json;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StorageConfig {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub storage_type: Option<String>,
    pub provider: Option<String>,
    pub endpoint: Option<String>,
    pub bucket_name: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub region: Option<String>,
    pub base_path: Option<String>,
    pub custom_domain: Option<String>,
    #[serde(serialize_with = "json::serialize_opt")]
    pub options: Option<String>,
    pub is_default: bool,
    pub status: i64,
    pub remark: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StorageConfigSaveReq {
    pub id: Option<i64>,
    pub name: String,
    #[serde(rename = "type")]
    pub storage_type: Option<String>,
    pub provider: Option<String>,
    pub endpoint: Option<String>,
    pub bucket_name: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub region: Option<String>,
    pub base_path: Option<String>,
    pub custom_domain: Option<String>,
    pub options: Option<serde_json::Value>,
    pub is_default: Option<bool>,
    pub status: Option<i64>,
    pub remark: Option<String>,
}

impl Default for StorageConfigSaveReq {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            storage_type: None,
            provider: None,
            endpoint: None,
            bucket_name: None,
            access_key: None,
            secret_key: None,
            region: None,
            base_path: None,
            custom_domain: None,
            options: None,
            is_default: None,
            status: None,
            remark: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageTestResult {
    pub success: bool,
    pub message: String,
    pub public_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    pub url: String,
    pub file_name: String,
    pub file_size: i64,
    pub content_type: Option<String>,
}
