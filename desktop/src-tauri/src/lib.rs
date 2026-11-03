//! 融光桌面壳：进程内拉起本地 HTTP 服务，再用系统 WebView 指向 `127.0.0.1`。
//!
//! 这样前后端同源（`http://127.0.0.1:<port>`），前端无需改动任何请求地址，
//! 也天然规避了 `tauri://localhost` 自定义协议下的 CORS / 相对路径问题。

mod commands;
mod tray;

use std::sync::Arc;

use fusion_core::config::AppConfig;
use fusion_core::paths::AppPaths;
use tauri::Manager;
use tokio::sync::Mutex;

/// 启动门闩：页面在窗口 ready 之前就调用 `backend_ready`，需要等一会儿。
#[derive(Default)]
pub struct BootState {
    data_dir: std::sync::OnceLock<std::path::PathBuf>,
    port: Arc<Mutex<Option<Result<u16, String>>>>,
    ready: Arc<tokio::sync::Notify>,
}

impl BootState {
    pub async fn wait(&self) -> Result<u16, String> {
        loop {
            // 先注册等待者再检查端口，避免「检查完→进入 await」之间丢通知。
            let waiter = self.ready.notified();
            tokio::pin!(waiter);
            waiter.as_mut().enable();
            if let Some(result) = self.port.lock().await.clone() {
                return result;
            }
            waiter.await;
        }
    }

    pub async fn set(&self, result: Result<u16, String>) {
        *self.port.lock().await = Some(result);
        self.ready.notify_waiters();
    }
}

fn init_tracing(data_dir: &std::path::Path) {
    let logs = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let file_appender = tracing_appender::rolling::daily(&logs, "rongguang.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    // 泄漏 guard 让后台写线程活到进程结束；桌面进程本就长期驻留。
    Box::leak(Box::new(guard));

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,sqlx=warn,tao=warn,wry=warn,tauri=warn".into());

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .try_init();
}

pub fn run() {
    let builder = tauri::Builder::default()
        .on_page_load(|window, payload| {
            // 后端可能比本地加载页更早失败；页面完成后补一次错误渲染，
            // 防止过早 eval 的结果被初始页面加载覆盖。
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && payload.url().path().ends_with("/loading.html")
            {
                let handle = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(message) = handle.state::<BootState>().wait().await {
                        let ui_handle = handle.clone();
                        let _ = handle.run_on_main_thread(move || show_fatal(&ui_handle, &message));
                    }
                });
            }
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && payload.url().host_str() == Some("127.0.0.1")
                && payload.url().path() == "/dashboard/"
            {
                tracing::info!("主界面加载完成: {}", payload.url());
                let _ = window.window().set_title("融光 · AI 视频创作工作台");
            }
        })
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(BootState::default())
        .invoke_handler(tauri::generate_handler![
            commands::backend_ready,
            commands::app_info,
            commands::open_data_dir,
            commands::open_url,
        ]);

    let app = builder
        .setup(|app| {
            let handle = app.handle().clone();
            // setup 必须立即返回，让 Windows 消息循环继续处理窗口/菜单创建。
            // 不得在这里 block_on 后台任务，或 join 等待需要主线程的托盘线程。
            tauri::async_runtime::spawn(async move {
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(60),
                    boot_backend(&handle),
                )
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("本地服务启动超时（60 秒），请检查数据目录和日志")));
                match result {
                    Ok(port) => {
                        handle.state::<BootState>().set(Ok(port)).await;
                        let ui_handle = handle.clone();
                        if let Err(err) = handle.run_on_main_thread(move || {
                            if let Err(err) = open_main_window(&ui_handle, port) {
                                tracing::error!("打开主窗口失败: {err:#}");
                                show_fatal(&ui_handle, &format!("打开主窗口失败: {err:#}"));
                            }
                            // 托盘依赖系统 appindicator，Linux 精简桌面可能缺失；
                            // 失败只降级为「无托盘」，不能拖垮主窗口。
                            if let Err(err) = tray::install(&ui_handle) {
                                tracing::warn!("系统托盘不可用: {err}");
                            }
                        }) {
                            tracing::error!("调度主线程 UI 初始化失败: {err:#}");
                        }
                    }
                    Err(err) => {
                        tracing::error!("本地服务启动失败: {err:#}");
                        let message = format!("{err:#}");
                        handle.state::<BootState>().set(Err(message.clone())).await;
                        let ui_handle = handle.clone();
                        let _ = handle.run_on_main_thread(move || {
                            show_fatal(&ui_handle, &message);
                        });
                    }
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("融光桌面端初始化失败");

    app.run(|_, _| {});
}

async fn boot_backend(handle: &tauri::AppHandle) -> anyhow::Result<u16> {
    // 数据目录的**唯一事实来源**是后端 AppPaths::resolve（FUSION_DATA_DIR → 平台默认目录）。
    // 桌面壳必须跟着它走，否则日志、runtime-port 会落到 Tauri identifier 目录，
    // 与 fusion.db / media 分家，用户「打开数据目录」时看不到日志。
    let resolved = AppPaths::resolve(resolve_web_dir(handle))?;
    let root = resolved.root.clone();
    let _ = handle.state::<BootState>().data_dir.set(root.clone());
    init_tracing(&root);
    tracing::info!("数据目录: {}", root.display());
    tracing::info!("前端资源: {}", resolved.web_dir.display());
    anyhow::ensure!(
        resolved.web_dir.join("dashboard").join("index.html").is_file(),
        "前端资源缺失: {}，请重新安装融光",
        resolved.web_dir.display()
    );

    let mut config = AppConfig::load(&resolved.root)?;

    // 打包后 FFmpeg 由安装包放在 resource_dir/bin，位置随平台而变
    // （Windows 在 exe 同级、macOS 在 .app/Contents/Resources、Linux 在 /usr/lib/<id>）。
    // 与其让后端去猜，不如在启动时解析一次、直接写进配置，路径唯一且可日志追溯。
    if config.ffmpeg_path.trim().is_empty() {
        if let Some(found) = locate_bundled_ffmpeg(handle) {
            tracing::info!("随包 ffmpeg: {}", found.display());
            config.ffmpeg_path = found.to_string_lossy().to_string();
        }
    }

    let port = std::env::var("FUSION_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .filter(|p| *p > 0)
        .unwrap_or(config.port);

    let (actual, _server) = fusion_server::start(resolved, config, Some(port)).await?;
    Ok(actual)
}

/// 在打包产物里定位随包 ffmpeg。找不到返回 `None`，交给后端回落 PATH。
fn locate_bundled_ffmpeg(handle: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(res) = handle.path().resource_dir() {
        candidates.push(res.join("bin").join(name));
        candidates.push(res.join(name));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // 绿色版：bin 与主程序同级
            candidates.push(dir.join("bin").join(name));
            candidates.push(dir.join(name));
            // macOS .app 内：Contents/MacOS/x -> Contents/Resources/bin/x
            candidates.push(dir.join("..").join("Resources").join("bin").join(name));
        }
    }
    candidates.into_iter().find(|p| p.exists())
}

/// 解析前端静态资源目录。返回 `None` 时交由 core 的默认推导处理，
/// 不要在这里硬塞一个兜底路径，否则会覆盖掉 core 基于可执行文件位置的查找逻辑。
fn resolve_web_dir(handle: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("FUSION_WEB_DIR") {
        if !dir.trim().is_empty() {
            return Some(std::path::PathBuf::from(dir));
        }
    }
    if let Ok(res) = handle.path().resource_dir() {
        for candidate in [res.join("web"), res.join("_up_").join("web")] {
            if candidate.join("index.html").exists() {
                return Some(candidate);
            }
        }
    }
    None
}

fn open_main_window(handle: &tauri::AppHandle, port: u16) -> anyhow::Result<()> {
    let url = format!("http://127.0.0.1:{port}/dashboard/");
    let parsed: tauri::Url = url.parse()?;

    // 主窗口已由配置创建。不要在主线程事件回调中再创建 WebView2 窗口。
    let win = handle.get_webview_window("main")
        .ok_or_else(|| anyhow::anyhow!("主窗口不存在"))?;
    win.navigate(parsed)?;
    win.show()?;
    win.set_focus()?;
    Ok(())
}

fn show_fatal(handle: &tauri::AppHandle, message: &str) {
    if let Some(win) = handle.get_webview_window("main") {
        let _ = win.set_title("融光未能启动");
        // 在当前窗口渲染，不依赖 WebView2 对 data: / file: 导航的支持。
        let html = serde_json::to_string(&startup_error_html(message)).unwrap_or_default();
        let script = format!(
            "(() => {{ const render = () => {{ document.documentElement.innerHTML = {html}; document.title = '融光未能启动'; }}; if (document.readyState === 'loading') {{ document.addEventListener('DOMContentLoaded', render, {{ once: true }}); }} else {{ render(); }} }})();"
        );
        if let Err(err) = win.eval(script) {
            tracing::error!("显示启动错误失败: {err}");
        }
        let _ = win.show();
        let _ = win.set_focus();
    }
}

fn startup_error_html(message: &str) -> String {
    let escaped = message
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    format!(
        r#"<!doctype html><meta charset="utf-8"><style>
        html,body{{height:100%;margin:0;background:#0b0b0f;color:#e8e8ef;
        font:14px/1.7 -apple-system,"Segoe UI","Microsoft YaHei",sans-serif;
        display:grid;place-items:center}}
        .box{{max-width:640px;padding:32px 36px;border-radius:16px;
        background:#15151c;border:1px solid #2a2a36}}
        h1{{font-size:18px;margin:0 0 12px}}pre{{white-space:pre-wrap;color:#ff8a80;
        background:#0b0b0f;padding:14px;border-radius:10px;font-size:12px;overflow:auto}}
        p{{color:#9a9aa8;margin:0 0 16px}}</style>
        <div class="box"><h1>融光未能启动</h1>
        <p>本地服务初始化失败，请查看下方信息，或到数据目录的 logs/ 中查找完整日志。</p>
        <pre>{escaped}</pre></div>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn startup_result_wakes_all_waiters_and_remains_available() {
        let state = Arc::new(BootState::default());
        let mut waiters = Vec::new();
        for _ in 0..8 {
            let state = state.clone();
            waiters.push(tokio::spawn(async move { state.wait().await }));
        }
        tokio::task::yield_now().await;
        state.set(Ok(12345)).await;
        for waiter in waiters {
            assert_eq!(tokio::time::timeout(std::time::Duration::from_secs(1), waiter).await.unwrap().unwrap(), Ok(12345));
        }
        assert_eq!(state.wait().await, Ok(12345));
    }

    #[tokio::test]
    async fn startup_failure_reaches_pending_and_late_callers() {
        let state = Arc::new(BootState::default());
        let pending = state.clone();
        let waiter = tokio::spawn(async move { pending.wait().await });
        tokio::task::yield_now().await;
        state.set(Err("端口被占用".into())).await;
        assert_eq!(tokio::time::timeout(std::time::Duration::from_secs(1), waiter).await.unwrap().unwrap(), Err("端口被占用".into()));
        assert_eq!(state.wait().await, Err("端口被占用".into()));
    }

    #[test]
    fn error_page_escapes_messages_and_needs_no_filesystem() {
        let html = startup_error_html("目录 <script>alert('x')</script> & \"路径\" # %");
        assert!(html.contains("目录 &lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; &quot;路径&quot; # %"));
        assert!(!html.contains("<script>"));
        assert!(startup_error_html("C:\\用户\\数据 # %\n拒绝访问").contains("C:\\用户\\数据 # %\n拒绝访问"));
    }
}
