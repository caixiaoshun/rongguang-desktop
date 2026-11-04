//! 供应商注册表与统一请求/响应类型。

pub mod client;
pub mod comfyui;
pub mod grok;
pub mod openai_compat;

use std::collections::HashMap;
use std::sync::Arc;

use futures_util::stream::BoxStream;

use fusion_core::error::{AppError, AppResult};
use fusion_core::models::ai::{ApiConfig, RemoteModelVo};

use crate::types::ToolDef;
use crate::types::{ChatRequest, ChatResponse, StreamDelta};

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub platform: String,
    pub base_url: String,
    pub api_key: String,
    pub proxy_type: Option<String>,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<i64>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
}

impl ProviderConfig {
    pub fn from_api_config(config: &ApiConfig, platform_override: Option<&str>) -> Self {
        Self {
            platform: platform_override
                .map(|s| s.to_string())
                .or_else(|| config.platform.clone())
                .unwrap_or_else(|| "grok".into()),
            base_url: config.api_url.clone().unwrap_or_default(),
            api_key: config.api_key.clone().unwrap_or_default(),
            proxy_type: config.proxy_type.clone(),
            proxy_host: config.proxy_host.clone(),
            proxy_port: config.proxy_port,
            proxy_username: config.proxy_username.clone(),
            proxy_password: config.proxy_password.clone(),
        }
    }

    /// 归一化根地址：去掉尾部斜杠与已被误填的 `/v1`、`/chat/completions` 后缀。
    pub fn root(&self) -> String {
        let mut url = self.base_url.trim().trim_end_matches('/').to_string();
        for suffix in [
            "/chat/completions",
            "/images/generations",
            "/images/edits",
            "/videos/generations",
        ] {
            if let Some(stripped) = url.strip_suffix(suffix) {
                url = stripped.to_string();
            }
        }
        if let Some(stripped) = url.strip_suffix("/v1") {
            url = stripped.to_string();
        }
        url
    }

    pub fn v1(&self) -> String {
        format!("{}/v1", self.root())
    }

    /// Generated media may point at a third-party CDN. Only the configured
    /// provider origin can receive its credential.
    pub fn media_auth(&self, url: &str) -> Option<&str> {
        let provider = reqwest::Url::parse(&self.base_url).ok()?;
        let media = reqwest::Url::parse(url).ok()?;
        if matches!(provider.scheme(), "https" | "http")
            && provider.origin() == media.origin()
            && !self.api_key.is_empty()
        {
            Some(self.api_key.as_str())
        } else {
            None
        }
    }

    pub fn require_key(&self) -> AppResult<()> {
        if self.api_key.trim().is_empty() {
            return Err(AppError::bad_request(
                "API 密钥未配置，请先在「设置 → AI 模型」里填写",
            ));
        }
        Ok(())
    }

    /// 校验服务地址。
    ///
    /// 安装包不预置任何服务地址（那是用户自己的中转站），所以首次使用时地址字段是空的。
    /// 没有这个校验的话，空地址会拼成 `/v1/chat/completions` 这种相对路径，
    /// 报错是 reqwest 的「builder error」，用户完全看不懂。
    pub fn require_base_url(&self) -> AppResult<()> {
        let root = self.root();
        if root.is_empty() {
            return Err(AppError::bad_request(
                "API 地址未配置，请先在「设置 → AI 模型」里填写服务地址（例如 https://你的域名）",
            ));
        }
        if !root.starts_with("http://") && !root.starts_with("https://") {
            return Err(AppError::bad_request(
                "API 地址必须以 http:// 或 https:// 开头，请检查「设置 → AI 模型」里的服务地址",
            ));
        }
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// 平台标识（`grok` / `openai_compatible` / `comfyui` …）。
    fn platform(&self) -> &'static str;

    async fn chat(&self, cfg: &ProviderConfig, req: ChatRequest) -> AppResult<ChatResponse>;
    fn chat_stream(
        &self,
        cfg: ProviderConfig,
        req: ChatRequest,
    ) -> BoxStream<'static, AppResult<StreamDelta>>;
    async fn list_models(&self, cfg: &ProviderConfig) -> AppResult<Vec<RemoteModelVo>>;
}

/// 图像生成请求。
#[derive(Debug, Clone, Default)]
pub struct ImageRequest {
    pub model: String,
    pub prompt: String,
    pub reference_images: Vec<String>,
    pub size: Option<String>,
    pub count: i64,
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct ImageResult {
    pub urls: Vec<String>,
}

#[async_trait::async_trait]
pub trait ImageProvider: Send + Sync {
    fn platform(&self) -> &'static str;
    async fn generate(&self, cfg: &ProviderConfig, req: ImageRequest) -> AppResult<ImageResult>;
}

/// 视频提交请求。
#[derive(Debug, Clone, Default)]
pub struct VideoSubmit {
    pub model: String,
    pub prompt: String,
    pub image_url: Option<String>,
    pub duration: Option<i64>,
    pub aspect_ratio: Option<String>,
    pub resolution: Option<String>,
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum VideoPoll {
    Pending {
        progress: Option<i64>,
    },
    Done {
        url: String,
        duration: Option<i64>,
        cover: Option<String>,
    },
    Failed {
        message: String,
    },
}

#[async_trait::async_trait]
pub trait VideoProvider: Send + Sync {
    fn platform(&self) -> &'static str;
    /// 提交任务，返回平台侧任务 ID。
    async fn submit(&self, cfg: &ProviderConfig, req: VideoSubmit) -> AppResult<String>;
    async fn poll(&self, cfg: &ProviderConfig, platform_task_id: &str) -> AppResult<VideoPoll>;
    /// 视频内容下载地址（部分平台需要带鉴权或专门端点）。
    fn content_url(&self, cfg: &ProviderConfig, platform_task_id: &str, direct: &str) -> String {
        let _ = (cfg, platform_task_id);
        direct.to_string()
    }
}

/// 语音合成与识别。
#[async_trait::async_trait]
pub trait SpeechProvider: Send + Sync {
    fn platform(&self) -> &'static str;
    async fn synthesize(
        &self,
        cfg: &ProviderConfig,
        model: &str,
        input: &str,
        voice: Option<&str>,
    ) -> AppResult<Vec<u8>>;
    async fn transcribe(
        &self,
        cfg: &ProviderConfig,
        model: &str,
        file_name: &str,
        bytes: Vec<u8>,
        mime: &str,
    ) -> AppResult<String>;
}

#[derive(Default)]
pub struct ProviderRegistry {
    chat: HashMap<String, Arc<dyn Provider>>,
    image: HashMap<String, Arc<dyn ImageProvider>>,
    video: HashMap<String, Arc<dyn VideoProvider>>,
    speech: HashMap<String, Arc<dyn SpeechProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_chat(&mut self, provider: Arc<dyn Provider>) {
        self.chat.insert(provider.platform().to_string(), provider);
    }
    pub fn register_image(&mut self, provider: Arc<dyn ImageProvider>) {
        self.image.insert(provider.platform().to_string(), provider);
    }
    pub fn register_video(&mut self, provider: Arc<dyn VideoProvider>) {
        self.video.insert(provider.platform().to_string(), provider);
    }
    pub fn register_speech(&mut self, provider: Arc<dyn SpeechProvider>) {
        self.speech
            .insert(provider.platform().to_string(), provider);
    }

    pub fn chat(&self, platform: &str) -> AppResult<Arc<dyn Provider>> {
        self.chat
            .get(platform)
            .cloned()
            .or_else(|| self.chat.get("openai_compatible").cloned())
            .ok_or_else(|| AppError::bad_request(format!("不支持的对话平台: {platform}")))
    }

    pub fn image(&self, platform: &str) -> AppResult<Arc<dyn ImageProvider>> {
        self.image
            .get(platform)
            .cloned()
            .or_else(|| self.image.get("openai_compatible").cloned())
            .ok_or_else(|| AppError::bad_request(format!("不支持的图像平台: {platform}")))
    }

    pub fn video(&self, platform: &str) -> AppResult<Arc<dyn VideoProvider>> {
        self.video
            .get(platform)
            .cloned()
            .or_else(|| self.video.get("grok").cloned())
            .ok_or_else(|| AppError::bad_request(format!("不支持的视频平台: {platform}")))
    }

    pub fn speech(&self, platform: &str) -> AppResult<Arc<dyn SpeechProvider>> {
        self.speech
            .get(platform)
            .cloned()
            .or_else(|| self.speech.get("grok").cloned())
            .ok_or_else(|| AppError::bad_request(format!("不支持的语音平台: {platform}")))
    }

    pub fn platforms(&self) -> Vec<String> {
        self.chat.keys().cloned().collect()
    }
}

/// 工具定义构造辅助，避免各适配器重复拼 JSON Schema。
pub fn tool_schema(properties: serde_json::Value, required: Vec<&str>) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

pub fn empty_tools() -> Vec<ToolDef> {
    Vec::new()
}
