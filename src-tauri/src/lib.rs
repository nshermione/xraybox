mod client_api;
mod community;
mod dns;
mod engine;
mod profile;
mod routing;
mod single_instance;
mod tray;

use tauri::Manager;

#[cfg(target_os = "macos")]
pub fn helper_requested() -> bool {
    engine::macos::helper_requested()
}

#[cfg(target_os = "macos")]
pub fn run_helper() {
    engine::macos::run_helper();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    boot_log("rust run() entered");
    community::bind_client_api();
    single_instance::claim_or_exit();
    engine::windows::elevate_at_startup();
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());
    #[cfg(target_os = "android")]
    let builder = builder.plugin(engine::android::init());
    #[cfg(target_os = "ios")]
    let builder = builder.plugin(engine::ios::init());
    boot_log("plugins registered, building app");
    builder
        .manage(engine::Engine::new())
        .invoke_handler(tauri::generate_handler![
            engine::commands::start_engine,
            engine::commands::stop_engine,
            engine::commands::engine_status,
            engine::commands::session_stats,
            engine::commands::lookup_public_ip,
            engine::commands::save_manual_endpoint,
            engine::commands::get_manual_endpoint,
            engine::commands::get_dns_settings,
            engine::commands::save_dns_settings,
            engine::commands::update_geo_assets,
            engine::commands::get_route_packs,
            engine::commands::save_route_packs,
            engine::commands::switch_server,
            engine::commands::get_engine_settings,
            engine::commands::save_engine_settings,
            engine::commands::save_subscription_url,
            engine::commands::get_subscription,
            engine::commands::fetch_subscription,
            engine::commands::list_profiles,
            engine::commands::select_profile,
            engine::commands::ping_profiles,
            engine::commands::clear_profiles,
            engine::commands::set_fetch_interval,
            engine::commands::get_community_usage,
            engine::commands::verify_community,
            engine::commands::append_boot_log,
            engine::commands::claim_boot_splash,
            engine::commands::connect_after_elevate,
            engine::commands::get_engine_log,
            engine::commands::get_app_log,
        ])
        .setup(|app| {
            if let Ok(dir) = app.path().app_data_dir() {
                engine::app_log::init(dir.join("logs").join("app.log"));
            }
            #[cfg(any(windows, target_os = "macos"))]
            {
                tray::setup(app)?;
                single_instance::start_show_listener(&app.handle());
                let handle = app.handle().clone();
                if let Ok(settings) = engine::settings::load_engine_settings(&handle) {
                    if let Err(err) = engine::autostart::apply_start_on_boot(settings.start_on_boot)
                    {
                        eprintln!("xraybox: start on boot refresh failed: {err}");
                    }
                    #[cfg(windows)]
                    if !settings.auto_connect {
                        engine::kill_switch::release_leftover(&handle);
                    }
                }
                // Outside the settings block, and unconditional unlike the kill
                // switch: there is no tunnel yet, so nothing leaks differently,
                // and a hard kill would otherwise leave IPv6 and LAN DNS blocked
                // across reboots — including when settings fail to load.
                // macOS leaves this to the root helper. Asking for a password
                // here would prompt before the user has done anything.
                #[cfg(windows)]
                engine::leak_guard::release_leftover();
                if engine::autostart::autostart_requested() {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            #[cfg(any(windows, target_os = "macos"))]
            tray::on_window_event(window, event);
            #[cfg(not(any(windows, target_os = "macos")))]
            {
                let _ = (window, event);
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            match event {
                tauri::RunEvent::Ready => {
                    boot_log("RunEvent::Ready — WebView should exist");
                    engine::commands::spawn_background_ruleset_update(app);
                    client_api::socket::spawn_on_ready(app);
                }
                tauri::RunEvent::ExitRequested { .. } => {
                    boot_log("RunEvent::ExitRequested");
                    // Do not std::process::exit on Android: that runs
                    // destructors while wry/WebView threads still lock
                    // those mutexes (FORTIFY abort). MainActivity kills
                    // the UI process with SIGKILL instead; :vpn stays.
                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                    if let Some(engine) = app.try_state::<engine::Engine>() {
                        engine.stop_best_effort(app);
                    }
                }
                tauri::RunEvent::Exit => {
                    boot_log("RunEvent::Exit");
                    if cfg!(not(any(target_os = "android", target_os = "ios"))) {
                        if let Some(engine) = app.try_state::<engine::Engine>() {
                            engine.stop_best_effort(app);
                        }
                    }
                }
                _ => {}
            }
        });
}

fn boot_log(message: &str) {
    #[cfg(target_os = "android")]
    engine::android::log_info(&format!("boot: {message}"));
    #[cfg(not(target_os = "android"))]
    {
        eprintln!("xraybox-boot: {message}");
        engine::app_log::append(&format!("xraybox-boot: {message}"));
    }
}
