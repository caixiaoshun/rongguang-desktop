//! 系统托盘：显示主窗口、打开数据目录与退出。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

/// 安装托盘。
///
/// Windows 上 Tauri 会把托盘创建强制调度到主线程并等待完成，因此不能在子线程中
/// 同步调用，否则会与阻塞的主线程形成死锁。这里由调用方保证在主线程执行。
///
/// 底层 `libappindicator-sys` 在找不到动态库时是**直接 panic**，不是 Err，
/// 而精简版 Linux 桌面经常不带 appindicator，因此用 `catch_unwind` 兜住 panic：
/// 托盘失败只意味着「没有托盘图标」，主窗口照常可用。
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build_tray(app))) {
        Ok(result) => result,
        Err(_panic) => {
            tracing::warn!("托盘初始化 panic（通常缺少 libayatana-appindicator3），已跳过");
            Ok(())
        }
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开主界面", true, None::<&str>)?;
    let data = MenuItem::with_id(app, "data", "打开数据目录", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出融光", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &data, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("融光 · AI 视频创作工作台")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => focus_main(app),
            "data" => {
                if let Err(error) = crate::commands::open_data_dir(app.state::<crate::BootState>()) {
                    tracing::warn!("打开数据目录失败: {error}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                focus_main(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

fn focus_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}
