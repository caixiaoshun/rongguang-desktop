//! 本地目录布局。桌面端所有可变数据都落在用户数据目录下，便于备份与卸载。

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub db_file: PathBuf,
    pub media_dir: PathBuf,
    pub upload_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub web_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve(web_dir: Option<PathBuf>) -> AppResult<Self> {
        let root = match std::env::var("FUSION_DATA_DIR") {
            Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
            _ => default_data_dir()?,
        };

        let db_file = root.join("fusion.db");
        let media_dir = root.join("media");
        let upload_dir = root.join("uploads");
        let workspace_dir = root.join("workspace");
        let logs_dir = root.join("logs");

        for dir in [&root, &media_dir, &upload_dir, &workspace_dir, &logs_dir] {
            std::fs::create_dir_all(dir).map_err(|e| {
                AppError::internal(format!("创建目录失败 {}: {e}", dir.display()))
            })?;
        }
        for sub in ["images", "videos", "audios", "covers", "temp"] {
            let _ = std::fs::create_dir_all(media_dir.join(sub));
        }

        let web_dir = web_dir.unwrap_or_else(default_web_dir).canonicalize().unwrap_or_else(|_| default_web_dir());

        Ok(Self {
            root,
            db_file,
            media_dir,
            upload_dir,
            workspace_dir,
            web_dir,
            logs_dir,
        })
    }

    /// 把可能是绝对路径或 URL 的媒体地址还原为本地文件路径。
    pub fn resolve_media_file(&self, url: &str) -> Option<PathBuf> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(rest) = trimmed.strip_prefix("/media/") {
            return Some(self.media_dir.join(rest));
        }
        let path = Path::new(trimmed);
        if path.is_absolute() {
            return Some(path.to_path_buf());
        }
        None
    }
}

fn default_data_dir() -> AppResult<PathBuf> {
    if cfg!(target_os = "windows") {
        let base = std::env::var("APPDATA").map_err(|_| {
            AppError::internal("无法定位 APPDATA，请设置 FUSION_DATA_DIR")
        })?;
        Ok(PathBuf::from(base).join("RongGuang"))
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME")
            .map_err(|_| AppError::internal("无法定位 HOME，请设置 FUSION_DATA_DIR"))?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("RongGuang"))
    } else {
        let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{home}/.local/share")
        });
        Ok(PathBuf::from(base).join("rongguang"))
    }
}

fn default_web_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("FUSION_WEB_DIR") {
        return PathBuf::from(dir);
    }

    // 开发态：源码树里的前端导出目录（desktop/crates/core → 仓库根）。
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent() // crates
        .and_then(|p| p.parent()) // desktop
        .and_then(|p| p.parent()); // 仓库根
    if let Some(root) = repo_root {
        let dev = root.join("web").join("out");
        if dev.exists() {
            return dev;
        }
    }

    // 发布态：可执行文件同级 web/ 或 resources/web。
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [dir.join("web"), dir.join("resources").join("web")] {
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }

    repo_root
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("web")
        .join("out")
}
