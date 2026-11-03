use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::json;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApiConfig {
    pub id: i64,
    pub name: Option<String>,
    pub platform: Option<String>,
    pub text_protocol: Option<String>,
    pub image_protocol: Option<String>,
    pub video_protocol: Option<String>,
    pub api_type: Option<i64>,
    pub api_url: Option<String>,
    pub proxy_type: Option<String>,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<i64>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub api_key: Option<String>,
    pub app_id: Option<String>,
    pub app_secret: Option<String>,
    pub model_id: Option<i64>,
    pub status: i64,
    pub remark: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

impl ApiConfig {
    /// 前端表单需要回填密钥，但列表只展示脱敏值；此方法给出掩码表示。
    pub fn masked_key(&self) -> Option<String> {
        self.api_key.as_ref().map(|k| mask_secret(k))
    }
}

pub fn mask_secret(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        return "••••••••".into();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{head}••••{tail}")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ApiConfigSaveReq {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub platform: Option<String>,
    pub text_protocol: Option<String>,
    pub image_protocol: Option<String>,
    pub video_protocol: Option<String>,
    pub api_type: Option<i64>,
    pub api_url: Option<String>,
    pub proxy_type: Option<String>,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<i64>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub api_key: Option<String>,
    pub app_id: Option<String>,
    pub app_secret: Option<String>,
    pub model_id: Option<i64>,
    pub status: Option<i64>,
    pub remark: Option<String>,
    pub auto_append_v1_path: Option<bool>,
}

impl Default for ApiConfigSaveReq {
    fn default() -> Self {
        Self {
            id: None,
            name: None,
            platform: None,
            text_protocol: None,
            image_protocol: None,
            video_protocol: None,
            api_type: None,
            api_url: None,
            proxy_type: None,
            proxy_host: None,
            proxy_port: None,
            proxy_username: None,
            proxy_password: None,
            api_key: None,
            app_id: None,
            app_secret: None,
            model_id: None,
            status: None,
            remark: None,
            auto_append_v1_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub id: i64,
    pub name: Option<String>,
    pub code: String,
    pub model_protocol: Option<String>,
    pub capability_preset_code: Option<String>,
    pub model_type: i64,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub sort: i64,
    pub status: i64,
    pub config: Option<String>,
    pub default_model: bool,
    pub max_concurrency: Option<i64>,
    pub api_config_id: Option<i64>,
    pub support_vision: bool,
    #[serde(serialize_with = "json::serialize_opt_default_array")]
    pub multimodal_input_types: Option<String>,
    #[serde(serialize_with = "json::serialize_opt_default_array")]
    pub multimodal_input_transports: Option<String>,
    pub support_reasoning: bool,
    #[serde(serialize_with = "json::serialize_opt_default_array")]
    pub reasoning_effort_levels: Option<String>,
    pub context_window: Option<i64>,
    pub create_time: String,
    pub update_time: String,
    /// 运行时派生字段：所属 API 配置的平台，便于前端展示。
    #[sqlx(default)]
    pub platform: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiModelSaveReq {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub code: Option<String>,
    pub model_protocol: Option<String>,
    pub capability_preset_code: Option<String>,
    pub model_type: Option<i64>,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub sort: Option<i64>,
    pub status: Option<i64>,
    pub config: Option<String>,
    pub max_concurrency: Option<i64>,
    pub default_model: Option<bool>,
    pub support_vision: Option<bool>,
    pub multimodal_input_types: Option<serde_json::Value>,
    pub multimodal_input_transports: Option<serde_json::Value>,
    pub support_reasoning: Option<bool>,
    pub reasoning_effort_levels: Option<serde_json::Value>,
    pub context_window: Option<i64>,
    pub api_config_id: Option<i64>,
    pub comfyui_workflow_id: Option<i64>,
}

impl Default for AiModelSaveReq {
    fn default() -> Self {
        Self {
            id: None,
            name: None,
            code: None,
            model_protocol: None,
            capability_preset_code: None,
            model_type: None,
            icon: None,
            description: None,
            sort: None,
            status: None,
            config: None,
            max_concurrency: None,
            default_model: None,
            support_vision: None,
            multimodal_input_types: None,
            multimodal_input_transports: None,
            support_reasoning: None,
            reasoning_effort_levels: None,
            context_window: None,
            api_config_id: None,
            comfyui_workflow_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModelVo {
    pub id: String,
    pub name: String,
    pub owned_by: Option<String>,
    pub model_type: Option<i64>,
    pub inferred: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConnectivityResult {
    pub model_id: i64,
    pub model_name: String,
    pub response_text: String,
    pub duration_ms: i64,
    pub tested_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPreset {
    pub code: String,
    pub name: String,
    pub model_type: i64,
    pub description: String,
    pub support_vision: bool,
    pub support_reasoning: bool,
    pub context_window: Option<i64>,
    pub config: Option<String>,
}
