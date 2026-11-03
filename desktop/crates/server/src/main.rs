//! 独立运行入口（开发调试 / 无 GUI 环境）。桌面壳由 `src-tauri` 负责。

use fusion_core::config::AppConfig;
use fusion_core::paths::AppPaths;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let web_dir = std::env::var("FUSION_WEB_DIR").ok().map(std::path::PathBuf::from);
    let paths = AppPaths::resolve(web_dir)?;
    let config = AppConfig::load(&paths.root)?;
    let port = std::env::var("FUSION_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(config.port);

    let (actual, server) = fusion_server::start(paths.clone(), config, Some(port)).await?;
    println!("融光服务已启动: http://127.0.0.1:{actual}");
    println!("数据目录: {}", paths.root.display());

    // 保持进程存活，等待 Ctrl+C 或服务任务提前结束。
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("收到退出信号，正在停止服务");
        }
        _ = server => {
            tracing::warn!("服务任务已结束");
        }
    }
    Ok(())
}
