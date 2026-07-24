//! System tray icon and the "now playing" state.
//!
//! The tray persists for the app's lifetime: it shows what is playing, and lets
//! the window be restored after it minimises while a game runs. Clicking the
//! icon, or its Show item, brings Selene back.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub const TRAY_ID: &str = "main";
const IDLE_TOOLTIP: &str = "Selene";

/// Build the tray icon during app setup.
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Selene", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().expect("bundled window icon").clone())
        .tooltip(IDLE_TOOLTIP)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => reveal(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left click restores the window, matching Steam's tray behaviour.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                reveal(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Show and focus the main window.
pub fn reveal(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Enter the playing state: tooltip shows the title, and the window minimises
/// out of the way if the user has that enabled.
pub fn start_playing(app: &AppHandle, title: &str, minimize: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(&format!("Now playing: {title}")));
    }
    if minimize {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.minimize();
        }
    }
}

/// Leave the playing state: reset the tooltip and bring the window back.
pub fn stop_playing(app: &AppHandle, was_minimized: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(IDLE_TOOLTIP));
    }
    if was_minimized {
        reveal(app);
    }
}
