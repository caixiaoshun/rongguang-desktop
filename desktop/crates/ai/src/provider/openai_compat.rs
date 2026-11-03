//! OpenAI 兼容供应商（覆盖 DeepSeek / 智谱 / Moonshot / NewAPI / 火山方舟兼容口）。
//!
//! 与 Grok 的差别：
//! - 图生图走官方 `image` 字段或 multipart；
//! - 视频走 Sora 风格 multipart + `/videos/{id}/content` 取片。

use std::pin::Pin;

use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};

use fusion_core::error::{AppError, AppResult};
use fusion_core::models::ai::RemoteModelVo;

use super::client::{build_client, describe_error};
use super::grok::infer_model_type;
use super::{
    ImageProvider, ImageRequest, ImageResult, Provider, ProviderConfig, VideoPoll, VideoProvider,
    VideoSubmit,
};
use crate::types::{ChatRequest, ChatResponse, StreamDelta, ToolCall};

pub struct OpenAiCompatProvider;

impl OpenAiCompatProvider {
    pub fn new() -> Self {
        Self
    }

    fn build_body(req: &ChatRequest, stream: bool) -> Value {
        let mut body = json!({
            "model": req.model,
            "messages": req.messages,
            "stream": stream,
        });
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(m) = req.max_tokens {
            body["max_tokens"] = json!(m);
        }
        if !req.tools.is_empty() {
            body["tools"] = json!(req.tools);
            body["tool_choice"] = json!(req.tool_choice.clone().unwrap_or_else(|| "auto".into()));
        }
        body
    }
}

impl Default for OpenAiCompatProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiCompatProvider {
    fn platform(&self) -> &'static str {
        "openai_compatible"
    }

    async fn chat(&self, cfg: &ProviderConfig, req: ChatRequest) -> AppResult<ChatResponse> {
        cfg.require_key()?;
        let client = build_client(cfg)?;
        let resp = client
            .post(format!("{}/chat/completions", cfg.v1()))
            .bearer_auth(&cfg.api_key)
            .json(&Self::build_body(&req, false))
            .send()
            .await
            .map_err(|e| AppError::Http(format!("请求失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(AppError::Http(describe_error(status, &text)));
        }
        let value: Value = serde_json::from_str(&text)?;
        let choice = value
            .get("choices")
            .and_then(|c| c.get(0))
            .ok_or_else(|| AppError::Http("上游未返回 choices".into()))?;
        let message = choice.get("message").cloned().unwrap_or(Value::Null);
        let usage = value.get("usage").cloned().unwrap_or(Value::Null);

        Ok(ChatResponse {
            content: message
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            reasoning: message
                .get("reasoning_content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            tool_calls: message
                .get("tool_calls")
                .and_then(|v| serde_json::from_value::<Vec<ToolCall>>(v.clone()).ok())
                .unwrap_or_default(),
            prompt_tokens: usage
                .get("prompt_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            completion_tokens: usage
                .get("completion_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            finish_reason: choice
                .get("finish_reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })
    }

    fn chat_stream(
        &self,
        cfg: ProviderConfig,
        req: ChatRequest,
    ) -> Pin<Box<dyn Stream<Item = AppResult<StreamDelta>> + Send>> {
        let body = Self::build_body(&req, true);
        Box::pin(async_stream::try_stream! {
            cfg.require_key()?;
            let client = build_client(&cfg)?;
            let resp = client
                .post(format!("{}/chat/completions", cfg.v1()))
                .bearer_auth(&cfg.api_key)
                .json(&body)
                .send()
                .await
                .map_err(|e| AppError::Http(format!("请求失败: {e}")))?;
            let status = resp.status().as_u16();
            let mut stream = if (200..300).contains(&status) {
                resp.bytes_stream()
            } else {
                let text = resp.text().await.unwrap_or_default();
                Err::<(), _>(AppError::Http(describe_error(status, &text)))?;
                unreachable!()
            };
            let mut buffer = String::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| AppError::Http(format!("读取流失败: {e}")))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim().to_string();
                    buffer.drain(..=pos);
                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }
                    let Some(payload) = line.strip_prefix("data:") else { continue };
                    let payload = payload.trim();
                    if payload == "[DONE]" {
                        yield StreamDelta::Done;
                        return;
                    }
                    let Ok(value) = serde_json::from_str::<Value>(payload) else { continue };
                    let Some(delta) = value
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("delta"))
                    else { continue };
                    if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            yield StreamDelta::Content(text.to_string());
                        }
                    }
                    if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                        for (idx, call) in calls.iter().enumerate() {
                            yield StreamDelta::ToolCallDelta {
                                index: call.get("index").and_then(|v| v.as_u64()).unwrap_or(idx as u64) as usize,
                                id: call.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                name: call.get("function").and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                arguments: call.get("function").and_then(|f| f.get("arguments")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            };
                        }
                    }
                }
            }
            yield StreamDelta::Done;
        })
    }

    async fn list_models(&self, cfg: &ProviderConfig) -> AppResult<Vec<RemoteModelVo>> {
        cfg.require_key()?;
        let client = build_client(cfg)?;
        let resp = client
            .get(format!("{}/models", cfg.v1()))
            .bearer_auth(&cfg.api_key)
            .send()
            .await
            .map_err(|e| AppError::Http(format!("获取模型列表失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(AppError::Http(describe_error(status, &text)));
        }
        let value: Value = serde_json::from_str(&text)?;
        Ok(value
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|m| {
                let id = m
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let model_type = infer_model_type(&id);
                RemoteModelVo {
                    name: id.clone(),
                    id,
                    owned_by: m
                        .get("owned_by")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    model_type,
                    inferred: model_type.is_some(),
                }
            })
            .collect())
    }
}

#[async_trait::async_trait]
impl ImageProvider for OpenAiCompatProvider {
    fn platform(&self) -> &'static str {
        "openai_compatible"
    }

    async fn generate(&self, cfg: &ProviderConfig, req: ImageRequest) -> AppResult<ImageResult> {
        cfg.require_key()?;
        let client = build_client(cfg)?;
        let count = req.count.max(1);

        if req.reference_images.is_empty() {
            let mut body = json!({
                "model": req.model,
                "prompt": req.prompt,
                "n": count,
            });
            if let Some(size) = &req.size {
                if !size.trim().is_empty() {
                    body["size"] = json!(size);
                }
            }
            let resp = client
                .post(format!("{}/images/generations", cfg.v1()))
                .bearer_auth(&cfg.api_key)
                .json(&body)
                .send()
                .await
                .map_err(|e| AppError::Http(format!("图像生成请求失败: {e}")))?;
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            if !(200..300).contains(&status) {
                return Err(AppError::Http(describe_error(status, &text)));
            }
            let value: Value = serde_json::from_str(&text)?;
            let urls = value
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|item| {
                    item.get("url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            item.get("b64_json")
                                .and_then(|v| v.as_str())
                                .map(|b| format!("data:image/png;base64,{b}"))
                        })
                })
                .collect::<Vec<_>>();
            return Ok(ImageResult { urls });
        }

        // 图生图：官方 multipart 形式（image 可重复）。
        let mut form = reqwest::multipart::Form::new()
            .text("model", req.model.clone())
            .text("prompt", req.prompt.clone())
            .text("n", count.to_string());
        if let Some(size) = &req.size {
            if !size.trim().is_empty() {
                form = form.text("size", size.clone());
            }
        }
        for (idx, url) in req.reference_images.iter().enumerate() {
            let bytes = download_bytes(cfg, url).await?;
            let part = reqwest::multipart::Part::bytes(bytes)
                .file_name(format!("reference-{idx}.png"))
                .mime_str("image/png")
                .map_err(|e| AppError::Http(e.to_string()))?;
            form = form.part("image", part);
        }

        let resp = client
            .post(format!("{}/images/edits", cfg.v1()))
            .bearer_auth(&cfg.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| AppError::Http(format!("图生图请求失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(AppError::Http(describe_error(status, &text)));
        }
        let value: Value = serde_json::from_str(&text)?;
        let urls = value
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|item| {
                item.get("url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect::<Vec<_>>();
        Ok(ImageResult { urls })
    }
}

async fn download_bytes(cfg: &ProviderConfig, url: &str) -> AppResult<Vec<u8>> {
    if let Some(b64) = url.strip_prefix("data:") {
        if let Some((_, data)) = b64.split_once(",") {
            use base64::Engine;
            return base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| AppError::Http(format!("base64 解码失败: {e}")));
        }
    }
    let client = build_client(cfg)?;
    let resp = client
        .get(url)
        .bearer_auth(&cfg.api_key)
        .send()
        .await
        .map_err(|e| AppError::Http(format!("下载参考图失败: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Http(format!(
            "下载参考图失败: HTTP {}",
            resp.status()
        )));
    }
    Ok(resp
        .bytes()
        .await
        .map_err(|e| AppError::Http(e.to_string()))?
        .to_vec())
}

#[async_trait::async_trait]
impl VideoProvider for OpenAiCompatProvider {
    fn platform(&self) -> &'static str {
        "openai_compatible"
    }

    async fn submit(&self, cfg: &ProviderConfig, req: VideoSubmit) -> AppResult<String> {
        cfg.require_key()?;
        let client = build_client(cfg)?;
        let mut form = reqwest::multipart::Form::new().text("model", req.model.clone());
        if !req.prompt.trim().is_empty() {
            form = form.text("prompt", req.prompt.clone());
        }
        if let Some(d) = req.duration {
            form = form.text("seconds", d.to_string());
        }
        if let Some(res) = &req.resolution {
            if !res.trim().is_empty() {
                form = form.text("size", res.clone());
            }
        }
        if let Some(url) = &req.image_url {
            if !url.trim().is_empty() {
                let bytes = download_bytes(cfg, url).await?;
                let part = reqwest::multipart::Part::bytes(bytes)
                    .file_name("reference.png")
                    .mime_str("image/png")
                    .map_err(|e| AppError::Http(e.to_string()))?;
                form = form.part("input_reference", part);
            }
        }

        let resp = client
            .post(format!("{}/videos", cfg.v1()))
            .bearer_auth(&cfg.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| AppError::Http(format!("视频提交失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(AppError::Http(describe_error(status, &text)));
        }
        let value: Value = serde_json::from_str(&text)?;
        value
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::Http("上游未返回视频任务 ID".into()))
    }

    async fn poll(&self, cfg: &ProviderConfig, platform_task_id: &str) -> AppResult<VideoPoll> {
        cfg.require_key()?;
        let client = build_client(cfg)?;
        let resp = client
            .get(format!("{}/videos/{}", cfg.v1(), platform_task_id))
            .bearer_auth(&cfg.api_key)
            .send()
            .await
            .map_err(|e| AppError::Http(format!("查询视频任务失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(AppError::Http(describe_error(status, &text)));
        }
        let value: Value = serde_json::from_str(&text)?;
        match value
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("queued")
        {
            "completed" | "succeeded" | "done" => Ok(VideoPoll::Done {
                url: format!("{}/videos/{}/content", cfg.v1(), platform_task_id),
                duration: value.get("seconds").and_then(|v| v.as_i64()),
                cover: None,
            }),
            "failed" | "cancelled" | "canceled" => Ok(VideoPoll::Failed {
                message: value
                    .get("error")
                    .and_then(|e| e.get("message").and_then(|m| m.as_str()))
                    .unwrap_or("视频生成失败")
                    .to_string(),
            }),
            _ => Ok(VideoPoll::Pending { progress: None }),
        }
    }

    fn content_url(&self, cfg: &ProviderConfig, platform_task_id: &str, direct: &str) -> String {
        if direct.starts_with("http") {
            direct.to_string()
        } else {
            format!("{}/videos/{}/content", cfg.v1(), platform_task_id)
        }
    }
}
