use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Storyboard {
    pub id: i64,
    pub project_id: i64,
    pub script_id: Option<i64>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub custom_columns: Option<String>,
    pub scope: i64,
    pub owner_type: Option<i64>,
    pub owner_id: Option<i64>,
    pub total_duration: Option<i64>,
    pub status: i64,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardEpisode {
    pub id: i64,
    pub storyboard_id: i64,
    pub script_episode_id: Option<i64>,
    pub episode_number: Option<i64>,
    pub title: Option<String>,
    pub synopsis: Option<String>,
    pub sort_order: i64,
    pub status: i64,
    pub composed_video_url: Option<String>,
    pub compose_status: i64,
    pub compose_error_msg: Option<String>,
    pub composed_at: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardScene {
    pub id: i64,
    pub episode_id: i64,
    pub storyboard_id: i64,
    pub scene_number: Option<String>,
    pub scene_heading: Option<String>,
    pub location: Option<String>,
    pub time_of_day: Option<String>,
    pub int_ext: Option<String>,
    pub sort_order: i64,
    pub status: i64,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardItem {
    pub id: i64,
    pub storyboard_id: i64,
    pub storyboard_episode_id: Option<i64>,
    pub storyboard_scene_id: Option<i64>,
    pub sort_order: i64,
    pub shot_number: Option<String>,
    pub auto_shot_number: Option<String>,
    pub image_url: Option<String>,
    pub reference_image_url: Option<String>,
    pub video_url: Option<String>,
    pub generated_image_url: Option<String>,
    pub first_frame_image_url: Option<String>,
    pub last_frame_image_url: Option<String>,
    pub first_frame_prompt: Option<String>,
    pub last_frame_prompt: Option<String>,
    pub generated_video_url: Option<String>,
    pub video_prompt: Option<String>,
    pub shot_type: Option<String>,
    pub duration: Option<f64>,
    pub content: Option<String>,
    pub scene_expectation: Option<String>,
    pub sound: Option<String>,
    pub dialogue: Option<String>,
    pub sound_effect: Option<String>,
    pub music: Option<String>,
    pub camera_movement: Option<String>,
    pub camera_angle: Option<String>,
    pub camera_equipment: Option<String>,
    pub focal_length: Option<String>,
    pub transition: Option<String>,
    pub character_ids: Option<String>,
    pub scene_asset_item_id: Option<i64>,
    pub prop_ids: Option<String>,
    pub remark: Option<String>,
    pub custom_data: Option<String>,
    pub ai_generated: bool,
    pub status: i64,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardStatistics {
    pub episode_count: i64,
    pub scene_count: i64,
    pub item_count: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoryboardUpdateReq {
    pub id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub script_id: Option<i64>,
    pub status: Option<i64>,
    pub custom_columns: Option<String>,
}

impl Default for StoryboardUpdateReq {
    fn default() -> Self {
        Self {
            id: 0,
            title: None,
            description: None,
            script_id: None,
            status: None,
            custom_columns: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoryboardEpisodeSaveReq {
    pub id: Option<i64>,
    pub storyboard_id: i64,
    pub script_episode_id: Option<i64>,
    pub episode_number: Option<i64>,
    pub title: Option<String>,
    pub synopsis: Option<String>,
    pub sort_order: Option<i64>,
    pub status: Option<i64>,
}

impl Default for StoryboardEpisodeSaveReq {
    fn default() -> Self {
        Self {
            id: None,
            storyboard_id: 0,
            script_episode_id: None,
            episode_number: None,
            title: None,
            synopsis: None,
            sort_order: None,
            status: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoryboardSceneSaveReq {
    pub id: Option<i64>,
    pub episode_id: i64,
    pub storyboard_id: i64,
    pub scene_number: Option<String>,
    pub scene_heading: Option<String>,
    pub location: Option<String>,
    pub time_of_day: Option<String>,
    pub int_ext: Option<String>,
    pub sort_order: Option<i64>,
    pub status: Option<i64>,
}

impl Default for StoryboardSceneSaveReq {
    fn default() -> Self {
        Self {
            id: None,
            episode_id: 0,
            storyboard_id: 0,
            scene_number: None,
            scene_heading: None,
            location: None,
            time_of_day: None,
            int_ext: None,
            sort_order: None,
            status: None,
        }
    }
}

/// 分镜条目保存：字段与前端 `StoryboardItemSaveReq` 对齐，全部可选，缺失即不改。
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct StoryboardItemSaveReq {
    pub id: Option<i64>,
    pub storyboard_id: i64,
    pub storyboard_episode_id: Option<i64>,
    pub storyboard_scene_id: Option<i64>,
    pub sort_order: Option<i64>,
    pub shot_number: Option<String>,
    pub auto_shot_number: Option<String>,
    pub image_url: Option<String>,
    pub reference_image_url: Option<String>,
    pub video_url: Option<String>,
    pub generated_image_url: Option<String>,
    pub first_frame_image_url: Option<String>,
    pub last_frame_image_url: Option<String>,
    pub first_frame_prompt: Option<String>,
    pub last_frame_prompt: Option<String>,
    pub generated_video_url: Option<String>,
    pub video_prompt: Option<String>,
    pub shot_type: Option<String>,
    pub duration: Option<f64>,
    pub content: Option<String>,
    pub scene_expectation: Option<String>,
    pub sound: Option<String>,
    pub dialogue: Option<String>,
    pub sound_effect: Option<String>,
    pub music: Option<String>,
    pub camera_movement: Option<String>,
    pub camera_angle: Option<String>,
    pub camera_equipment: Option<String>,
    pub focal_length: Option<String>,
    pub transition: Option<String>,
    pub character_ids: Option<String>,
    pub scene_asset_item_id: Option<i64>,
    pub prop_ids: Option<String>,
    pub remark: Option<String>,
    pub custom_data: Option<String>,
    pub ai_generated: Option<bool>,
    pub status: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSortReq {
    pub ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItemsReq {
    pub storyboard_id: i64,
    pub items: Vec<StoryboardItemSaveReq>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateFrameReq {
    pub frame_type: Option<String>,
    pub image_url: Option<String>,
    pub prompt: Option<String>,
}

impl Default for UpdateFrameReq {
    fn default() -> Self {
        Self {
            frame_type: None,
            image_url: None,
            prompt: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BindScriptEpisodeReq {
    pub script_episode_id: Option<i64>,
}

impl Default for BindScriptEpisodeReq {
    fn default() -> Self {
        Self {
            script_episode_id: None,
        }
    }
}
