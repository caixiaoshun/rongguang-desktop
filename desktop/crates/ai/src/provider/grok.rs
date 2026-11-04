//! Grok（grok2api，OpenAI 兼容）适配器。
//!
//! 实测差异（以真机验证为准，不可照搬 OpenAI 文档）：
//! - 对话：标准 `/v1/chat/completions`，SSE 中可能夹杂 `: grok2api-reasoning-evidence` 注释帧；
//! - 文生图：`/v1/images/generations`，`size` 会就近取模；
//! - 图生图：`/v1/images/edits` **只接受** `images:[{"url": ...}]`，传 `image:"url"` 会 400；
//! - 文生视频：`/v1/videos/generations` 为 **JSON 包体**，返回 `{"request_id"}`；
//! - 轮询：`GET /v1/videos/{id}`，状态 `pending` / `done` / `failed`，成品在 `video.url`；
//! - 语音：`/v1/audio/speech`（audio/mpeg）与 `/v1/audio/transcriptions`（multipart）。

use std::pin::Pin;

use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};

use fusion_core::error::{AppError, AppResult};
use fusion_core::models::ai::RemoteModelVo;

use super::client::{build_client, describe_error};
use super::{
    ImageProvider, ImageRequest, ImageResult, Provider, ProviderConfig, SpeechProvider, VideoPoll,
    VideoProvider, VideoSubmit,
};
use crate::types::{ChatRequest, ChatResponse, StreamDelta, ToolCall};

pub struct GrokProvider;

impl GrokProvider {
    pub fn new() -> Self {
        Self
    }

    fn build_body(&self, req: &ChatRequest, stream: bool) -> Value {
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

    async fn post_json(&self, cfg: &ProviderConfig, path: &str, body: Value) -> AppResult<Value> {
        cfg.require_base_url()?;
        cfg.require_key()?;
        let client = build_client(cfg)?;
        let url = format!("{}{}", cfg.v1(), path);
        let resp = client
            .post(&url)
            .bearer_auth(&cfg.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Http(format!("请求 Grok 失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(AppError::Http(describe_error(status, &text)));
        }
        serde_json::from_str(&text).map_err(|e| AppError::Http(format!("响应解析失败: {e}")))
    }
}

impl Default for GrokProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn chinese_text_survives_a_split_utf8_character() {
        for compat in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base_url = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 8192];
                socket.read(&mut request).await.unwrap();
                let body = "data: {\"choices\":[{\"delta\":{\"content\":\"中文台词\"}}]}\n\ndata: [DONE]\n\n";
                let split = body.find('中').unwrap() + 1;
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
                socket.write_all(&body.as_bytes()[..split]).await.unwrap();
                socket.flush().await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                socket.write_all(&body.as_bytes()[split..]).await.unwrap();
            });
            let cfg = ProviderConfig {
                platform: "grok".into(),
                base_url,
                api_key: "test".into(),
                proxy_type: Some("none".into()),
                proxy_host: None,
                proxy_port: None,
                proxy_username: None,
                proxy_password: None,
            };
            let provider: Box<dyn Provider> = if compat {
                Box::new(crate::provider::openai_compat::OpenAiCompatProvider::new())
            } else {
                Box::new(GrokProvider::new())
            };
            let mut stream = provider.chat_stream(cfg, ChatRequest::default());
            let mut text = String::new();
            while let Some(delta) = stream.next().await {
                if let StreamDelta::Content(chunk) = delta.unwrap() {
                    text.push_str(&chunk);
                }
            }
            assert_eq!(text, "中文台词");
            server.await.unwrap();
        }
    }
}

#[async_trait::async_trait]
impl Provider for GrokProvider {
    fn platform(&self) -> &'static str {
        "grok"
    }

    async fn chat(&self, cfg: &ProviderConfig, req: ChatRequest) -> AppResult<ChatResponse> {
        let body = self.build_body(&req, false);
        let value = self.post_json(cfg, "/chat/completions", body).await?;
        let choice = value
            .get("choices")
            .and_then(|c| c.get(0))
            .ok_or_else(|| AppError::Http("上游未返回 choices".into()))?;

        let message = choice.get("message").cloned().unwrap_or(Value::Null);
        let content = match message.get("content") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        };
        let reasoning = message
            .get("reasoning_content")
            .or_else(|| message.get("reasoning"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let tool_calls = message
            .get("tool_calls")
            .and_then(|v| serde_json::from_value::<Vec<ToolCall>>(v.clone()).ok())
            .unwrap_or_default();

        let usage = value.get("usage").cloned().unwrap_or(Value::Null);
        Ok(ChatResponse {
            content,
            reasoning,
            tool_calls,
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
        let body = self.build_body(&req, true);
        Box::pin(async_stream::try_stream! {
            cfg.require_base_url()?;
            cfg.require_key()?;
            let client = build_client(&cfg)?;
            let url = format!("{}/chat/completions", cfg.v1());
            let resp = client
                .post(&url)
                .bearer_auth(&cfg.api_key)
                .json(&body)
                .send()
                .await
                .map_err(|e| AppError::Http(format!("请求 Grok 失败: {e}")))?;

            let status = resp.status().as_u16();
            // 非 2xx 时把响应体读成文本用于报错，其余情况直接交给流式解析。
            let mut stream = if (200..300).contains(&status) {
                resp.bytes_stream()
            } else {
                let text = resp.text().await.unwrap_or_default();
                Err::<(), _>(AppError::Http(describe_error(status, &text)))?;
                unreachable!()
            };
            let mut buffer = Vec::<u8>::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| AppError::Http(format!("读取流失败: {e}")))?;
                buffer.extend_from_slice(&chunk);
                while let Some(pos) = buffer.iter().position(|byte| *byte == b'\n') {
                    let line = String::from_utf8_lossy(&buffer[..pos]).trim_end_matches('\r').to_string();
                    buffer.drain(..=pos);
                    let line = line.trim();
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
                    if let Some(err) = value.get("error") {
                        let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("上游流式错误");
                        Err(AppError::Http(msg.to_string()))?;
                    }
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
                    if let Some(text) = delta
                        .get("reasoning_content")
                        .or_else(|| delta.get("reasoning"))
                        .and_then(|v| v.as_str())
                    {
                        if !text.is_empty() {
                            yield StreamDelta::Reasoning(text.to_string());
                        }
                    }
                    if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                        for (idx, call) in calls.iter().enumerate() {
                            let index = call.get("index").and_then(|v| v.as_u64()).unwrap_or(idx as u64) as usize;
                            let id = call.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let name = call
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let args = call
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            yield StreamDelta::ToolCallDelta { index, id, name, arguments: args };
                        }
                    }
                }
            }
            yield StreamDelta::Done;
        })
    }

    async fn list_models(&self, cfg: &ProviderConfig) -> AppResult<Vec<RemoteModelVo>> {
        cfg.require_base_url()?;
        cfg.require_key()?;
        let client = build_client(cfg)?;
        let url = format!("{}/models", cfg.v1());
        let resp = client
            .get(&url)
            .bearer_auth(&cfg.api_key)
            .send()
            .await
            .map_err(|e| AppError::Http(format!("获取模型列表失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(AppError::Http(describe_error(status, &text)));
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| AppError::Http(format!("模型列表解析失败: {e}")))?;
        let models = value
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
            .collect();
        Ok(models)
    }
}

/// 按模型标识推断能力类型：1 对话 2 图像 3 视频 4 TTS 5 STT。
pub fn infer_model_type(id: &str) -> Option<i64> {
    let lower = id.to_lowercase();
    if lower.contains("video")
        || lower.contains("sora")
        || lower.contains("kling")
        || lower.contains("seedance")
    {
        return Some(3);
    }
    if lower.contains("image")
        || lower.contains("imagine-image")
        || lower.contains("dall")
        || lower.contains("flux")
    {
        return Some(2);
    }
    if lower.contains("tts") || lower.contains("voice") || lower.contains("speech") {
        return Some(4);
    }
    if lower.contains("stt") || lower.contains("whisper") || lower.contains("transcri") {
        return Some(5);
    }
    if lower.contains("chat") || lower.contains("grok-") || lower.contains("composer") {
        return Some(1);
    }
    None
}

#[async_trait::async_trait]
impl ImageProvider for GrokProvider {
    fn platform(&self) -> &'static str {
        "grok"
    }

    async fn generate(&self, cfg: &ProviderConfig, req: ImageRequest) -> AppResult<ImageResult> {
        let count = req.count.max(1);
        let mut urls = Vec::new();

        for _ in 0..count {
            let (path, body) = if req.reference_images.is_empty() {
                let mut body = json!({
                    "model": req.model,
                    "prompt": req.prompt,
                    "n": 1,
                });
                if let Some(size) = &req.size {
                    if !size.trim().is_empty() {
                        body["size"] = json!(size);
                    }
                }
                ("/images/generations", body)
            } else {
                // 实测：只接受 images:[{url}]，且支持 size。
                let images: Vec<Value> = req
                    .reference_images
                    .iter()
                    .map(|u| json!({ "url": u }))
                    .collect();
                let mut body = json!({
                    "model": req.model,
                    "prompt": req.prompt,
                    "images": images,
                });
                if let Some(size) = &req.size {
                    if !size.trim().is_empty() {
                        body["size"] = json!(size);
                    }
                }
                ("/images/edits", body)
            };

            let value = self.post_json(cfg, path, body).await?;
            let data = value
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();
            for item in data {
                if let Some(url) = item.get("url").and_then(|v| v.as_str()) {
                    urls.push(url.to_string());
                } else if let Some(b64) = item.get("b64_json").and_then(|v| v.as_str()) {
                    urls.push(format!("data:image/png;base64,{b64}"));
                }
            }
        }

        if urls.is_empty() {
            return Err(AppError::Http("上游未返回图片地址".into()));
        }
        Ok(ImageResult { urls })
    }
}

#[async_trait::async_trait]
impl VideoProvider for GrokProvider {
    fn platform(&self) -> &'static str {
        "grok"
    }

    async fn submit(&self, cfg: &ProviderConfig, req: VideoSubmit) -> AppResult<String> {
        let mut body = json!({
            "model": req.model,
            "prompt": req.prompt,
        });
        if let Some(url) = &req.image_url {
            if !url.trim().is_empty() {
                body["image"] = json!({ "url": url });
            }
        }
        if let Some(duration) = req.duration {
            body["duration"] = json!(duration);
        }
        if let Some(ratio) = &req.aspect_ratio {
            if !ratio.trim().is_empty() {
                body["aspect_ratio"] = json!(ratio);
            }
        }
        if let Some(res) = &req.resolution {
            if !res.trim().is_empty() {
                body["resolution"] = json!(res);
            }
        }

        let value = self.post_json(cfg, "/videos/generations", body).await?;
        value
            .get("request_id")
            .or_else(|| value.get("id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::Http("上游未返回视频任务 ID".into()))
    }

    async fn poll(&self, cfg: &ProviderConfig, platform_task_id: &str) -> AppResult<VideoPoll> {
        cfg.require_base_url()?;
        cfg.require_key()?;
        let client = build_client(cfg)?;
        let url = format!("{}/videos/{}", cfg.v1(), platform_task_id);
        let resp = client
            .get(&url)
            .bearer_auth(&cfg.api_key)
            .send()
            .await
            .map_err(|e| AppError::Http(format!("查询视频任务失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(AppError::Http(describe_error(status, &text)));
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| AppError::Http(format!("视频状态解析失败: {e}")))?;

        let state = value
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending");
        match state {
            "done" | "completed" | "succeeded" | "success" => {
                let video = value.get("video").cloned().unwrap_or(Value::Null);
                let url = video
                    .get("url")
                    .or_else(|| value.get("url"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                if url.is_empty() {
                    return Ok(VideoPoll::Failed {
                        message: "任务完成但未返回视频地址".into(),
                    });
                }
                Ok(VideoPoll::Done {
                    url,
                    duration: video.get("duration").and_then(|v| v.as_i64()),
                    cover: video
                        .get("cover_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                })
            }
            "failed" | "error" | "canceled" | "cancelled" => Ok(VideoPoll::Failed {
                message: value
                    .get("error")
                    .and_then(|e| {
                        e.get("message")
                            .and_then(|m| m.as_str())
                            .or_else(|| e.as_str())
                    })
                    .unwrap_or("视频生成失败")
                    .to_string(),
            }),
            _ => Ok(VideoPoll::Pending {
                progress: value.get("progress").and_then(|v| v.as_i64()),
            }),
        }
    }
}

#[async_trait::async_trait]
impl SpeechProvider for GrokProvider {
    fn platform(&self) -> &'static str {
        "grok"
    }

    async fn synthesize(
        &self,
        cfg: &ProviderConfig,
        model: &str,
        input: &str,
        voice: Option<&str>,
    ) -> AppResult<Vec<u8>> {
        cfg.require_base_url()?;
        cfg.require_key()?;
        let client = build_client(cfg)?;
        let body = json!({
            "model": model,
            "input": input,
            "voice": voice.unwrap_or("alloy"),
        });
        let resp = client
            .post(format!("{}/audio/speech", cfg.v1()))
            .bearer_auth(&cfg.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Http(format!("语音合成失败: {e}")))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Http(describe_error(status, &text)));
        }
        Ok(resp
            .bytes()
            .await
            .map_err(|e| AppError::Http(e.to_string()))?
            .to_vec())
    }

    async fn transcribe(
        &self,
        cfg: &ProviderConfig,
        model: &str,
        file_name: &str,
        bytes: Vec<u8>,
        mime: &str,
    ) -> AppResult<String> {
        cfg.require_base_url()?;
        cfg.require_key()?;
        let client = build_client(cfg)?;
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(file_name.to_string())
            .mime_str(mime)
            .map_err(|e| AppError::Http(format!("音频类型无效: {e}")))?;
        let form = reqwest::multipart::Form::new()
            .text("model", model.to_string())
            .part("file", part);
        let resp = client
            .post(format!("{}/audio/transcriptions", cfg.v1()))
            .bearer_auth(&cfg.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| AppError::Http(format!("语音识别失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(AppError::Http(describe_error(status, &text)));
        }
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(value
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or(text.as_str())
            .to_string())
    }
}
