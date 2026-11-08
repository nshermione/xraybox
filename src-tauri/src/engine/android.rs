use serde::{Deserialize, Serialize};
use tauri::plugin::mobile::PluginInvokeError;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Manager, Runtime};

use super::error::EngineError;

pub struct AndroidVpn<R: Runtime>(PluginHandle<R>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EstablishArgs {
    dns: Vec<String>,
    exclude_hosts: Vec<String>,
    config_path: String,
    workdir: String,
    asset_dir: String,
    sidecar_path: String,
    kill_switch: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Established {
    sidecar_path: String,
}

#[derive(Deserialize)]
struct SidecarPath {
    path: String,
}

#[derive(Deserialize)]
struct Running {
    running: bool,
}

pub struct AndroidStart {
    pub dns: Vec<String>,
    pub exclude_hosts: Vec<String>,
    pub config_path: String,
    pub workdir: String,
    pub asset_dir: String,
    pub sidecar_path: String,
    pub kill_switch: bool,
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::<R, ()>::new("xraybox-vpn")
        .setup(|app, api| {
            let handle = api.register_android_plugin("io.github.xraybox.vpn", "VpnPlugin")?;
            app.manage(AndroidVpn(handle));
            Ok(())
        })
        .build()
}

pub fn prepare<R: Runtime>(app: &AppHandle<R>) -> Result<(), EngineError> {
    let vpn = vpn_handle(app)?;
    vpn.0
        .run_mobile_plugin::<()>("prepare", ())
        .map_err(map_invoke)
}

pub fn start_vpn<R: Runtime>(app: &AppHandle<R>, start: AndroidStart) -> Result<(), EngineError> {
    if !std::path::Path::new(&start.sidecar_path).is_file() {
        return Err(EngineError::SidecarMissing);
    }
    let vpn = vpn_handle(app)?;
    let _established = vpn
        .0
        .run_mobile_plugin::<Established>(
            "establish",
            EstablishArgs {
                dns: start.dns,
                exclude_hosts: start.exclude_hosts,
                config_path: start.config_path,
                workdir: start.workdir,
                asset_dir: start.asset_dir,
                sidecar_path: start.sidecar_path,
                kill_switch: start.kill_switch,
            },
        )
        .map_err(map_invoke)?;
    Ok(())
}

#[derive(Serialize)]
struct FlagArgs {
    enabled: bool,
}

pub fn apply_start_on_boot<R: Runtime>(
    app: &AppHandle<R>,
    enabled: bool,
) -> Result<(), EngineError> {
    let vpn = vpn_handle(app)?;
    vpn.0
        .run_mobile_plugin::<()>("applyStartOnBoot", FlagArgs { enabled })
        .map_err(map_invoke)
}

pub fn set_kill_switch<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<(), EngineError> {
    let vpn = vpn_handle(app)?;
    vpn.0
        .run_mobile_plugin::<()>("setKillSwitch", FlagArgs { enabled })
        .map_err(map_invoke)
}

pub fn teardown<R: Runtime>(app: &AppHandle<R>) -> Result<(), EngineError> {
    let vpn = vpn_handle(app)?;
    vpn.0
        .run_mobile_plugin::<()>("teardown", ())
        .map_err(map_invoke)
}

pub fn is_running<R: Runtime>(app: &AppHandle<R>) -> Result<bool, EngineError> {
    log_info("boot: is_running plugin invoke start");
    let started = std::time::Instant::now();
    let vpn = vpn_handle(app)?;
    let found = vpn
        .0
        .run_mobile_plugin::<Running>("running", ())
        .map_err(map_invoke)?;
    log_info(&format!(
        "boot: is_running plugin invoke done running={} {}ms",
        found.running,
        started.elapsed().as_millis()
    ));
    Ok(found.running)
}

pub fn sidecar_path<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, EngineError> {
    let vpn = vpn_handle(app)?;
    let found = vpn
        .0
        .run_mobile_plugin::<SidecarPath>("sidecarPath", ())
        .map_err(map_invoke)?;
    Ok(std::path::PathBuf::from(found.path))
}

pub fn log_info(message: &str) {
    write_log(ANDROID_LOG_INFO, message);
}

pub fn log_error(message: &str) {
    write_log(ANDROID_LOG_ERROR, message);
}

const BOOT_LOG_PATH: &str = "/data/data/io.github.xraybox/files/xraybox-boot.log";

fn write_log(prio: i32, message: &str) {
    let text = message.replace('\0', "");
    eprintln!("{text}");
    append_boot_file(&text);
    let tag = std::ffi::CString::new("xraybox-boot").expect("static tag");
    let body = std::ffi::CString::new(text)
        .unwrap_or_else(|_| std::ffi::CString::new("xraybox last_error").expect("fallback"));
    unsafe {
        __android_log_write(prio, tag.as_ptr(), body.as_ptr());
    }
}

fn append_boot_file(message: &str) {
    use std::io::Write;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let line = format!(
        "{}.{:03} rust {message}\n",
        now.as_secs(),
        now.subsec_millis()
    );
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(BOOT_LOG_PATH)
    {
        let _ = file.write_all(line.as_bytes());
        let _ = file.sync_all();
    }
}

#[link(name = "log")]
extern "C" {
    fn __android_log_write(
        prio: i32,
        tag: *const std::os::raw::c_char,
        text: *const std::os::raw::c_char,
    ) -> i32;
}

const ANDROID_LOG_INFO: i32 = 4;
const ANDROID_LOG_ERROR: i32 = 6;

fn vpn_handle<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<tauri::State<'_, AndroidVpn<R>>, EngineError> {
    app.try_state::<AndroidVpn<R>>()
        .ok_or_else(|| EngineError::Process("android vpn plugin is not registered".into()))
}

fn map_invoke(err: PluginInvokeError) -> EngineError {
    match &err {
        PluginInvokeError::InvokeRejected(resp) => {
            let code = resp.code.as_deref().unwrap_or("");
            let message = resp.message.as_deref().unwrap_or("");
            if code == "vpn_denied" || message == "vpn_denied" {
                return EngineError::VpnDenied;
            }
            if code == super::error::SIDECAR_MISSING || message == super::error::SIDECAR_MISSING {
                return EngineError::SidecarMissing;
            }
            EngineError::Process(err.to_string())
        }
        _ => EngineError::Process(err.to_string()),
    }
}
