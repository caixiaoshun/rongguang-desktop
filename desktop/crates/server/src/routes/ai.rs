//! AI 供应商配置与模型管理。
//!
//! 默认把 Grok 作为主供应商：初始化时即写入一条 `platform = grok` 的接入配置，
//! 并按实测能力矩阵写入 18 个模型，用户只需补一把 API Key。

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use fusion_ai::provider::ProviderConfig;
use fusion_core::error::AppError;
use fusion_core::models::ai::{
    AiModel, AiModelSaveReq, ApiConfig, ApiConfigSaveReq, ModelConnectivityResult, ModelPreset,
    RemoteModelVo,
};
use fusion_core::models::{PageQuery, PageResult};

use crate::auth::AuthUser;
use crate::state::AppState;
use crate::support::{now, ok, to_json_text, ApiResult};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/ai/api-config/create", post(api_config_create))
        .route("/api/ai/api-config/update", put(api_config_update))
        .route("/api/ai/api-config/delete", axum::routing::delete(api_config_delete))
        .route("/api/ai/api-config/get", get(api_config_get))
        .route("/api/ai/api-config/page", get(api_config_page))
        .route("/api/ai/api-config/list", get(api_config_list))
        .route("/api/ai/api-config/remote-models", get(api_config_remote_models))
        .route(
            "/api/ai/api-config/test-comfyui-connectivity",
            post(test_comfyui),
        )
        .route("/api/ai/model/create", post(model_create))
        .route("/api/ai/model/update", put(model_update))
        .route("/api/ai/model/delete", axum::routing::delete(model_delete))
        .route("/api/ai/model/get", get(model_get))
        .route("/api/ai/model/page", get(model_page))
        .route("/api/ai/model/list", get(model_list))
        .route("/api/ai/model/list-by-type", get(model_list_by_type))
        .route("/api/ai/model/presets", get(model_presets))
        .route("/api/ai/model/preset-config", get(preset_config))
        .route(
            "/api/ai/model/test-text-connectivity",
            post(test_text_connectivity),
        )
}

/// 默认模型清单：来自 grok2api 实测的 18 个模型（类型 1 对话 / 2 图像 / 3 视频 / 4 TTS / 5 STT）。
pub const GROK_MODELS: &[(&str, &str, i64, bool, bool)] = &[
    ("grok-4.7", "Grok 4.7", 1, true, true),
    ("grok-4.5", "Grok 4.5", 1, true, true),
    ("grok-4.3", "Grok 4.3", 1, true, true),
    ("grok-4.20-0309-reasoning", "Grok 4.20 推理", 1, true, true),
    ("grok-4.20-0309-non-reasoning", "Grok 4.20 非推理", 1, true, false),
    ("grok-4.20-multi-agent-0309", "Grok 4.20 多智能体", 1, true, true),
    ("grok-composer-2.5-fast", "Grok Composer 2.5 Fast", 1, false, false),
    ("grok-build-0.1", "Grok Build 0.1", 1, false, false),
    ("grok-imagine-image", "Grok Imagine 图像", 2, false, false),
    ("grok-imagine-image-2.0", "Grok Imagine 图像 2.0", 2, false, false),
    ("grok-imagine-image-quality", "Grok Imagine 图像（高清）", 2, false, false),
    ("grok-imagine-image-edit", "Grok Imagine 图像编辑", 2, false, false),
    ("grok-imagine-video", "Grok Imagine 视频", 3, false, false),
    ("grok-imagine-video-1.5", "Grok Imagine 视频 1.5", 3, false, false),
    ("grok-voice-latest", "Grok Voice", 4, false, false),
    ("grok-voice-think-fast-1.0", "Grok Voice Think 1.0", 4, false, false),
    ("grok-voice-think-fast-2.0", "Grok Voice Think 2.0", 4, false, false),
    ("grok-stt", "Grok 语音识别", 5, false, false),
];

/// 初始化时写入 Grok 接入配置与模型（幂等）。
pub async fn seed_grok_models(state: &AppState) -> Result<(), AppError> {
    let existing: Option<i64> =
        sqlx::query_scalar("SELECT id FROM afv_api_config WHERE platform = 'grok' AND deleted = 0 LIMIT 1")
            .fetch_optional(&state.db)
            .await?;

    let config_id = match existing {
        Some(id) => id,
        None => {
            // 状态仍为启用：这条配置是「默认主供应商」槽位，让用户在列表里一眼看到并直接点开填写。
            // 真正拦在校验上——地址或 Key 为空时，生成接口会给出明确提示。
            sqlx::query(
                "INSERT INTO afv_api_config (name, platform, text_protocol, image_protocol, video_protocol, api_type, api_url, api_key, proxy_type, status, remark, deleted, create_time, update_time)
                 VALUES (?, 'grok', 'grok', 'grok', 'grok', 1, ?, '', 'none', 1, '默认主供应商，填入服务地址与 API Key 后可用', 0, ?, ?)",
            )
            .bind("Grok（默认主供应商）")
            .bind(String::new())
            .bind(now())
            .bind(now())
            .execute(&state.db)
            .await?
            .last_insert_rowid()
        }
    };

    // 桌面版单机单用户：允许用运行期环境变量 FUSION_GROK_KEY 预置 Key（便于自建部署）。
    // 注意：编译期注入通道已移除——仓库与安装包都是公开分发的，二进制里不能烧进密钥。
    // 普通用户首次启动时 api_key 为空，需在「设置 → AI 模型」里自行填写。
    let bundled_key = std::env::var("FUSION_GROK_KEY")
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    if let Some(key) = bundled_key {
        let empty: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM afv_api_config WHERE platform = 'grok' AND deleted = 0 AND (api_key IS NULL OR api_key = '') LIMIT 1",
        )
        .fetch_optional(&state.db)
        .await?;
        if let Some(id) = empty {
            sqlx::query("UPDATE afv_api_config SET api_key = ?, update_time = ? WHERE id = ?")
                .bind(&key)
                .bind(now())
                .bind(id)
                .execute(&state.db)
                .await?;
            tracing::info!("已为 Grok 供应商填充内置 API Key");
        }
    }

    let count: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM afv_ai_model WHERE deleted = 0")
        .fetch_one(&state.db)
        .await?;
    if count == 0 {
        for (idx, (code, name, model_type, vision, reasoning)) in GROK_MODELS.iter().enumerate() {
            let config = if *model_type == 3 {
                Some(r#"{"supportedDurations":[6,10],"supportedRatios":["16:9","9:16","1:1"],"defaultDuration":6}"#.to_string())
            } else {
                None
            };
            sqlx::query(
                "INSERT INTO afv_ai_model (name, code, model_protocol, model_type, sort, status, config, default_model, max_concurrency, api_config_id, support_vision, multimodal_input_types, multimodal_input_transports, support_reasoning, reasoning_effort_levels, context_window, deleted, create_time, update_time)
                 VALUES (?, ?, 'grok', ?, ?, 1, ?, ?, 2, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
            )
            .bind(name)
            .bind(code)
            .bind(model_type)
            .bind(idx as i64)
            .bind(config)
            .bind(if *code == "grok-4.5" { 1 } else { 0 })
            .bind(config_id)
            .bind(if *vision { 1 } else { 0 })
            .bind(if *vision { r#"["image"]"# } else { "[]" })
            .bind(if *vision { r#"{"image":["url"]}"# } else { "{}" })
            .bind(if *reasoning { 1 } else { 0 })
            .bind(if *reasoning { r#"["low","medium","high"]"# } else { "[]" })
            .bind(131072i64)
            .bind(now())
            .bind(now())
            .execute(&state.db)
            .await?;
        }
    }

    Ok(())
}

/// 桌面端主推 Grok，其余平台默认「降级为可选」：
/// 未配置 Key 的存量配置会被置为停用，仅在用户主动填写 Key 后才恢复，
/// 避免首次打开时满屏都是用不了的模型。
pub async fn seed_demote_other_providers(state: &AppState) -> Result<(), AppError> {
    let affected = sqlx::query(
        "UPDATE afv_api_config
            SET status = 0, update_time = ?
          WHERE deleted = 0
            AND status = 1
            AND platform IS NOT NULL
            AND platform <> 'grok'
            AND (api_key IS NULL OR api_key = '')",
    )
    .bind(now())
    .execute(&state.db)
    .await?
    .rows_affected();

    let models = sqlx::query(
        "UPDATE afv_ai_model
            SET status = 0, update_time = ?
          WHERE deleted = 0
            AND status = 1
            AND model_protocol IS NOT NULL
            AND model_protocol <> 'grok'
            AND api_config_id IN (
                SELECT id FROM afv_api_config
                 WHERE deleted = 0 AND status = 0 AND platform <> 'grok'
            )",
    )
    .bind(now())
    .execute(&state.db)
    .await?
    .rows_affected();

    if affected > 0 || models > 0 {
        tracing::info!("已弱化非 Grok 平台：停用 {affected} 个未配置 Key 的接入、{models} 个模型");
    }
    Ok(())
}

pub async fn seed_storage(state: &AppState) -> Result<(), AppError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM afv_storage_config WHERE deleted = 0")
        .fetch_one(&state.db)
        .await?;
    if count == 0 {
        sqlx::query(
            "INSERT INTO afv_storage_config (name, type, provider, base_path, is_default, status, remark, deleted, create_time, update_time)
             VALUES ('本地媒体库', 'local', 'local', ?, 1, 1, '桌面端默认存储，文件保存在本机数据目录', 0, ?, ?)",
        )
        .bind(state.paths.media_dir.to_string_lossy().to_string())
        .bind(now())
        .bind(now())
        .execute(&state.db)
        .await?;
    }
    Ok(())
}

// ---------- API 配置 ----------

async fn api_config_create(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<ApiConfigSaveReq>,
) -> ApiResult<i64> {
    let platform = req.platform.clone().unwrap_or_else(|| "grok".into());
    let id = sqlx::query(
        "INSERT INTO afv_api_config (name, platform, text_protocol, image_protocol, video_protocol, api_type, api_url, proxy_type, proxy_host, proxy_port, proxy_username, proxy_password, api_key, app_id, app_secret, model_id, status, remark, deleted, create_time, update_time)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
    )
    .bind(req.name.clone().unwrap_or_else(|| platform.clone()))
    .bind(&platform)
    .bind(req.text_protocol.clone())
    .bind(req.image_protocol.clone())
    .bind(req.video_protocol.clone())
    .bind(req.api_type.unwrap_or(1))
    .bind(req.api_url.clone())
    .bind(req.proxy_type.clone().unwrap_or_else(|| "none".into()))
    .bind(req.proxy_host.clone())
    .bind(req.proxy_port)
    .bind(req.proxy_username.clone())
    .bind(req.proxy_password.clone())
    .bind(req.api_key.clone().unwrap_or_default())
    .bind(req.app_id.clone())
    .bind(req.app_secret.clone())
    .bind(req.model_id)
    .bind(req.status.unwrap_or(1))
    .bind(req.remark.clone())
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();
    ok(id)
}

async fn api_config_update(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<ApiConfigSaveReq>,
) -> ApiResult<bool> {
    let id = req.id.ok_or_else(|| AppError::bad_request("缺少配置 ID"))?;
    sqlx::query(
        "UPDATE afv_api_config SET name = COALESCE(?, name), platform = COALESCE(?, platform),
         text_protocol = COALESCE(?, text_protocol), image_protocol = COALESCE(?, image_protocol),
         video_protocol = COALESCE(?, video_protocol), api_url = COALESCE(?, api_url),
         proxy_type = COALESCE(?, proxy_type), proxy_host = COALESCE(?, proxy_host),
         proxy_port = COALESCE(?, proxy_port), proxy_username = COALESCE(?, proxy_username),
         proxy_password = COALESCE(?, proxy_password), api_key = COALESCE(?, api_key),
         app_id = COALESCE(?, app_id), app_secret = COALESCE(?, app_secret),
         status = COALESCE(?, status), remark = COALESCE(?, remark), update_time = ?
         WHERE id = ?",
    )
    .bind(req.name)
    .bind(req.platform)
    .bind(req.text_protocol)
    .bind(req.image_protocol)
    .bind(req.video_protocol)
    .bind(req.api_url)
    .bind(req.proxy_type)
    .bind(req.proxy_host)
    .bind(req.proxy_port)
    .bind(req.proxy_username)
    .bind(req.proxy_password)
    .bind(req.api_key)
    .bind(req.app_id)
    .bind(req.app_secret)
    .bind(req.status)
    .bind(req.remark)
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;

    // 同步运行时默认地址，避免设置页与内嵌配置漂移。
    if let Ok(url) = sqlx::query_scalar::<_, String>(
        "SELECT IFNULL(api_url,'') FROM afv_api_config WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    {
        if !url.trim().is_empty() {
            let mut cfg = state.config.write();
            cfg.grok_base_url = url;
            let _ = cfg.save(&state.paths.root);
        }
    }
    ok(true)
}

async fn api_config_delete(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_api_config SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(q.id)
        .execute(&state.db)
        .await?;
    ok(true)
}

#[derive(Deserialize)]
struct IdQuery {
    id: i64,
}

async fn api_config_get(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<ApiConfig> {
    let config = sqlx::query_as::<_, ApiConfig>(
        "SELECT * FROM afv_api_config WHERE id = ? AND deleted = 0",
    )
    .bind(q.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("配置不存在"))?;
    ok(config)
}

async fn api_config_list(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
) -> ApiResult<Vec<ApiConfig>> {
    let rows = sqlx::query_as::<_, ApiConfig>(
        "SELECT * FROM afv_api_config WHERE deleted = 0 ORDER BY status DESC, id ASC",
    )
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn api_config_page(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<PageQuery>,
) -> ApiResult<PageResult<ApiConfig>> {
    let page_no = q.page_no.max(1);
    let page_size = q.page_size.max(1);
    let total: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM afv_api_config WHERE deleted = 0")
        .fetch_one(&state.db)
        .await?;
    let rows = sqlx::query_as::<_, ApiConfig>(
        "SELECT * FROM afv_api_config WHERE deleted = 0 ORDER BY id ASC LIMIT ? OFFSET ?",
    )
    .bind(page_size)
    .bind(q.offset())
    .fetch_all(&state.db)
    .await?;
    ok(PageResult::new(rows, total, page_no, page_size))
}

async fn api_config_remote_models(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<Vec<RemoteModelVo>> {
    let config = load_api_config(&state, q.id).await?;
    let platform = resolve_platform(&config, None);
    let provider_cfg = ProviderConfig::from_api_config(&config, Some(&platform));
    let provider = state.registry.chat(&platform)?;
    let models = provider.list_models(&provider_cfg).await?;
    ok(models)
}

async fn test_comfyui(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<serde_json::Value> {
    let config = load_api_config(&state, q.id).await?;
    let base = config.api_url.clone().unwrap_or_default();
    if base.trim().is_empty() {
        return Err(AppError::bad_request("未配置 ComfyUI 地址"));
    }
    let url = format!("{}/system_stats", base.trim_end_matches('/'));
    match state.http.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => ok(json!({
            "success": true,
            "message": "连接成功",
        })),
        Ok(resp) => ok(json!({
            "success": false,
            "message": format!("连接失败: HTTP {}", resp.status().as_u16()),
        })),
        Err(err) => ok(json!({
            "success": false,
            "message": format!("连接失败: {err}"),
        })),
    }
}

pub async fn load_api_config(state: &AppState, id: i64) -> Result<ApiConfig, AppError> {
    sqlx::query_as::<_, ApiConfig>("SELECT * FROM afv_api_config WHERE id = ? AND deleted = 0")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("API 配置不存在"))
}

pub async fn load_model(state: &AppState, id: i64) -> Result<AiModel, AppError> {
    sqlx::query_as::<_, AiModel>(
        "SELECT m.*, c.platform AS platform FROM afv_ai_model m
         LEFT JOIN afv_api_config c ON c.id = m.api_config_id
         WHERE m.id = ? AND m.deleted = 0",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("模型不存在"))
}

/// 解析模型应使用的平台：模型协议 > API 配置平台 > 默认 grok。
pub fn resolve_platform(config: &ApiConfig, protocol: Option<&str>) -> String {
    protocol
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty() && p != "openai_compatible")
        .or_else(|| config.platform.clone())
        .map(|p| p.to_lowercase())
        .unwrap_or_else(|| "grok".into())
}

/// 解析模型请求协议（文本/图像/视频按类型取默认值）。
pub fn protocol_for(model: &AiModel, kind: ModelKind) -> Option<String> {
    if let Some(p) = model.model_protocol.clone().filter(|p| !p.trim().is_empty()) {
        return Some(p);
    }
    None.or_else(|| match kind {
        ModelKind::Chat => None,
        _ => None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Chat,
    Image,
    Video,
    Tts,
    Stt,
}

// ---------- 模型 ----------

async fn model_create(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<AiModelSaveReq>,
) -> ApiResult<i64> {
    let code = req
        .code
        .clone()
        .ok_or_else(|| AppError::bad_request("模型标识不能为空"))?;
    let id = sqlx::query(
        "INSERT INTO afv_ai_model (name, code, model_protocol, capability_preset_code, model_type, icon, description, sort, status, config, default_model, max_concurrency, api_config_id, support_vision, multimodal_input_types, multimodal_input_transports, support_reasoning, reasoning_effort_levels, context_window, deleted, create_time, update_time)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
    )
    .bind(req.name.clone().unwrap_or_else(|| code.clone()))
    .bind(&code)
    .bind(req.model_protocol.clone())
    .bind(req.capability_preset_code.clone())
    .bind(req.model_type.unwrap_or(1))
    .bind(req.icon.clone())
    .bind(req.description.clone())
    .bind(req.sort.unwrap_or(0))
    .bind(req.status.unwrap_or(1))
    .bind(req.config.clone())
    .bind(req.default_model.unwrap_or(false))
    .bind(req.max_concurrency.unwrap_or(2))
    .bind(req.api_config_id)
    .bind(req.support_vision.unwrap_or(false))
    .bind(to_json_text(&req.multimodal_input_types).unwrap_or_else(|| "[]".into()))
    .bind(to_json_text(&req.multimodal_input_transports).unwrap_or_else(|| "{}".into()))
    .bind(req.support_reasoning.unwrap_or(false))
    .bind(to_json_text(&req.reasoning_effort_levels).unwrap_or_else(|| "[]".into()))
    .bind(req.context_window)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    if req.default_model.unwrap_or(false) {
        clear_other_defaults(&state, id, req.model_type.unwrap_or(1)).await?;
    }
    ok(id)
}

async fn clear_other_defaults(state: &AppState, keep_id: i64, model_type: i64) -> Result<(), AppError> {
    sqlx::query("UPDATE afv_ai_model SET default_model = 0 WHERE id != ? AND model_type = ?")
        .bind(keep_id)
        .bind(model_type)
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn model_update(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<AiModelSaveReq>,
) -> ApiResult<bool> {
    let id = req.id.ok_or_else(|| AppError::bad_request("缺少模型 ID"))?;
    sqlx::query(
        "UPDATE afv_ai_model SET
           name = COALESCE(?, name),
           code = COALESCE(?, code),
           model_protocol = COALESCE(?, model_protocol),
           capability_preset_code = COALESCE(?, capability_preset_code),
           model_type = COALESCE(?, model_type),
           icon = COALESCE(?, icon),
           description = COALESCE(?, description),
           sort = COALESCE(?, sort),
           status = COALESCE(?, status),
           config = COALESCE(?, config),
           default_model = COALESCE(?, default_model),
           max_concurrency = COALESCE(?, max_concurrency),
           api_config_id = COALESCE(?, api_config_id),
           support_vision = COALESCE(?, support_vision),
           multimodal_input_types = COALESCE(?, multimodal_input_types),
           multimodal_input_transports = COALESCE(?, multimodal_input_transports),
           support_reasoning = COALESCE(?, support_reasoning),
           reasoning_effort_levels = COALESCE(?, reasoning_effort_levels),
           context_window = COALESCE(?, context_window),
           update_time = ?
         WHERE id = ?",
    )
    .bind(req.name)
    .bind(req.code)
    .bind(req.model_protocol)
    .bind(req.capability_preset_code)
    .bind(req.model_type)
    .bind(req.icon)
    .bind(req.description)
    .bind(req.sort)
    .bind(req.status)
    .bind(req.config)
    .bind(req.default_model)
    .bind(req.max_concurrency)
    .bind(req.api_config_id)
    .bind(req.support_vision)
    .bind(to_json_text(&req.multimodal_input_types))
    .bind(to_json_text(&req.multimodal_input_transports))
    .bind(req.support_reasoning)
    .bind(to_json_text(&req.reasoning_effort_levels))
    .bind(req.context_window)
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;

    if req.default_model == Some(true) {
        let model_type: i64 = sqlx::query_scalar("SELECT model_type FROM afv_ai_model WHERE id = ?")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
        clear_other_defaults(&state, id, model_type).await?;
    }
    ok(true)
}

async fn model_delete(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<bool> {
    sqlx::query("UPDATE afv_ai_model SET deleted = 1, update_time = ? WHERE id = ?")
        .bind(now())
        .bind(q.id)
        .execute(&state.db)
        .await?;
    ok(true)
}

async fn model_get(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<AiModel> {
    ok(load_model(&state, q.id).await?)
}

async fn model_list(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
) -> ApiResult<Vec<AiModel>> {
    let rows = sqlx::query_as::<_, AiModel>(
        "SELECT m.*, c.platform AS platform FROM afv_ai_model m
         LEFT JOIN afv_api_config c ON c.id = m.api_config_id
         WHERE m.deleted = 0 ORDER BY m.model_type ASC, m.sort ASC",
    )
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

#[derive(Deserialize)]
struct TypeQuery {
    #[serde(rename = "type")]
    model_type: Option<i64>,
}

async fn model_list_by_type(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<TypeQuery>,
) -> ApiResult<Vec<AiModel>> {
    let rows = sqlx::query_as::<_, AiModel>(
        "SELECT m.*, c.platform AS platform FROM afv_ai_model m
         LEFT JOIN afv_api_config c ON c.id = m.api_config_id
         WHERE m.deleted = 0 AND (? IS NULL OR m.model_type = ?)
         ORDER BY m.sort ASC",
    )
    .bind(q.model_type)
    .bind(q.model_type)
    .fetch_all(&state.db)
    .await?;
    ok(rows)
}

async fn model_page(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<PageQuery>,
) -> ApiResult<PageResult<AiModel>> {
    let page_no = q.page_no.max(1);
    let page_size = q.page_size.max(1);
    let total: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM afv_ai_model WHERE deleted = 0")
        .fetch_one(&state.db)
        .await?;
    let rows = sqlx::query_as::<_, AiModel>(
        "SELECT m.*, c.platform AS platform FROM afv_ai_model m
         LEFT JOIN afv_api_config c ON c.id = m.api_config_id
         WHERE m.deleted = 0 ORDER BY m.model_type ASC, m.sort ASC LIMIT ? OFFSET ?",
    )
    .bind(page_size)
    .bind(q.offset())
    .fetch_all(&state.db)
    .await?;
    ok(PageResult::new(rows, total, page_no, page_size))
}

async fn model_presets() -> ApiResult<Vec<ModelPreset>> {
    ok(GROK_MODELS
        .iter()
        .map(|(code, name, model_type, vision, reasoning)| ModelPreset {
            code: code.to_string(),
            name: name.to_string(),
            model_type: *model_type,
            description: format!("Grok 平台模型：{name}"),
            support_vision: *vision,
            support_reasoning: *reasoning,
            context_window: Some(131072),
            config: None,
        })
        .collect())
}

async fn preset_config() -> ApiResult<serde_json::Value> {
    ok(json!({
        "reasoningEffortLevels": ["low", "medium", "high"],
        "aspectRatios": ["16:9", "9:16", "1:1", "4:3", "3:4"],
        "resolutions": ["720p", "1080p"],
        "durations": [6, 10]
    }))
}

async fn test_text_connectivity(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<IdQuery>,
) -> ApiResult<ModelConnectivityResult> {
    let model = load_model(&state, q.id).await?;
    let api_config_id = model
        .api_config_id
        .ok_or_else(|| AppError::bad_request("模型未绑定 API 配置"))?;
    let config = load_api_config(&state, api_config_id).await?;
    let platform = resolve_platform(&config, model.model_protocol.as_deref());
    let provider_cfg = ProviderConfig::from_api_config(&config, Some(&platform));
    let provider = state.registry.chat(&platform)?;

    let started = std::time::Instant::now();
    let response = provider
        .chat(
            &provider_cfg,
            fusion_ai::types::ChatRequest {
                model: model.code.clone(),
                messages: vec![fusion_ai::types::ChatMessage::user("请只回复：连接正常")],
                temperature: Some(0.1),
                max_tokens: Some(64),
                tools: Vec::new(),
                tool_choice: None,
            },
        )
        .await?;

    ok(ModelConnectivityResult {
        model_id: model.id,
        model_name: model.name.unwrap_or(model.code),
        response_text: response.content,
        duration_ms: started.elapsed().as_millis() as i64,
        tested_at: now(),
    })
}

/// 供生成流程复用：按模型解析出（供应商, 配置）。
pub async fn resolve_provider_config(
    state: &AppState,
    model: &AiModel,
) -> Result<(String, ProviderConfig), AppError> {
    let api_config_id = model
        .api_config_id
        .ok_or_else(|| AppError::business("模型未绑定 API 配置"))?;
    let config = load_api_config(state, api_config_id).await?;
    let platform = resolve_platform(&config, model.model_protocol.as_deref());
    let cfg = ProviderConfig::from_api_config(&config, Some(&platform));
    Ok((platform, cfg))
}
