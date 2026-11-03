//! FFmpeg 调用封装：视频拼接、封面抽帧、时长探测。

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

use fusion_core::error::{AppError, AppResult};

/// 解析 ffmpeg 可执行文件：配置值 > 随包 `bin/` > 可执行文件同级 > PATH。
pub fn ffmpeg_bin(configured: &str) -> String {
    ff_tool(configured, "ffmpeg")
}

/// 解析 ffprobe（与 ffmpeg 同目录随包）。
pub fn ffprobe_bin(configured_ffmpeg: &str) -> String {
    // 用户手动指定了 ffmpeg 时，优先认定同目录下的 ffprobe。
    let cfg = if configured_ffmpeg.trim().is_empty() {
        String::new()
    } else {
        let p = Path::new(configured_ffmpeg.trim());
        p.with_file_name(exe_name("ffprobe"))
            .to_string_lossy()
            .to_string()
    };
    ff_tool(&cfg, "ffprobe")
}

fn exe_name(base: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn ff_tool(configured: &str, base: &str) -> String {
    if !configured.trim().is_empty() {
        return configured.trim().to_string();
    }
    let name = exe_name(base);
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // 发布态：Tauri 把随包资源放在 resources/bin；开发态：target/debug/bin。
            for candidate in [
                dir.join("bin").join(&name),
                dir.join(&name),
                dir.join("resources").join("bin").join(&name),
                dir.join("..").join("Resources").join("bin").join(&name),
            ] {
                if candidate.exists() {
                    return candidate.to_string_lossy().to_string();
                }
            }
        }
    }
    name
}

/// 探测 ffmpeg 是否可用，返回版本行首行，用于启动自检。
pub async fn probe_version(bin: &str) -> Option<String> {
    // 随包的 ffmpeg 若损坏/缺少动态库，可能出现长时间不返回的情况；
    // 这属于「非致命」探测，绝不能把整个应用启动拖住。
    let mut command = Command::new(bin);
    command.kill_on_drop(true);
    // GUI 程序探测 ffmpeg 时不弹出控制台；超时后同时回收子进程。
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        command
            .arg("-version")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().next().map(|l| l.trim().to_string())
}

/// 用 concat 协议拼接视频。优先流拷贝，失败时退回重新编码。
pub async fn concat_videos(ffmpeg: &str, inputs: &[PathBuf], output: &Path) -> AppResult<()> {
    if inputs.is_empty() {
        return Err(AppError::bad_request("没有可拼接的视频片段"));
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let list_file = std::env::temp_dir().join(format!("rg-concat-{}.txt", uuid::Uuid::new_v4().simple()));
    let content = inputs
        .iter()
        .map(|path| format!("file '{}'", path.to_string_lossy().replace('\\', "/").replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&list_file, content)?;

    let copy_result = run(
        ffmpeg,
        &[
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            &list_file.to_string_lossy(),
            "-c",
            "copy",
            &output.to_string_lossy(),
        ],
    )
    .await;

    let ok = match copy_result {
        Ok(()) => output.exists(),
        Err(_) => false,
    };

    if !ok {
        let _ = run(
            ffmpeg,
            &[
                "-y",
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
                &list_file.to_string_lossy(),
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "23",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                &output.to_string_lossy(),
            ],
        )
        .await?;
    }

    let _ = std::fs::remove_file(&list_file);
    if !output.exists() {
        return Err(AppError::internal("视频拼接未生成输出文件"));
    }
    Ok(())
}

/// 抽取首帧作为封面。
pub async fn extract_cover(ffmpeg: &str, input: &Path, output: &Path) -> AppResult<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    run(
        ffmpeg,
        &[
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-frames:v",
            "1",
            "-q:v",
            "2",
            &output.to_string_lossy(),
        ],
    )
    .await
}

pub async fn probe_duration(ffprobe: &str, input: &Path) -> Option<f64> {
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            &input.to_string_lossy(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .ok()
}

async fn run(bin: &str, args: &[&str]) -> AppResult<()> {
    let output = Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| AppError::internal(format!("调用 ffmpeg 失败: {e}（路径: {bin}）")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.chars().rev().take(600).collect::<String>().chars().rev().collect();
        return Err(AppError::internal(format!("ffmpeg 执行失败: {tail}")));
    }
    Ok(())
}
