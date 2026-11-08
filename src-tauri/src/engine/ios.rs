//! iPhone packet-tunnel session files and On Demand rules.
//! The Swift extension reads the same names from the App Group.

#![cfg_attr(not(target_os = "ios"), allow(dead_code))]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::autostart::should_restore_on_boot;
use super::error::EngineError;

pub const APP_GROUP: &str = "group.io.github.xraybox";
pub const EXTENSION_BUNDLE: &str = "io.github.xraybox.PacketTunnel";
pub const SOCKS_PORT: u16 = 10808;
pub const MEMORY_BUDGET_BYTES: u64 = 45 * 1024 * 1024;
pub const SESSION_FILE: &str = "session.json";
pub const READY_FILE: &str = "ready.json";
pub const DESIRED_FILE: &str = "vpn-desired";
pub const LAST_FILE: &str = "vpn-last";
pub const USER_STOP_FILE: &str = "user-stop";
pub const STATS_FILE: &str = "stats.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IosSession {
    pub config_path: String,
    pub asset_dir: String,
    pub log_path: String,
    pub server_host: String,
    pub server_port: u16,
    pub exclude_hosts: Vec<String>,
    pub dns: Vec<String>,
    pub kill_switch: bool,
    pub socks_port: u16,
    pub on_demand: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IosReady {
    pub ok: bool,
    #[serde(default)]
    pub footprint: u64,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IosStats {
    #[serde(default)]
    pub uplink: u64,
    #[serde(default)]
    pub downlink: u64,
}

/// Hosts the OS tunnel must not capture. Remote DNS is intentionally absent:
/// extension sockets already skip the tunnel, and excluding a resolver would
/// let other apps query it on the physical interface.
pub fn ios_exclude_hosts(server: &str, extra: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut hosts = Vec::new();
    push_unique(&mut hosts, server);
    for host in extra {
        push_unique(&mut hosts, &host);
    }
    hosts.retain(|host| !is_loopback_host(host));
    hosts
}

pub fn ios_dev_exclude_hosts(dev_url_host: Option<&str>) -> Vec<String> {
    dev_url_host
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(|host| host.to_string())
        .into_iter()
        .collect()
}

/// On Demand is armed only for a session that is still desired. iOS reconnects
/// immediately when the rule stays on, so an explicit disconnect must clear
/// `session_desired` before this returns true. Kill switch and start-on-boot
/// are the two reasons to bring the extension back after jetsam or reboot.
pub fn ios_on_demand_enabled(
    user_requested_stop: bool,
    kill_switch: bool,
    session_desired: bool,
    start_on_boot: bool,
) -> bool {
    if user_requested_stop || !session_desired {
        return false;
    }
    start_on_boot || kill_switch
}

/// Same boot predicate Android uses, kept here so a test can show why iOS does
/// not arm On Demand from `has_last` alone.
pub fn ios_would_restore_on_boot(
    start_on_boot: bool,
    auto_connect: bool,
    session_desired: bool,
    has_last: bool,
) -> bool {
    should_restore_on_boot(start_on_boot, auto_connect, session_desired, has_last)
}

pub fn marker_path(shared: &Path, name: &str) -> PathBuf {
    shared.join(name)
}

pub fn write_marker(shared: &Path, name: &str) -> Result<(), EngineError> {
    fs::create_dir_all(shared)?;
    let path = marker_path(shared, name);
    let mut file = fs::File::create(&path)?;
    file.write_all(b"1")?;
    file.sync_all()?;
    Ok(())
}

pub fn clear_marker(shared: &Path, name: &str) -> Result<(), EngineError> {
    let path = marker_path(shared, name);
    if path.is_file() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn has_marker(shared: &Path, name: &str) -> bool {
    marker_path(shared, name).is_file()
}

pub fn write_session(shared: &Path, session: &IosSession) -> Result<(), EngineError> {
    fs::create_dir_all(shared)?;
    let path = shared.join(SESSION_FILE);
    let tmp = shared.join(format!("{SESSION_FILE}.tmp"));
    fs::write(&tmp, serde_json::to_string_pretty(session)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn read_ready(shared: &Path) -> Option<IosReady> {
    let text = fs::read_to_string(shared.join(READY_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn read_stats(shared: &Path) -> IosStats {
    fs::read_to_string(shared.join(STATS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn copy_geo_assets(from: &Path, to: &Path) -> Result<(), EngineError> {
    fs::create_dir_all(to)?;
    for name in ["geosite.dat", "geoip.dat"] {
        let source = from.join(name);
        if source.is_file() {
            let tmp = to.join(format!("{name}.tmp"));
            fs::copy(&source, &tmp)?;
            fs::rename(&tmp, to.join(name))?;
        }
    }
    Ok(())
}

pub fn wait_ready_file(
    shared: &Path,
    timeout: Duration,
    mut still_starting: impl FnMut() -> bool,
) -> Result<IosReady, EngineError> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !still_starting() {
            return Err(EngineError::Process(
                "xray exited before it was ready".into(),
            ));
        }
        if let Some(ready) = read_ready(shared) {
            if ready.ok {
                if ready.footprint > MEMORY_BUDGET_BYTES {
                    return Err(EngineError::Process("ios_memory_budget".into()));
                }
                return Ok(ready);
            }
            let detail = if ready.error.is_empty() {
                "ios tunnel failed".to_string()
            } else {
                ready.error
            };
            if detail.contains("ios_memory_budget") {
                return Err(EngineError::Process("ios_memory_budget".into()));
            }
            return Err(EngineError::Process(detail));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(EngineError::Process(
        "xray started but the extension did not become ready".into(),
    ))
}

fn push_unique(hosts: &mut Vec<String>, host: &str) {
    let host = host.trim();
    if host.is_empty() {
        return;
    }
    if !hosts.iter().any(|existing| existing == host) {
        hosts.push(host.to_string());
    }
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

#[cfg(target_os = "ios")]
mod plugin {
    use serde::{Deserialize, Serialize};
    use tauri::plugin::mobile::PluginInvokeError;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::{AppHandle, Manager, Runtime};

    use crate::engine::error::EngineError;

    tauri::ios_plugin_binding!(init_plugin_xraybox_vpn);

    pub struct IosVpn<R: Runtime>(PluginHandle<R>);

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct EstablishArgs {
        kill_switch: bool,
        on_demand: bool,
    }

    #[derive(Serialize)]
    struct FlagArgs {
        enabled: bool,
    }

    #[derive(Deserialize)]
    struct PathResponse {
        path: String,
    }

    #[derive(Deserialize)]
    struct RunningResponse {
        running: bool,
    }

    pub fn init<R: Runtime>() -> TauriPlugin<R> {
        Builder::<R, ()>::new("xraybox-vpn")
            .setup(|app, api| {
                let handle = api.register_ios_plugin(init_plugin_xraybox_vpn)?;
                app.manage(IosVpn(handle));
                Ok(())
            })
            .build()
    }

    pub fn shared_dir<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, EngineError> {
        let vpn = handle(app)?;
        let found = vpn
            .0
            .run_mobile_plugin::<PathResponse>("sharedDir", ())
            .map_err(map_invoke)?;
        Ok(std::path::PathBuf::from(found.path))
    }

    pub fn prepare<R: Runtime>(app: &AppHandle<R>) -> Result<(), EngineError> {
        let vpn = handle(app)?;
        vpn.0
            .run_mobile_plugin::<()>("prepare", ())
            .map_err(map_invoke)
    }

    pub fn establish<R: Runtime>(
        app: &AppHandle<R>,
        kill_switch: bool,
        on_demand: bool,
    ) -> Result<(), EngineError> {
        let vpn = handle(app)?;
        vpn.0
            .run_mobile_plugin::<()>(
                "establish",
                EstablishArgs {
                    kill_switch,
                    on_demand,
                },
            )
            .map_err(map_invoke)
    }

    pub fn teardown<R: Runtime>(app: &AppHandle<R>) -> Result<(), EngineError> {
        let vpn = handle(app)?;
        vpn.0
            .run_mobile_plugin::<()>("teardown", ())
            .map_err(map_invoke)
    }

    pub fn is_running<R: Runtime>(app: &AppHandle<R>) -> Result<bool, EngineError> {
        let vpn = handle(app)?;
        let found = vpn
            .0
            .run_mobile_plugin::<RunningResponse>("running", ())
            .map_err(map_invoke)?;
        Ok(found.running)
    }

    pub fn set_kill_switch<R: Runtime>(
        app: &AppHandle<R>,
        enabled: bool,
    ) -> Result<(), EngineError> {
        let vpn = handle(app)?;
        vpn.0
            .run_mobile_plugin::<()>("setKillSwitch", FlagArgs { enabled })
            .map_err(map_invoke)
    }

    pub fn set_on_demand<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<(), EngineError> {
        let vpn = handle(app)?;
        vpn.0
            .run_mobile_plugin::<()>("setOnDemand", FlagArgs { enabled })
            .map_err(map_invoke)
    }

    pub fn reload<R: Runtime>(app: &AppHandle<R>) -> Result<(), EngineError> {
        let vpn = handle(app)?;
        vpn.0
            .run_mobile_plugin::<()>("reload", ())
            .map_err(map_invoke)
    }

    fn handle<R: Runtime>(app: &AppHandle<R>) -> Result<tauri::State<'_, IosVpn<R>>, EngineError> {
        app.try_state::<IosVpn<R>>()
            .ok_or_else(|| EngineError::Process("ios vpn plugin is not registered".into()))
    }

    fn map_invoke(err: PluginInvokeError) -> EngineError {
        match &err {
            PluginInvokeError::InvokeRejected(resp) => {
                let code = resp.code.as_deref().unwrap_or("");
                let message = resp.message.as_deref().unwrap_or("");
                if code == "vpn_denied" || message == "vpn_denied" {
                    return EngineError::VpnDenied;
                }
                if code == "ios_memory_budget" || message.contains("ios_memory_budget") {
                    return EngineError::Process("ios_memory_budget".into());
                }
                if code.is_empty() {
                    EngineError::Process(message.to_string())
                } else if message.is_empty() || message == code {
                    EngineError::Process(code.to_string())
                } else {
                    EngineError::Process(format!("{code}: {message}"))
                }
            }
            _ => EngineError::Process(err.to_string()),
        }
    }
}

#[cfg(target_os = "ios")]
pub use plugin::{
    establish, init, is_running, prepare, reload, set_kill_switch, set_on_demand, shared_dir,
    teardown,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclude_hosts_are_server_and_extra_without_loopback() {
        let hosts = ios_exclude_hosts(
            "edge.example.com",
            [
                "localhost".into(),
                "192.168.1.20".into(),
                "edge.example.com".into(),
            ],
        );
        assert_eq!(hosts, vec!["edge.example.com", "192.168.1.20"]);
    }

    #[test]
    fn remote_dns_is_not_part_of_the_exclude_list() {
        let hosts = ios_exclude_hosts("edge.example.com", ios_dev_exclude_hosts(Some("10.0.0.8")));
        assert!(!hosts
            .iter()
            .any(|host| host == "1.1.1.1" || host == "8.8.8.8"));
        assert_eq!(hosts, vec!["edge.example.com", "10.0.0.8"]);
    }

    #[test]
    fn dev_host_is_only_added_when_it_is_a_real_machine() {
        assert!(ios_dev_exclude_hosts(Some("127.0.0.1")).is_empty() == false);
        let filtered =
            ios_exclude_hosts("edge.example.com", ios_dev_exclude_hosts(Some("127.0.0.1")));
        assert_eq!(filtered, vec!["edge.example.com"]);
        let lan = ios_exclude_hosts(
            "edge.example.com",
            ios_dev_exclude_hosts(Some("192.168.1.8")),
        );
        assert_eq!(lan, vec!["edge.example.com", "192.168.1.8"]);
    }

    #[test]
    fn on_demand_follows_an_active_session_only() {
        assert!(ios_on_demand_enabled(false, true, true, false));
        assert!(ios_on_demand_enabled(false, false, true, true));
        assert!(!ios_on_demand_enabled(true, true, true, true));
        assert!(!ios_on_demand_enabled(false, true, false, true));
        assert!(!ios_on_demand_enabled(false, false, true, false));
    }

    #[test]
    fn boot_restore_can_see_last_without_arming_on_demand() {
        assert!(ios_would_restore_on_boot(true, true, false, true));
        assert!(!ios_on_demand_enabled(false, false, false, true));
    }

    #[test]
    fn shared_file_names_match_the_extension() {
        assert_eq!(APP_GROUP, "group.io.github.xraybox");
        assert_eq!(EXTENSION_BUNDLE, "io.github.xraybox.PacketTunnel");
        assert_eq!(SESSION_FILE, "session.json");
        assert_eq!(READY_FILE, "ready.json");
        assert_eq!(DESIRED_FILE, "vpn-desired");
        assert_eq!(LAST_FILE, "vpn-last");
        assert_eq!(USER_STOP_FILE, "user-stop");
        assert_eq!(STATS_FILE, "stats.json");
        assert_eq!(SOCKS_PORT, 10808);
    }

    #[test]
    fn memory_budget_is_under_the_extension_jetsam_line() {
        assert_eq!(MEMORY_BUDGET_BYTES, 45 * 1024 * 1024);
    }
}
