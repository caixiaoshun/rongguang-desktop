//! 前端可调用的命令。桌面版把「路径类操作」收拢到这里，避免网页侧直接拿到 fs 权限。

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::BootState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub port: u16,
    pub platform: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendReady {
    pub port: u16,
    pub origin: String,
}

/// 页面启动时调用：返回本地服务端口，前端据此拼出绝对地址（下载、预览等场景）。
#[tauri::command]
pub async fn backend_ready(state: State<'_, BootState>) -> Result<BackendReady, String> {
    let port = state.wait().await;
    Ok(BackendReady {
        port,
        origin: format!("http://127.0.0.1:{port}"),
    })
}

#[tauri::command]
pub async fn app_info(app: AppHandle, state: State<'_, BootState>) -> Result<AppInfo, String> {
    let port = state.wait().await;
    let data_dir = app
        .path()
        .app_data_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        data_dir,
        port,
        platform: std::env::consts::OS.to_string(),
    })
}

/// 在系统文件管理器中打开数据目录，方便用户备份 SQLite 与媒体文件。
#[tauri::command]
pub fn open_data_dir(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    tauri_plugin_opener::open_path(dir, None::<&str>).map_err(|e| e.to_string())
}

/// 用系统默认浏览器打开外链（登录第三方平台、文档等）。
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("仅支持 http/https 链接".into());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}
