use super::ai::{load_model, resolve_provider_config};
use crate::{
    auth::AuthUser,
    state::AppState,
    support::{ok, ApiResult},
};
use axum::{
    extract::{Multipart, State},
    routing::post,
    Json, Router,
};
use fusion_core::error::AppError;
use serde::Deserialize;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/generation/audio/speech", post(speech))
        .route("/api/generation/audio/transcribe", post(transcribe))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpeechRequest {
    model_id: i64,
    input: String,
    voice: Option<String>,
}

async fn speech(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<SpeechRequest>,
) -> ApiResult<String> {
    if req.input.trim().is_empty() || req.input.chars().count() > 10000 {
        return Err(AppError::bad_request("配音文本需为 1–10000 个字符"));
    }
    let model = load_model(&state, req.model_id).await?;
    if model.model_type != 4 {
        return Err(AppError::bad_request("请选择语音合成模型"));
    }
    let (platform, cfg) = resolve_provider_config(&state, &model).await?;
    let bytes = state
        .registry
        .speech(&platform)?
        .synthesize(&cfg, &model.code, req.input.trim(), req.voice.as_deref())
        .await?;
    if bytes.is_empty() {
        return Err(AppError::business("模型未返回音频，请重试"));
    }
    ok(state.media.save_bytes("audios", "mp3", &bytes)?)
}

async fn transcribe(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    mut form: Multipart,
) -> ApiResult<String> {
    let mut model_id = None;
    let mut audio = None;
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(e.to_string()))?
    {
        match field.name().unwrap_or("") {
            "modelId" => model_id = field.text().await.ok().and_then(|v| v.parse::<i64>().ok()),
            "file" => {
                let name = field.file_name().unwrap_or("audio.mp3").to_string();
                let mime = field.content_type().unwrap_or("audio/mpeg").to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::bad_request(e.to_string()))?;
                if bytes.is_empty() || bytes.len() > 25 * 1024 * 1024 {
                    return Err(AppError::bad_request("请选择 25MB 以内的音频文件"));
                }
                audio = Some((name, mime, bytes.to_vec()));
            }
            _ => {}
        }
    }
    let model = load_model(
        &state,
        model_id.ok_or_else(|| AppError::bad_request("请选择识别模型"))?,
    )
    .await?;
    if model.model_type != 5 {
        return Err(AppError::bad_request("请选择语音识别模型"));
    }
    let (name, mime, bytes) = audio.ok_or_else(|| AppError::bad_request("请上传音频"))?;
    let (platform, cfg) = resolve_provider_config(&state, &model).await?;
    ok(state
        .registry
        .speech(&platform)?
        .transcribe(&cfg, &model.code, &name, bytes, &mime)
        .await?)
}
