//! 检查更新：远端版本检测 / 安装包下载 / 拉起安装器。
//!
//! 设计取舍（重要）：
//! - 检测源优先 Gitee，失败自动回退 GitHub。国内访问 Gitee 稳定得多，GitHub API 常常超时。
//! - 只做「提示 → 下载 → 拉起系统安装器」，**不静默替换自身二进制**。
//!   两个平台都没有代码签名，静默替换在 macOS 上会被 Gatekeeper 拦、在 Windows 上会和
//!   正在运行的文件锁冲突，反而把应用搞成打不开的状态。
//! - 下载地址做白名单校验，避免这个接口被当成任意文件下载器。

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::state::AppState;
use fusion_core::error::AppError;

use crate::support::{ok, ApiResult};

/// 发布仓库。两个仓库的内容一致，只是托管平台不同。
const GITEE_REPO: &str = "caixiaoshun/ai-fusion-video";
const GITHUB_REPO: &str = "caixiaoshun/rongguang-desktop";

/// 检测结果缓存时间。启动时查一次，用户手动点「检查更新」带 force 跳过缓存。
const CACHE_TTL: Duration = Duration::from_secs(600);
const HTTP_TIMEOUT: Duration = Duration::from_secs(8);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/system/update/download", post(download))
        .route("/api/system/update/progress", get(progress))
        .route("/api/system/update/install", post(install))
}

// ============ 版本信息 ============

/// 字段与前端 `SystemVersionInfo` 一一对应，改名要两边一起改。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub current_version: String,
    pub current_version_display: String,
    pub latest_version: Option<String>,
    pub latest_version_display: String,
    pub latest_release_version: Option<String>,
    pub latest_release_version_display: String,
    pub latest_release_url: String,
    pub latest_release_published_at: Option<String>,
    pub latest_release_docker_ready: bool,
    pub update_available: bool,
    pub comparison_enabled: bool,
    pub development_build: bool,
    pub build_profile: String,
    /// behind / same / ahead / incomparable
    pub version_relation: String,
    pub release_url: String,
    pub tag_url: String,
    pub published_at: Option<String>,
    pub checked_at: String,
    /// gitee / github / none
    pub source: String,
    pub check_succeeded: bool,
    pub message: Option<String>,
    /// 当前平台对应的安装包直链，供前端一键下载
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: Option<u64>,
}

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 去掉 `v` 前缀与 `-desktop` 之类的后缀，只留数字段。
fn parse_version(raw: &str) -> Option<Vec<u64>> {
    let trimmed = raw.trim().trim_start_matches('v');
    let core = trimmed.split(['-', '+']).next()?.trim();
    if core.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for seg in core.split('.') {
        parts.push(seg.parse::<u64>().ok()?);
    }
    Some(parts)
}

fn compare_versions(a: &[u64], b: &[u64]) -> i32 {
    for i in 0..a.len().max(b.len()) {
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        if x != y {
            return if x > y { 1 } else { -1 };
        }
    }
    0
}

// ============ 远端拉取 ============

#[derive(Clone, Debug)]
struct RemoteRelease {
    tag: String,
    html_url: String,
    published_at: Option<String>,
    body: Option<String>,
    assets: Vec<RemoteAsset>,
    source: &'static str,
}

#[derive(Clone, Debug)]
struct RemoteAsset {
    name: String,
    url: String,
    /// GitHub 的 assets 带 `size`，Gitee 的不带（只给 name + browser_download_url），
    /// 所以 Gitee 侧恒为 0。前端只在有值时显示体积，0 就不显示，不需要为它多发一次请求。
    size: u64,
}

fn client() -> &'static reqwest::Client {
    static C: LazyLock<reqwest::Client> = LazyLock::new(|| {
        reqwest::Client::builder()
            // GitHub API 不带 UA 会直接 403
            .user_agent(concat!("rongguang-desktop/", env!("CARGO_PKG_VERSION")))
            .timeout(HTTP_TIMEOUT)
            .build()
            .unwrap_or_default()
    });
    &C
}

/// Gitee 的 releases 返回数组，但**顺序不可靠**：改过某个 release 的说明（PATCH）之后，
/// 它会跑到列表前面去。所以不能取第一条，必须按 tag 里的版本号挑最大的那个。
async fn fetch_gitee() -> Option<RemoteRelease> {
    let url = format!(
        "https://gitee.com/api/v5/repos/{GITEE_REPO}/releases?per_page=20&page=1"
    );
    let resp = client().get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let list: serde_json::Value = resp.json().await.ok()?;
    let arr = list.as_array()?;

    // 预发布（prerelease）跳过：下架旧版本时会把它标成 prerelease
    let latest = arr
        .iter()
        .filter(|r| !r.get("prerelease").and_then(|v| v.as_bool()).unwrap_or(false))
        .filter_map(|r| {
            let tag = r.get("tag_name")?.as_str()?;
            let parsed = parse_version(tag)?;
            Some((parsed, r))
        })
        .max_by(|a, b| {
            compare_versions(&a.0, &b.0).cmp(&0)
        })
        .map(|(_, r)| r)?;

    Some(RemoteRelease {
        tag: latest.get("tag_name")?.as_str()?.to_string(),
        html_url: latest
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        published_at: latest
            .get("created_at")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        body: latest.get("body").and_then(|v| v.as_str()).map(str::to_string),
        assets: parse_assets(latest),
        source: "gitee",
    })
}

async fn fetch_github() -> Option<RemoteRelease> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let resp = client().get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;

    Some(RemoteRelease {
        tag: v.get("tag_name")?.as_str()?.to_string(),
        html_url: v
            .get("html_url")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        published_at: v
            .get("published_at")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        body: v.get("body").and_then(|x| x.as_str()).map(str::to_string),
        assets: parse_assets(&v),
        source: "github",
    })
}

fn parse_assets(v: &serde_json::Value) -> Vec<RemoteAsset> {
    v.get("assets")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    Some(RemoteAsset {
                        name: a.get("name")?.as_str()?.to_string(),
                        url: a.get("browser_download_url")?.as_str()?.to_string(),
                        size: a.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 按平台挑安装包。每个平台只发一个包，所以后缀匹配足够。
fn pick_asset(assets: &[RemoteAsset]) -> Option<&RemoteAsset> {
    let suffixes: &[&str] = if cfg!(windows) {
        &["x64-setup.exe", ".exe"]
    } else if cfg!(target_os = "macos") {
        &[".dmg"]
    } else {
        &[".deb"]
    };
    for s in suffixes {
        if let Some(hit) = assets.iter().find(|a| a.name.ends_with(s)) {
            return Some(hit);
        }
    }
    None
}

// ============ 检测入口 ============

static CACHE: LazyLock<Mutex<Option<(Instant, VersionInfo)>>> = LazyLock::new(|| Mutex::new(None));

fn empty_info(current: &str, message: Option<String>) -> VersionInfo {
    VersionInfo {
        current_version: current.to_string(),
        current_version_display: current.to_string(),
        latest_version: None,
        latest_version_display: String::new(),
        latest_release_version: None,
        latest_release_version_display: String::new(),
        latest_release_url: String::new(),
        latest_release_published_at: None,
        latest_release_docker_ready: false,
        update_available: false,
        comparison_enabled: parse_version(current).is_some(),
        development_build: cfg!(debug_assertions),
        build_profile: if cfg!(debug_assertions) { "debug" } else { "release" }.to_string(),
        version_relation: "incomparable".to_string(),
        release_url: String::new(),
        tag_url: String::new(),
        published_at: None,
        checked_at: crate::support::now(),
        source: "none".to_string(),
        check_succeeded: false,
        message,
        asset_name: None,
        asset_url: None,
        asset_size: None,
    }
}

/// 检测最新版本。force=true 时跳过缓存。
pub async fn check_version(force: bool) -> VersionInfo {
    let current = current_version();

    if !force {
        if let Some((at, info)) = CACHE.lock().as_ref() {
            if at.elapsed() < CACHE_TTL {
                return info.clone();
            }
        }
    }

    // 先 Gitee 后 GitHub：任一成功即可，两个都挂才判定为检测失败。
    let remote = match fetch_gitee().await {
        Some(r) => Some(r),
        None => {
            tracing::debug!("Gitee 版本检测失败，回退 GitHub");
            fetch_github().await
        }
    };

    let Some(remote) = remote else {
        let info = empty_info(&current, Some("无法连接发布服务器，请检查网络后重试".into()));
        *CACHE.lock() = Some((Instant::now(), info.clone()));
        return info;
    };

    let latest_raw = remote.tag.trim_start_matches('v').to_string();
    let relation = match (parse_version(&current), parse_version(&latest_raw)) {
        (Some(a), Some(b)) => match compare_versions(&a, &b) {
            1 => "ahead",
            0 => "same",
            _ => "behind",
        },
        _ => "incomparable",
    };

    let asset = pick_asset(&remote.assets);
    let info = VersionInfo {
        current_version: current.clone(),
        current_version_display: current.clone(),
        latest_version: Some(latest_raw.clone()),
        latest_version_display: latest_raw.clone(),
        latest_release_version: Some(latest_raw.clone()),
        latest_release_version_display: latest_raw.clone(),
        latest_release_url: remote.html_url.clone(),
        latest_release_published_at: remote.published_at.clone(),
        latest_release_docker_ready: false,
        update_available: relation == "behind",
        comparison_enabled: relation != "incomparable",
        development_build: cfg!(debug_assertions),
        build_profile: if cfg!(debug_assertions) { "debug" } else { "release" }.to_string(),
        version_relation: relation.to_string(),
        release_url: remote.html_url.clone(),
        tag_url: remote.html_url.clone(),
        published_at: remote.published_at.clone(),
        checked_at: crate::support::now(),
        source: remote.source.to_string(),
        check_succeeded: true,
        message: remote.body.clone(),
        asset_name: asset.map(|a| a.name.clone()),
        asset_url: asset.map(|a| a.url.clone()),
        asset_size: asset.map(|a| a.size),
    };

    *CACHE.lock() = Some((Instant::now(), info.clone()));
    info
}

// ============ 下载 ============

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadReq {
    pub url: String,
    pub name: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    /// idle / downloading / done / failed
    pub status: String,
    pub received: u64,
    pub total: u64,
    pub path: Option<String>,
    pub error: Option<String>,
}

impl Default for DownloadProgress {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            received: 0,
            total: 0,
            path: None,
            error: None,
        }
    }
}

static PROGRESS: LazyLock<Mutex<DownloadProgress>> = LazyLock::new(|| Mutex::new(DownloadProgress::default()));

/// 只允许从发布平台下载，避免这个接口变成任意文件下载器。
fn allowed_host(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    matches!(
        parsed.host_str(),
        Some("gitee.com") | Some("github.com") | Some("objects.githubusercontent.com")
            | Some("release-assets.githubusercontent.com")
    )
}

fn safe_file_name(name: &str) -> Option<String> {
    let base = Path::new(name).file_name()?.to_str()?.to_string();
    if base.is_empty() || base.contains("..") {
        return None;
    }
    Some(base)
}

async fn download(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DownloadReq>,
) -> ApiResult<DownloadProgress> {
    if !allowed_host(&req.url) {
        return Err(AppError::bad_request("只允许从 Gitee / GitHub 下载安装包"));
    }

    let dir = state.paths.root.join("downloads");
    let _ = std::fs::create_dir_all(&dir);

    let fallback = req
        .url
        .rsplit('/')
        .next()
        .unwrap_or("rongguang-setup.exe")
        .to_string();
    let name = safe_file_name(req.name.as_deref().unwrap_or(&fallback))
        .unwrap_or_else(|| "rongguang-setup.exe".to_string());
    let dest = dir.join(&name);

    // 已下载过就直接复用，避免重复拉几十 MB
    if dest.exists() {
        if let Ok(meta) = std::fs::metadata(&dest) {
            if meta.len() > 1024 * 1024 {
                *PROGRESS.lock() = DownloadProgress {
                    status: "done".to_string(),
                    received: meta.len(),
                    total: meta.len(),
                    path: Some(dest.to_string_lossy().to_string()),
                    error: None,
                };
                return ok(PROGRESS.lock().clone());
            }
        }
    }

    *PROGRESS.lock() = DownloadProgress {
        status: "downloading".to_string(),
        ..Default::default()
    };

    let url = req.url.clone();
    let dest_clone = dest.clone();
    tokio::spawn(async move {
        match stream_to_file(&url, &dest_clone).await {
            Ok(total) => {
                *PROGRESS.lock() = DownloadProgress {
                    status: "done".to_string(),
                    received: total,
                    total,
                    path: Some(dest_clone.to_string_lossy().to_string()),
                    error: None,
                };
                tracing::info!("安装包下载完成: {}", dest_clone.display());
            }
            Err(err) => {
                tracing::warn!("安装包下载失败: {err}");
                *PROGRESS.lock() = DownloadProgress {
                    status: "failed".to_string(),
                    error: Some(err),
                    ..Default::default()
                };
            }
        }
    });

    ok(PROGRESS.lock().clone())
}

async fn stream_to_file(url: &str, dest: &Path) -> Result<u64, String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let resp = client()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);

    let tmp = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| format!("创建文件失败: {e}"))?;

    let mut received = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入失败: {e}"))?;
        received += chunk.len() as u64;
        {
            let mut p = PROGRESS.lock();
            p.received = received;
            p.total = total;
        }
    }
    file.flush().await.map_err(|e| format!("落盘失败: {e}"))?;
    drop(file);

    tokio::fs::rename(&tmp, dest)
        .await
        .map_err(|e| format!("重命名失败: {e}"))?;
    Ok(received)
}

async fn progress() -> ApiResult<DownloadProgress> {
    ok(PROGRESS.lock().clone())
}

// ============ 安装 ============

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReq {
    pub path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub launched: bool,
    pub message: String,
}

async fn install(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InstallReq>,
) -> ApiResult<InstallResult> {
    let path = match req.path {
        Some(p) => PathBuf::from(p),
        None => match PROGRESS.lock().path.clone() {
            Some(p) => PathBuf::from(p),
            None => {
                return Err(AppError::bad_request("还没有下载好的安装包"))
            }
        },
    };

    if !path.exists() {
        return Err(AppError::bad_request(format!("安装包不存在: {}", path.display())));
    }

    // 只允许拉起自己下载目录里的文件
    let allowed_dir = state.paths.root.join("downloads");
    if !path.starts_with(&allowed_dir) {
        return Err(AppError::bad_request("只允许安装下载目录内的安装包"));
    }

    match launch_installer(&path) {
        Ok(msg) => {
            tracing::info!("已拉起安装程序: {}", path.display());
            ok(InstallResult {
                launched: true,
                message: msg,
            })
        }
        Err(err) => {
            tracing::warn!("拉起安装程序失败: {err}");
            ok(InstallResult {
                launched: false,
                message: err,
            })
        }
    }
}

#[cfg(windows)]
fn launch_installer(path: &Path) -> Result<String, String> {
    // NSIS 安装器会自己处理「程序正在运行」，这里直接起进程即可。
    // 用 cmd /C start 是为了脱离当前进程，避免我们退出时把安装器一起带走。
    std::process::Command::new("cmd")
        .args(["/C", "start", "", &path.to_string_lossy()])
        .spawn()
        .map_err(|e| format!("启动安装程序失败: {e}"))?;
    Ok("安装程序已启动，请按提示完成安装；安装完成后重新打开应用即可。".to_string())
}

#[cfg(target_os = "macos")]
fn launch_installer(path: &Path) -> Result<String, String> {
    // dmg 交给系统挂载，用户把 App 拖进「应用程序」即完成更新。
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map_err(|e| format!("打开 dmg 失败: {e}"))?;
    Ok("已打开 dmg，把「融光」拖进「应用程序」覆盖旧版本即可。".to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn launch_installer(path: &Path) -> Result<String, String> {
    // deb 安装需要 root，无法在应用内静默完成，打开所在目录并给出命令。
    let dir = path.parent().unwrap_or(path);
    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    Ok(format!(
        "已打开下载目录。安装请执行：sudo dpkg -i {}",
        path.display()
    ))
}
