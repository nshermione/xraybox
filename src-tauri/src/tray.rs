use crate::engine::state::EngineStatus;
use tauri::AppHandle;

pub const APP_SHOWN_EVENT: &str = "app-shown";

pub fn show_main(app: &AppHandle) {
    #[cfg(any(windows, target_os = "macos"))]
    desktop::show_main(app);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = app;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Idle,
    Connecting,
    Connected,
}

pub fn icon_kind(status: EngineStatus) -> IconKind {
    match status {
        EngineStatus::Running => IconKind::Connected,
        EngineStatus::Starting | EngineStatus::Stopping => IconKind::Connecting,
        EngineStatus::Stopped | EngineStatus::Failed => IconKind::Idle,
    }
}

pub fn tooltip_for(status: EngineStatus) -> &'static str {
    match icon_kind(status) {
        IconKind::Idle => "xraybox — Disconnected",
        IconKind::Connecting => "xraybox — Connecting",
        IconKind::Connected => "xraybox — Connected",
    }
}

pub fn sync_icons(app: &AppHandle, status: EngineStatus) {
    #[cfg(any(windows, target_os = "macos"))]
    desktop::sync_icons(app, status);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (app, status);
    }
}

#[cfg(any(windows, target_os = "macos"))]
pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    desktop::setup(app)
}

#[cfg(any(windows, target_os = "macos"))]
pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    desktop::on_window_event(window, event)
}

#[cfg(any(windows, target_os = "macos"))]
mod desktop {
    use super::{tooltip_for, EngineStatus, IconKind};
    use tauri::image::Image;
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::{AppHandle, Emitter, Manager};

    const TRAY_ID: &str = "xraybox";

    fn status_bytes(kind: IconKind) -> &'static [u8] {
        #[cfg(target_os = "macos")]
        {
            // Colored glyphs, not menu-bar templates. Template images are
            // recolored by AppKit, so white / yellow / green would be lost.
            match kind {
                IconKind::Idle => include_bytes!("../icons/status-tray-idle.png").as_slice(),
                IconKind::Connecting => {
                    include_bytes!("../icons/status-tray-connecting.png").as_slice()
                }
                IconKind::Connected => {
                    include_bytes!("../icons/status-tray-connected.png").as_slice()
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            match kind {
                IconKind::Idle => include_bytes!("../icons/status-idle.png").as_slice(),
                IconKind::Connecting => include_bytes!("../icons/status-connecting.png").as_slice(),
                IconKind::Connected => include_bytes!("../icons/status-connected.png").as_slice(),
            }
        }
    }

    fn status_image(status: EngineStatus) -> Image<'static> {
        Image::from_bytes(status_bytes(super::icon_kind(status))).expect("status icon png")
    }

    fn apply_icons(app: &AppHandle, status: EngineStatus) {
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            #[cfg(target_os = "macos")]
            let _ = tray.set_icon_with_as_template(Some(status_image(status)), false);
            #[cfg(not(target_os = "macos"))]
            let _ = tray.set_icon(Some(status_image(status)));
            let _ = tray.set_tooltip(Some(tooltip_for(status)));
        }
        #[cfg(windows)]
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_icon(status_image(status));
        }
    }

    pub fn sync_icons(app: &AppHandle, status: EngineStatus) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || apply_icons(&handle, status));
    }

    pub fn show_main(app: &AppHandle) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            let _ = window.emit(super::APP_SHOWN_EVENT, ());
        }
    }

    pub fn setup(app: &tauri::App) -> tauri::Result<()> {
        let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
        let menu = Menu::with_items(app, &[&show, &quit])?;

        TrayIconBuilder::with_id(TRAY_ID)
            .icon(status_image(EngineStatus::Stopped))
            .tooltip(tooltip_for(EngineStatus::Stopped))
            .menu(&menu)
            .show_menu_on_left_click(cfg!(target_os = "macos"))
            .icon_as_template(false)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "show" => show_main(app),
                "quit" => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if cfg!(windows) {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                }
            })
            .build(app)?;

        apply_icons(&app.handle(), EngineStatus::Stopped);
        Ok(())
    }

    pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = window.hide();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_uses_connected_icon() {
        assert_eq!(icon_kind(EngineStatus::Running), IconKind::Connected);
        assert_eq!(tooltip_for(EngineStatus::Running), "xraybox — Connected");
    }

    #[test]
    fn starting_and_stopping_use_connecting_icon() {
        assert_eq!(icon_kind(EngineStatus::Starting), IconKind::Connecting);
        assert_eq!(icon_kind(EngineStatus::Stopping), IconKind::Connecting);
        assert_eq!(tooltip_for(EngineStatus::Starting), "xraybox — Connecting");
    }

    #[test]
    fn app_shown_event_name_matches_frontend() {
        assert_eq!(APP_SHOWN_EVENT, "app-shown");
    }

    #[test]
    fn stopped_and_failed_use_idle_icon() {
        assert_eq!(icon_kind(EngineStatus::Stopped), IconKind::Idle);
        assert_eq!(icon_kind(EngineStatus::Failed), IconKind::Idle);
        assert_eq!(tooltip_for(EngineStatus::Failed), "xraybox — Disconnected");
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn status_icon_pngs_decode() {
        for bytes in [
            include_bytes!("../icons/status-idle.png").as_slice(),
            include_bytes!("../icons/status-connecting.png").as_slice(),
            include_bytes!("../icons/status-connected.png").as_slice(),
            include_bytes!("../icons/status-tray-idle.png").as_slice(),
            include_bytes!("../icons/status-tray-connecting.png").as_slice(),
            include_bytes!("../icons/status-tray-connected.png").as_slice(),
        ] {
            tauri::image::Image::from_bytes(bytes).expect("status icon png");
        }
    }
}
