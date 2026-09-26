//! The notification-area icon that keeps the app resident: closing the main
//! window only hides it (see `lib.rs`'s `on_window_event`), so the session,
//! the mic hotkey and the subtitle overlay keep running, and the tray is how
//! the window comes back — or how the app actually quits.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

const TRAY_ID: &str = "main";
/// Label of the window declared in `tauri.conf.json`.
pub const MAIN_WINDOW: &str = "main";

const MENU_SHOW: &str = "show";
const MENU_QUIT: &str = "quit";

fn menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let show = MenuItem::with_id(
        app,
        MENU_SHOW,
        crate::tr!("Show Voice Chat", "显示主界面"),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        MENU_QUIT,
        crate::tr!("Quit", "退出"),
        true,
        None::<&str>,
    )?;
    Menu::with_items(app, &[&show, &quit])
}

/// Must run after `i18n::set` in `setup`, so the menu starts out in the
/// user's language.
pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Voice Chat")
        .menu(&menu(app)?)
        // Left click brings the window back; the menu is on right click.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_SHOW => show_main_window(app),
            // Goes through `RunEvent::Exit`, which shuts the session down.
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Rebuilds the menu in the current display language. Called from
/// `app::commands::set_ui_language`; a no-op if the tray was never created.
pub fn refresh_language<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu(app)?))?;
    }
    Ok(())
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    // Each step is best-effort: a window that fails to unminimize should
    // still get shown and focused rather than stay hidden.
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}
