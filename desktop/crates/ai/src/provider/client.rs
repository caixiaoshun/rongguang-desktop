//! 共享 HTTP 客户端构造：支持 SOCKS5/HTTP 代理。
//!
//! ## 为什么要按配置缓存
//!
//! `reqwest::Client` 内部持有连接池与 TLS 会话缓存。视频任务轮询是每 5 秒一次、
//! 一次任务十几轮，如果每次都 `Client::builder().build()`，就等于每轮重新做一遍
//! DNS + TCP + TLS 握手。实测单次建连在抖动链路上要 1–21 秒，直接把轮询节奏拖垮。
//! 这里按「代理配置」做进程内缓存，同一配置全程复用同一个连接池。

use reqwest::Client;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use fusion_core::error::{AppError, AppResult};

use crate::provider::ProviderConfig;

fn client_cache() -> &'static Mutex<HashMap<String, Client>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Client>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 缓存键只取影响连接建立的字段，不含 api_key / base_url。
fn cache_key(cfg: &ProviderConfig) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        cfg.proxy_type.as_deref().unwrap_or("none"),
        cfg.proxy_host.as_deref().unwrap_or(""),
        cfg.proxy_port.unwrap_or(0),
        cfg.proxy_username.as_deref().unwrap_or(""),
        cfg.proxy_password.as_deref().unwrap_or(""),
    )
}

pub fn build_client(cfg: &ProviderConfig) -> AppResult<Client> {
    let key = cache_key(cfg);
    if let Ok(guard) = client_cache().lock() {
        if let Some(client) = guard.get(&key) {
            return Ok(client.clone());
        }
    }

    let mut builder = Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(120))
        .pool_max_idle_per_host(16)
        .tcp_keepalive(Duration::from_secs(60))
        .user_agent("rongguang-desktop/0.1");

    let proxy_type = cfg.proxy_type.as_deref().unwrap_or("none");
    if proxy_type != "none" {
        if let (Some(host), Some(port)) = (cfg.proxy_host.as_deref(), cfg.proxy_port) {
            let scheme = if proxy_type == "socks5" {
                "socks5h"
            } else {
                "http"
            };
            let auth = match (cfg.proxy_username.as_deref(), cfg.proxy_password.as_deref()) {
                (Some(u), Some(p)) if !u.is_empty() => format!("{u}:{p}@"),
                _ => String::new(),
            };
            let url = format!("{scheme}://{auth}{host}:{port}");
            let proxy = reqwest::Proxy::all(&url)
                .map_err(|e| AppError::internal(format!("代理配置无效: {e}")))?;
            builder = builder.proxy(proxy);
        }
    }

    let client = builder
        .build()
        .map_err(|e| AppError::internal(format!("HTTP 客户端创建失败: {e}")))?;

    if let Ok(mut guard) = client_cache().lock() {
        guard.insert(key, client.clone());
    }
    Ok(client)
}

/// 把上游错误体压缩成一行可读文本。
pub fn describe_error(status: u16, body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return format!("上游返回 HTTP {status}");
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(msg) = value
            .get("error")
            .and_then(|e| {
                e.get("message")
                    .and_then(|m| m.as_str())
                    .or_else(|| e.as_str())
            })
            .or_else(|| value.get("message").and_then(|m| m.as_str()))
        {
            return format!("{msg}（HTTP {status}）");
        }
    }
    let snippet: String = trimmed.chars().take(200).collect();
    format!("{snippet}（HTTP {status}）")
}
