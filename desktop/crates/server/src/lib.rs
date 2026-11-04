//! 融光桌面端服务层：暴露与网页版一致的 REST 契约，并托管静态前端。

pub mod auth;
pub mod router;
pub mod routes;
pub mod state;
pub mod support;

pub use state::AppState;

use std::sync::Arc;

use fusion_core::config::AppConfig;
use fusion_core::paths::AppPaths;

/// 初始化并启动内嵌服务，返回实际监听端口与后台服务任务。
///
/// 调用方（独立入口 / Tauri 桌面壳）自行决定把任务挂到哪个 runtime 上等待，
/// 因此这里不调用 `tokio::spawn`，避免 runtime 归属不清导致的 panic。
pub async fn start(
    paths: AppPaths,
    config: AppConfig,
    port: Option<u16>,
) -> anyhow::Result<(u16, tokio::task::JoinHandle<()>)> {
    let state = Arc::new(AppState::new(paths, config).await?);
    // 桌面版没有登录/初始化流程，启动即准备好本地账号与默认供应商。
    routes::system::ensure_local_bootstrap(&state).await?;
    routes::assistant::recover_interrupted(&state).await?;
    routes::generation::recover_interrupted(&state).await?;

    // ffmpeg 自检：缺失不阻塞启动，但要在日志里说清楚，否则用户只在合成时才发现。
    {
        let bin = support::ffmpeg::ffmpeg_bin(&state.config.read().ffmpeg_path);
        match support::ffmpeg::probe_version(&bin).await {
            Some(ver) => tracing::info!("ffmpeg 就绪: {ver}"),
            None => tracing::warn!(
                "未找到可用的 ffmpeg（{bin}）：视频合成与封面抽帧将不可用。\
                 请把 ffmpeg 放入程序目录 bin/，或在设置中指定路径。"
            ),
        }
    }

    let app = router::build(state.clone());

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port.unwrap_or(0))).await?;
    let addr = listener.local_addr()?;
    tracing::info!("融光服务已启动: http://{addr}");

    let _ = std::fs::write(state.paths.root.join("runtime-port"), addr.port().to_string());

    // 立刻 spawn 到当前 runtime；返回句柄供调用方 keep-alive 或 abort。
    let handle = tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!("服务异常退出: {err}");
        }
    });
    Ok((addr.port(), handle))
}
