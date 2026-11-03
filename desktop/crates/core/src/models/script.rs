use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::json;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub id: i64,
    pub project_id: i64,
    pub title: Option<String>,
    pub content: Option<String>,
    pub raw_content: Option<String>,
    pub total_episodes: i64,
    pub story_synopsis: Option<String>,
    #[serde(serialize_with = "json::serialize_opt_default_array")]
    pub characters_json: Option<String>,
    pub source_type: i64,
    pub parsing_status: i64,
    pub parsing_progress: Option<String>,
    pub summary: Option<String>,
    pub genre: Option<String>,
    pub target_audience: Option<String>,
    pub duration_estimate: Option<i64>,
    pub scope: i64,
    pub owner_type: Option<i64>,
    pub owner_id: Option<i64>,
    pub ai_generated: bool,
    pub version: i64,
    pub status: i64,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ScriptEpisode {
    pub id: i64,
    pub script_id: i64,
    pub episode_number: Option<i64>,
    pub title: Option<String>,
    pub synopsis: Option<String>,
    pub raw_content: Option<String>,
    pub duration_estimate: Option<i64>,
    pub total_scenes: i64,
    pub source_type: i64,
    pub sort_order: i64,
    pub parsing_status: i64,
    pub status: i64,
    pub version: i64,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ScriptSceneItem {
    pub id: i64,
    pub episode_id: i64,
    pub script_id: i64,
    pub scene_number: Option<String>,
    pub scene_heading: Option<String>,
    pub location: Option<String>,
    pub time_of_day: Option<String>,
    pub int_ext: Option<String>,
    #[serde(serialize_with = "json::serialize_opt_default_array")]
    pub characters: Option<String>,
    #[serde(serialize_with = "json::serialize_opt_default_array")]
    pub character_asset_ids: Option<String>,
    pub scene_asset_id: Option<i64>,
    #[serde(serialize_with = "json::serialize_opt_default_array")]
    pub prop_asset_ids: Option<String>,
    pub scene_description: Option<String>,
    #[serde(serialize_with = "json::serialize_opt_default_array")]
    pub dialogues: Option<String>,
    pub sort_order: i64,
    pub status: i64,
    pub version: i64,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptUpdateReq {
    pub id: i64,
    pub title: Option<String>,
    pub content: Option<String>,
    pub raw_content: Option<String>,
    pub story_synopsis: Option<String>,
    pub summary: Option<String>,
    pub genre: Option<String>,
    pub target_audience: Option<String>,
    pub total_episodes: Option<i64>,
    pub duration_estimate: Option<i64>,
    pub status: Option<i64>,
    pub characters_json: Option<serde_json::Value>,
}

impl Default for ScriptUpdateReq {
    fn default() -> Self {
        Self {
            id: 0,
            title: None,
            content: None,
            raw_content: None,
            story_synopsis: None,
            summary: None,
            genre: None,
            target_audience: None,
            total_episodes: None,
            duration_estimate: None,
            status: None,
            characters_json: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptSourceUpdateReq {
    pub raw_content: Option<String>,
    pub content: Option<String>,
    pub title: Option<String>,
    pub source_type: Option<i64>,
}

impl Default for ScriptSourceUpdateReq {
    fn default() -> Self {
        Self {
            raw_content: None,
            content: None,
            title: None,
            source_type: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptEpisodeSaveReq {
    pub id: Option<i64>,
    pub script_id: i64,
    pub episode_number: Option<i64>,
    pub title: Option<String>,
    pub synopsis: Option<String>,
    pub raw_content: Option<String>,
    pub duration_estimate: Option<i64>,
    pub sort_order: Option<i64>,
    pub status: Option<i64>,
}

impl Default for ScriptEpisodeSaveReq {
    fn default() -> Self {
        Self {
            id: None,
            script_id: 0,
            episode_number: None,
            title: None,
            synopsis: None,
            raw_content: None,
            duration_estimate: None,
            sort_order: None,
            status: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptSceneSaveReq {
    pub id: Option<i64>,
    pub episode_id: i64,
    pub script_id: Option<i64>,
    pub scene_number: Option<String>,
    pub scene_heading: Option<String>,
    pub location: Option<String>,
    pub time_of_day: Option<String>,
    pub int_ext: Option<String>,
    pub characters: Option<serde_json::Value>,
    pub character_asset_ids: Option<serde_json::Value>,
    pub scene_asset_id: Option<i64>,
    pub prop_asset_ids: Option<serde_json::Value>,
    pub scene_description: Option<String>,
    pub dialogues: Option<serde_json::Value>,
    pub sort_order: Option<i64>,
    pub status: Option<i64>,
}
