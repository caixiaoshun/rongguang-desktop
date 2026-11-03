//! ComfyUI 适配：提交工作流、轮询历史、取回产物。
//!
//! 兼容标准 ComfyUI HTTP API：`POST /prompt` → `GET /history/{prompt_id}` → `GET /view`。

use serde_json::{json, Value};
use std::time::Duration;

use fusion_core::error::{AppError, AppResult};

use super::client::describe_error;
use super::{
    ImageProvider, ImageRequest, ImageResult, ProviderConfig, VideoPoll, VideoProvider, VideoSubmit,
};

pub struct ComfyUiProvider;

impl ComfyUiProvider {
    pub fn new() -> Self {
        Self
    }

    fn client(_cfg: &ProviderConfig) -> AppResult<reqwest::Client> {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| AppError::Http(e.to_string()))
    }

    fn base(cfg: &ProviderConfig) -> String {
        cfg.base_url.trim().trim_end_matches('/').to_string()
    }

    /// 工作流 JSON 里通过占位符替换注入提示词：`%prompt%`、`%width%`、`%height%`、`%seed%`。
    pub fn inject(workflow: &Value, prompt: &str, width: i64, height: i64, seed: i64) -> Value {
        let text = serde_json::to_string(workflow).unwrap_or_default();
        let replaced = text
            .replace("%prompt%", &escape(prompt))
            .replace("%width%", &width.to_string())
            .replace("%height%", &height.to_string())
            .replace("%seed%", &seed.to_string());
        serde_json::from_str(&replaced).unwrap_or_else(|_| workflow.clone())
    }

    async fn submit_workflow(&self, cfg: &ProviderConfig, workflow: Value) -> AppResult<String> {
        let client = Self::client(cfg)?;
        let body = json!({ "prompt": workflow, "client_id": "rongguang-desktop" });
        let resp = client
            .post(format!("{}/prompt", Self::base(cfg)))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Http(format!("提交 ComfyUI 任务失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(AppError::Http(describe_error(status, &text)));
        }
        let value: Value = serde_json::from_str(&text)?;
        value
            .get("prompt_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::Http("ComfyUI 未返回 prompt_id，请检查工作流格式".into()))
    }
}

fn escape(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

impl Default for ComfyUiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ImageProvider for ComfyUiProvider {
    fn platform(&self) -> &'static str {
        "comfyui"
    }

    async fn generate(&self, cfg: &ProviderConfig, req: ImageRequest) -> AppResult<ImageResult> {
        let workflow = req
            .extra
            .get("workflow")
            .cloned()
            .ok_or_else(|| AppError::bad_request("ComfyUI 模型未绑定工作流版本"))?;
        let width = req
            .extra
            .get("width")
            .and_then(|v| v.as_i64())
            .unwrap_or(1024);
        let height = req
            .extra
            .get("height")
            .and_then(|v| v.as_i64())
            .unwrap_or(1024);
        let seed = req
            .extra
            .get("seed")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| rand_seed());

        let injected = Self::inject(&workflow, &req.prompt, width, height, seed);
        let prompt_id = self.submit_workflow(cfg, injected).await?;

        let client = Self::client(cfg)?;
        let base = Self::base(cfg);
        for _ in 0..150 {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let resp = client
                .get(format!("{base}/history/{prompt_id}"))
                .send()
                .await
                .map_err(|e| AppError::Http(format!("查询 ComfyUI 任务失败: {e}")))?;
            let value: Value = resp.json().await.unwrap_or(Value::Null);
            let Some(entry) = value.get(&prompt_id) else {
                continue;
            };
            let status = entry
                .get("status")
                .and_then(|s| s.get("status_str"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if status == "error" {
                return Err(AppError::Http("ComfyUI 工作流执行失败".into()));
            }
            let outputs = entry.get("outputs").and_then(|o| o.as_object()).cloned();
            if let Some(outputs) = outputs {
                let mut urls = Vec::new();
                for (_, node) in outputs {
                    if let Some(images) = node.get("images").and_then(|i| i.as_array()) {
                        for img in images {
                            let filename =
                                img.get("filename").and_then(|v| v.as_str()).unwrap_or("");
                            let subfolder =
                                img.get("subfolder").and_then(|v| v.as_str()).unwrap_or("");
                            let folder_type =
                                img.get("type").and_then(|v| v.as_str()).unwrap_or("output");
                            if filename.is_empty() {
                                continue;
                            }
                            urls.push(format!(
                                "{base}/view?filename={}&subfolder={}&type={}",
                                urlencoding::encode(filename),
                                urlencoding::encode(subfolder),
                                urlencoding::encode(folder_type)
                            ));
                        }
                    }
                }
                if !urls.is_empty() {
                    return Ok(ImageResult { urls });
                }
            }
        }
        Err(AppError::Http("ComfyUI 任务超时".into()))
    }
}

#[async_trait::async_trait]
impl VideoProvider for ComfyUiProvider {
    fn platform(&self) -> &'static str {
        "comfyui"
    }

    async fn submit(&self, _cfg: &ProviderConfig, _req: VideoSubmit) -> AppResult<String> {
        Err(AppError::bad_request(
            "ComfyUI 视频生成需在模型配置中绑定视频工作流后使用",
        ))
    }

    async fn poll(&self, _cfg: &ProviderConfig, _id: &str) -> AppResult<VideoPoll> {
        Err(AppError::bad_request("ComfyUI 视频轮询未启用"))
    }
}

fn rand_seed() -> i64 {
    use rand::Rng;
    rand::thread_rng().gen_range(1..2_000_000_000)
}
