use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ImageTask {
    pub id: i64,
    pub task_id: String,
    pub user_id: Option<i64>,
    pub project_id: Option<i64>,
    pub prompt: Option<String>,
    pub prompt_template_id: Option<i64>,
    pub ref_image_urls: Option<String>,
    pub ratio: Option<String>,
    pub resolution: Option<String>,
    pub aspect_ratio: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub count: i64,
    pub success_count: i64,
    pub status: i64,
    pub error_msg: Option<String>,
    pub model_id: Option<i64>,
    pub category: Option<String>,
    pub owner_type: Option<i64>,
    pub owner_id: Option<i64>,
    pub create_time: String,
    pub update_time: String,
    #[sqlx(default)]
    pub workflow_version_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ImageItem {
    pub id: i64,
    pub task_id: i64,
    pub platform_task_id: Option<String>,
    pub image_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub file_size: Option<i64>,
    pub status: i64,
    pub error_msg: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct VideoTask {
    pub id: i64,
    pub task_id: String,
    pub user_id: Option<i64>,
    pub project_id: Option<i64>,
    pub prompt: Option<String>,
    pub prompt_template_id: Option<i64>,
    pub generate_mode: Option<String>,
    pub first_frame_image_url: Option<String>,
    pub last_frame_image_url: Option<String>,
    pub reference_image_urls: Option<String>,
    pub reference_video_urls: Option<String>,
    pub reference_audio_urls: Option<String>,
    pub ratio: Option<String>,
    pub resolution: Option<String>,
    pub duration: Option<i64>,
    pub watermark: Option<bool>,
    pub generate_audio: Option<bool>,
    pub seed: Option<i64>,
    pub camera_fixed: Option<bool>,
    pub count: i64,
    pub success_count: i64,
    pub status: i64,
    pub error_msg: Option<String>,
    pub model_id: Option<i64>,
    pub category: Option<String>,
    pub owner_type: Option<i64>,
    pub owner_id: Option<i64>,
    pub create_time: String,
    pub update_time: String,
    #[sqlx(default)]
    pub workflow_version_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct VideoItem {
    pub id: i64,
    pub task_id: i64,
    pub platform_task_id: Option<String>,
    pub video_url: Option<String>,
    pub cover_url: Option<String>,
    pub duration: Option<i64>,
    pub file_size: Option<i64>,
    pub status: i64,
    pub error_msg: Option<String>,
    pub first_frame_url: Option<String>,
    pub last_frame_url: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageSubmitReq {
    pub project_id: Option<i64>,
    pub prompt: String,
    pub ref_image_urls: Option<String>,
    pub ratio: Option<String>,
    pub resolution: Option<String>,
    pub aspect_ratio: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub count: Option<i64>,
    pub model_id: i64,
    pub category: Option<String>,
}

impl Default for ImageSubmitReq {
    fn default() -> Self {
        Self {
            project_id: None,
            prompt: String::new(),
            ref_image_urls: None,
            ratio: None,
            resolution: None,
            aspect_ratio: None,
            width: None,
            height: None,
            count: None,
            model_id: 0,
            category: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VideoSubmitReq {
    pub project_id: Option<i64>,
    pub prompt: String,
    pub generate_mode: Option<String>,
    pub first_frame_image_url: Option<String>,
    pub last_frame_image_url: Option<String>,
    pub reference_image_urls: Option<String>,
    pub reference_video_urls: Option<String>,
    pub reference_audio_urls: Option<String>,
    pub ratio: Option<String>,
    pub resolution: Option<String>,
    pub duration: Option<i64>,
    pub watermark: Option<bool>,
    pub generate_audio: Option<bool>,
    pub seed: Option<i64>,
    pub camera_fixed: Option<bool>,
    pub count: Option<i64>,
    pub model_id: i64,
    pub category: Option<String>,
}

impl Default for VideoSubmitReq {
    fn default() -> Self {
        Self {
            project_id: None,
            prompt: String::new(),
            generate_mode: None,
            first_frame_image_url: None,
            last_frame_image_url: None,
            reference_image_urls: None,
            reference_video_urls: None,
            reference_audio_urls: None,
            ratio: None,
            resolution: None,
            duration: None,
            watermark: None,
            generate_audio: None,
            seed: None,
            camera_fixed: None,
            count: None,
            model_id: 0,
            category: None,
        }
    }
}

/// 任务状态：0 排队 1 生成中 2 成功 3 失败 4 已取消
pub mod task_status {
    pub const PENDING: i64 = 0;
    pub const RUNNING: i64 = 1;
    pub const SUCCESS: i64 = 2;
    pub const FAILED: i64 = 3;
    pub const CANCELED: i64 = 4;
}
