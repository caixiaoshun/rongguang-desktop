//! 全局状态：数据库、路径、配置、供应商注册表与任务登记。

use std::collections::HashSet;
use std::sync::Arc;

use fusion_ai::provider::grok::GrokProvider;
use fusion_ai::provider::openai_compat::OpenAiCompatProvider;
use fusion_ai::provider::comfyui::ComfyUiProvider;
use fusion_ai::provider::{ImageProvider, Provider, ProviderRegistry, SpeechProvider, VideoProvider};
use fusion_ai::media::MediaStore;
use fusion_core::config::AppConfig;
use fusion_core::db::{self, Db};
use fusion_core::paths::AppPaths;
use parking_lot::Mutex;

/// 桌面版内置账号 ID。
pub const LOCAL_USER_ID: i64 = 1;

pub struct AppState {
    pub db: Db,
    pub paths: AppPaths,
    pub config: parking_lot::RwLock<AppConfig>,
    pub media: MediaStore,
    pub registry: ProviderRegistry,
    pub http: reqwest::Client,
    /// 正在取消的生成任务 ID
    pub canceled_tasks: Mutex<HashSet<String>>,
}

impl AppState {
    pub async fn new(paths: AppPaths, config: AppConfig) -> anyhow::Result<Self> {
        let db = db::connect(&paths).await?;
        let media = MediaStore::new(paths.media_dir.clone());

        let mut registry = ProviderRegistry::new();
        let grok = Arc::new(GrokProvider::new());
        registry.register_chat(grok.clone() as Arc<dyn Provider>);
        registry.register_image(grok.clone() as Arc<dyn ImageProvider>);
        registry.register_video(grok.clone() as Arc<dyn VideoProvider>);
        registry.register_speech(grok as Arc<dyn SpeechProvider>);

        let compat = Arc::new(OpenAiCompatProvider::new());
        registry.register_chat(compat.clone() as Arc<dyn Provider>);
        registry.register_image(compat.clone() as Arc<dyn ImageProvider>);
        registry.register_video(compat as Arc<dyn VideoProvider>);

        let comfy = Arc::new(ComfyUiProvider::new());
        registry.register_image(comfy.clone() as Arc<dyn ImageProvider>);
        registry.register_video(comfy as Arc<dyn VideoProvider>);

        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .connect_timeout(std::time::Duration::from_secs(20))
            .user_agent("rongguang-desktop/0.1")
            .build()?;

        Ok(Self {
            db,
            paths,
            config: parking_lot::RwLock::new(config),
            media,
            registry,
            http,
            canceled_tasks: Mutex::new(HashSet::new()),
        })
    }

    /// 桌面版单机单用户：固定使用 id=1 的本地账号。
    /// 首次启动时由 `ensure_local_user` 落库，之后所有请求都以它身份执行。
    pub async fn local_user(&self) -> crate::auth::AuthUser {
        crate::auth::AuthUser {
            id: LOCAL_USER_ID,
            username: "local".into(),
        }
    }

    pub fn jwt_secret(&self) -> String {
        self.config.read().jwt_secret.clone()
    }

    pub fn is_canceled(&self, task_id: &str) -> bool {
        self.canceled_tasks.lock().contains(task_id)
    }

    pub fn mark_canceled(&self, task_id: &str) {
        self.canceled_tasks.lock().insert(task_id.to_string());
    }

    pub fn clear_canceled(&self, task_id: &str) {
        self.canceled_tasks.lock().remove(task_id);
    }
}
