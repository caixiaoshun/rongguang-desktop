//! 媒体落地：把上游产物（远端 URL / base64）保存到本地目录，返回 `/media/...` 相对地址。
//!
//! 这一步是硬要求：grok2api 的媒体目录有 1 GiB 上限并按最旧优先清理，
//! 只存远端 URL 会导致历史作品静默失效。
//!
//! ## 为什么下载要并发分段
//!
//! 实测（2026-11-02，沙箱 → 纽约 VPS，RTT 210ms、丢包 20%）：
//! 单个 TCP 流下载 5.5MB 视频只有 **13.9 KB/s，耗时 403 秒**；
//! 同样内容用 8 路 Range 并发，聚合 **85 KB/s，24 秒**（提升 6 倍）。
//!
//! 高丢包链路下单个流的拥塞窗口涨不起来，吞吐被 RTT 与丢包率死死压住。
//! 这是链路的物理属性，改不了，只能靠多流叠加绕过去——这也是下载器普遍分片的根本原因。
//! 因此大文件（≥1.5MB）且上游支持 Range 时，一律走并发分段。

use std::path::{Path, PathBuf};

use base64::Engine;
use futures_util::future::join_all;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use fusion_core::error::{AppError, AppResult};

/// 超过这个体积才值得分片：小文件起并发连接的开销大于收益。
const PARALLEL_MIN_SIZE: u64 = 1_500_000;
/// 并发流数。实测 8 路已能拿到 6 倍提升；再高会压垮单核小机器（grok2api 宿主仅 1 核）。
const PARALLEL_STREAMS: u64 = 8;
/// 单片最小 256KB，避免文件刚过阈值时被切成碎片。
const PARALLEL_CHUNK_MIN: u64 = 256 * 1024;

#[derive(Debug, Clone)]
pub struct MediaStore {
    root: PathBuf,
}

impl MediaStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        for sub in ["images", "videos", "audios", "covers", "temp"] {
            let _ = std::fs::create_dir_all(root.join(sub));
        }
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 保存二进制并返回 `/media/<kind>/<file>`。
    pub fn save_bytes(&self, kind: &str, ext: &str, bytes: &[u8]) -> AppResult<String> {
        let kind = normalize_kind(kind);
        let ext = sanitize_ext(ext);
        let name = format!("{}.{}", Uuid::new_v4().simple(), ext);
        let dir = self.root.join(kind);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(&name);
        std::fs::write(&path, bytes)?;
        Ok(format!("/media/{kind}/{name}"))
    }

    /// 保存远端 URL 或 data URL，返回本地相对地址。`auth_token` 用于需要鉴权的上游。
    pub async fn persist(
        &self,
        client: &reqwest::Client,
        url: &str,
        kind: &str,
        auth_token: Option<&str>,
    ) -> AppResult<String> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(AppError::bad_request("媒体地址为空"));
        }

        // 已经是本地媒体地址，直接返回。
        if trimmed.starts_with("/media/") {
            return Ok(trimmed.to_string());
        }

        if let Some(rest) = trimmed.strip_prefix("data:") {
            let (meta, data) = rest
                .split_once(',')
                .ok_or_else(|| AppError::bad_request("data URL 格式无效"))?;
            let mime = meta.split(';').next().unwrap_or("image/png");
            let ext = ext_from_mime(mime);
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| AppError::bad_request(format!("base64 解码失败: {e}")))?;
            return self.save_bytes(kind, ext, &bytes);
        }

        // 大文件下载对网络抖动敏感，失败（含截断）时重试 3 次。
        let mut last_err = None;
        for attempt in 0..3 {
            match self.download_once(client, trimmed, kind, auth_token).await {
                Ok(url) => return Ok(url),
                Err(err) => {
                    tracing::warn!("媒体下载第 {} 次失败: {err}", attempt + 1);
                    last_err = Some(err);
                    if attempt < 2 {
                        tokio::time::sleep(std::time::Duration::from_secs(1 + attempt as u64))
                            .await;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| AppError::Http("下载媒体失败".into())))
    }

    async fn download_once(
        &self,
        client: &reqwest::Client,
        trimmed: &str,
        kind: &str,
        auth_token: Option<&str>,
    ) -> AppResult<String> {
        // 先探一下体积与 Range 支持，决定单流还是并发分段。
        if let Some((total, mime)) = self.probe_size(client, trimmed, auth_token).await {
            if total >= PARALLEL_MIN_SIZE {
                tracing::info!(
                    "媒体下载改用 {} 路并发分段：{:.2} MB",
                    PARALLEL_STREAMS,
                    total as f64 / 1024.0 / 1024.0
                );
                match self
                    .download_parallel(client, trimmed, kind, auth_token, total, &mime)
                    .await
                {
                    Ok(url) => return Ok(url),
                    // 分段失败（上游拒绝 Range、代理不支持等）不应让整个任务失败，
                    // 退回单流再试一次。
                    Err(err) => tracing::warn!("并发分段下载失败，退回单流: {err}"),
                }
            }
        }
        self.download_single(client, trimmed, kind, auth_token)
            .await
    }

    /// 用 `Range: bytes=0-0` 探出总长度与 MIME。
    /// 返回 `None` 表示探测失败或不支持 Range —— 调用方退回单流即可，不算错误。
    ///
    /// MIME 必须在这里一起取回：`/v1/media/videos/vid_xxx` 这类地址没有扩展名，
    /// 只能靠 Content-Type 决定后缀，否则会存成 `.bin`（内容仍是合法 MP4，但不好认）。
    async fn probe_size(
        &self,
        client: &reqwest::Client,
        url: &str,
        auth_token: Option<&str>,
    ) -> Option<(u64, String)> {
        let mut request = client.get(url).header(reqwest::header::RANGE, "bytes=0-0");
        if let Some(token) = auth_token {
            if !token.is_empty() {
                request = request.bearer_auth(token);
            }
        }
        let resp = request.send().await.ok()?;
        if resp.status().as_u16() != 206 {
            return None;
        }
        let mime = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let ranges = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())?;
        // 形如 `bytes 0-0/5756930`
        let total = ranges.rsplit('/').next()?.trim().parse::<u64>().ok()?;
        Some((total, mime))
    }

    /// 并发分段下载并按序拼接。任一分片重试后仍失败则整体失败。
    async fn download_parallel(
        &self,
        client: &reqwest::Client,
        url: &str,
        kind: &str,
        auth_token: Option<&str>,
        total: u64,
        mime: &str,
    ) -> AppResult<String> {
        let chunk = std::cmp::max(total / PARALLEL_STREAMS + 1, PARALLEL_CHUNK_MIN);
        let mut ranges = Vec::new();
        let mut start = 0u64;
        while start < total {
            let end = std::cmp::min(start + chunk - 1, total - 1);
            ranges.push((start, end));
            start = end + 1;
        }

        let jobs = ranges.iter().map(|(s, e)| {
            let client = client.clone();
            let url = url.to_string();
            let token = auth_token.map(|t| t.to_string());
            let (s, e) = (*s, *e);
            async move { fetch_range(&client, &url, token.as_deref(), s, e).await }
        });

        let parts = join_all(jobs).await;
        let mut buf: Vec<u8> = Vec::with_capacity(total as usize);
        for (idx, part) in parts.into_iter().enumerate() {
            match part {
                Ok(bytes) => buf.extend_from_slice(&bytes),
                Err(err) => {
                    return Err(AppError::Http(format!(
                        "第 {} 个分片下载失败: {err}",
                        idx + 1
                    )))
                }
            }
        }

        if buf.len() as u64 != total {
            return Err(AppError::Http(format!(
                "分段下载字节数不符：期望 {total}，实际 {}",
                buf.len()
            )));
        }

        let ext = guess_ext(url, mime);
        self.save_bytes(kind, &ext, &buf)
    }

    async fn download_single(
        &self,
        client: &reqwest::Client,
        trimmed: &str,
        kind: &str,
        auth_token: Option<&str>,
    ) -> AppResult<String> {
        let mut request = client.get(trimmed);
        if let Some(token) = auth_token {
            if !token.is_empty() {
                request = request.bearer_auth(token);
            }
        }
        let resp = request
            .send()
            .await
            .map_err(|e| AppError::Http(format!("下载媒体失败: {e}")))?;

        if !resp.status().is_success() {
            return Err(AppError::Http(format!(
                "下载媒体失败: HTTP {}",
                resp.status().as_u16()
            )));
        }

        let mime = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let ext = guess_ext(trimmed, &mime);

        let content_length = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::Http(format!("读取媒体内容失败: {e}")))?;

        // 大文件在网络抖动时可能被静默截断（连接提前关闭但未报错），
        // 用 Content-Length 做一次完整性校验，避免把半截视频当成成品存档。
        if let Some(expected) = content_length {
            if (bytes.len() as u64) != expected {
                return Err(AppError::Http(format!(
                    "媒体下载不完整：期望 {expected} 字节，实际 {} 字节",
                    bytes.len()
                )));
            }
        }
        if bytes.is_empty() {
            return Err(AppError::Http("媒体内容为空".into()));
        }

        self.save_bytes(kind, &ext, &bytes)
    }

    /// 按内容哈希去重保存，适用于用户上传。
    pub fn save_dedup(&self, kind: &str, ext: &str, bytes: &[u8]) -> AppResult<String> {
        let kind = normalize_kind(kind);
        let ext = sanitize_ext(ext);
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let hash = hex::encode(hasher.finalize());
        let name = format!("{}.{}", &hash[..32], ext);
        let dir = self.root.join(kind);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(&name);
        if !path.exists() {
            std::fs::write(&path, bytes)?;
        }
        Ok(format!("/media/{kind}/{name}"))
    }

    pub fn local_path(&self, public_url: &str) -> Option<PathBuf> {
        public_url
            .strip_prefix("/media/")
            .map(|rest| self.root.join(rest))
    }
}

fn normalize_kind(kind: &str) -> &'static str {
    match kind {
        "videos" | "video" => "videos",
        "audios" | "audio" => "audios",
        "covers" | "cover" => "covers",
        "temp" => "temp",
        _ => "images",
    }
}

fn sanitize_ext(ext: &str) -> String {
    let cleaned: String = ext
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    if cleaned.is_empty() {
        "bin".into()
    } else {
        cleaned.chars().take(6).collect()
    }
}

fn is_known_ext(ext: &str) -> bool {
    matches!(
        ext.to_lowercase().as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "gif"
            | "bmp"
            | "mp4"
            | "webm"
            | "mov"
            | "mp3"
            | "wav"
            | "m4a"
            | "ogg"
    )
}

/// 先看 URL 后缀，再退回 MIME 推断。
fn guess_ext(url: &str, mime: &str) -> String {
    Path::new(url)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| is_known_ext(e))
        .map(|e| e.to_string())
        .unwrap_or_else(|| ext_from_mime(mime).to_string())
}

/// 取一个字节区间。分片失败重试 3 次——单段很小，重试成本远低于整文件重下。
async fn fetch_range(
    client: &reqwest::Client,
    url: &str,
    auth_token: Option<&str>,
    start: u64,
    end: u64,
) -> AppResult<Vec<u8>> {
    let want = end - start + 1;
    let mut last_err = None;

    for attempt in 0..3 {
        let mut request = client
            .get(url)
            .header(reqwest::header::RANGE, format!("bytes={start}-{end}"));
        if let Some(token) = auth_token {
            if !token.is_empty() {
                request = request.bearer_auth(token);
            }
        }

        match request.send().await {
            Ok(resp) if resp.status().as_u16() == 206 || resp.status().is_success() => {
                match resp.bytes().await {
                    Ok(bytes) if bytes.len() as u64 == want => return Ok(bytes.to_vec()),
                    Ok(bytes) => {
                        last_err = Some(AppError::Http(format!(
                            "分片 {start}-{end} 长度不符：期望 {want}，实际 {}",
                            bytes.len()
                        )));
                    }
                    Err(e) => last_err = Some(AppError::Http(format!("读取分片失败: {e}"))),
                }
            }
            Ok(resp) => {
                last_err = Some(AppError::Http(format!(
                    "分片 {start}-{end} 返回 HTTP {}",
                    resp.status().as_u16()
                )))
            }
            Err(e) => last_err = Some(AppError::Http(format!("请求分片失败: {e}"))),
        }

        if attempt < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(400 * (attempt + 1) as u64)).await;
        }
    }

    Err(last_err.unwrap_or_else(|| AppError::Http("分片下载失败".into())))
}

pub fn ext_from_mime(mime: &str) -> &'static str {
    let base = mime.split(';').next().unwrap_or("").trim().to_lowercase();
    match base.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/mp4" | "audio/m4a" => "m4a",
        "audio/ogg" => "ogg",
        _ => "bin",
    }
}

pub fn kind_from_mime(mime: &str) -> &'static str {
    let base = mime.split(';').next().unwrap_or("").trim().to_lowercase();
    if base.starts_with("video/") {
        "videos"
    } else if base.starts_with("audio/") {
        "audios"
    } else {
        "images"
    }
}
