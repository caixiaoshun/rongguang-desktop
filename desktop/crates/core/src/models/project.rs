use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::json;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub scope: i64,
    pub owner_type: i64,
    pub owner_id: i64,
    pub status: i64,
    #[serde(serialize_with = "json::serialize_opt")]
    pub properties: Option<String>,
    pub art_style: Option<String>,
    pub art_style_description: Option<String>,
    pub art_style_image_prompt: Option<String>,
    pub art_style_image_url: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectCreateReq {
    pub name: String,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub properties: Option<String>,
    pub art_style: Option<String>,
    pub art_style_description: Option<String>,
    pub art_style_image_prompt: Option<String>,
    pub art_style_image_url: Option<String>,
}

impl Default for ProjectCreateReq {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: None,
            cover_url: None,
            properties: None,
            art_style: None,
            art_style_description: None,
            art_style_image_prompt: None,
            art_style_image_url: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectUpdateReq {
    pub id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub status: Option<i64>,
    pub properties: Option<String>,
    pub art_style: Option<String>,
    pub art_style_description: Option<String>,
    pub art_style_image_prompt: Option<String>,
    pub art_style_image_url: Option<String>,
}

impl Default for ProjectUpdateReq {
    fn default() -> Self {
        Self {
            id: 0,
            name: None,
            description: None,
            cover_url: None,
            status: None,
            properties: None,
            art_style: None,
            art_style_description: None,
            art_style_image_prompt: None,
            art_style_image_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMember {
    pub id: i64,
    pub project_id: i64,
    pub user_id: i64,
    pub role: i64,
    pub create_time: String,
    #[sqlx(default)]
    pub username: Option<String>,
    #[sqlx(default)]
    pub nickname: Option<String>,
    #[sqlx(default)]
    pub avatar: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMemberAddReq {
    pub project_id: i64,
    pub user_id: i64,
    #[serde(default)]
    pub role: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtStylePreset {
    pub key: String,
    pub name: String,
    pub description: String,
    pub image_prompt: String,
    pub image_url: String,
}
