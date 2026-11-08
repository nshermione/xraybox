use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::config::{ConfigBuilder, API_LISTEN, PROXY_OUTBOUND_TAG};
use super::error::EngineError;
#[cfg(windows)]
use super::process::require_wintun_beside_sidecar;
#[cfg(not(any(target_os = "android", target_os = "macos")))]
use super::process::run_config_args;
use super::process::{
    handshake_failure_code, sidecar_path, validate_config, EngineProcess, LOG_FILE_NAME,
};
use super::settings::EngineSettings;
use super::state::{EngineSnapshot, EngineStateMachine, EngineStatus};
use crate::dns::store as dns_store;
use crate::dns::DnsSettings;
use crate::profile::country::infer_country_code;
use crate::profile::ping;
use crate::profile::subscription::{self, latest_latency};
use crate::profile::{
    load_manual_profile, resolve_connect_profile, save_manual_profile, sync_smart_selected_id,
    FetchInterval, ProfileNode, SubscriptionSettings, VlessReality,
};
use crate::routing::brands::bind_route;
use crate::routing::ruleset;
use crate::routing::store as route_store;
use crate::routing::PackState;

struct StartPlan {
    sidecar: PathBuf,
    workdir: PathBuf,
    config_arg: String,
    asset_dir: PathBuf,
    warnings: Vec<String>,
    extra_env: Vec<(String, String)>,
    server: String,
    server_port: u16,
    dns_servers: Vec<String>,
    app_data_dir: PathBuf,
}

const PROBE_URL: &str = "http://cp.cloudflare.com/generate_204";
const TRACE_URL: &str = "http://cp.cloudflare.com/cdn-cgi/trace";
const PROBE_ATTEMPTS: u32 = 3;
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const PROBE_INTERVAL: Duration = Duration::from_secs(30);
// The frontend only polls while its window is alive, and hidden-window timers
// are throttled. A dead sidecar takes the TUN default route down with it, so the
// UI must not be the first thing that notices.
const SUPERVISOR_POLL: Duration = Duration::from_secs(1);
// Creating the Wintun adapter can take much longer than the 0.83s we
// measured on a warm machine: first-ever driver install, orphaned-adapter
// cleanup, or a slow disk. A dead child is detected immediately, so this
// budget only waits on a live process that is still setting up.
const API_READY_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeOutcome {
    Ok,
    Timeout,
    Refused,
    Hijacked,
}

pub struct Engine {
    inner: Arc<Mutex<EngineInner>>,
}

struct EngineInner {
    machine: EngineStateMachine,
    process: EngineProcess,
    last_error: Option<String>,
    last_probe_at: Option<Instant>,
    probe_inflight: bool,
    session_started_at: Option<SystemTime>,
    baseline_up: u64,
    baseline_down: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTraffic {
    pub uplink: u64,
    pub downlink: u64,
    pub total: u64,
    pub started_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicIpInfo {
    pub ip: Option<String>,
    pub country_code: Option<String>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(EngineInner {
                machine: EngineStateMachine::default(),
                process: EngineProcess::default(),
                last_error: None,
                last_probe_at: None,
                probe_inflight: false,
                session_started_at: None,
                baseline_up: 0,
                baseline_down: 0,
            })),
        }
    }

    pub fn start(&self, app: &AppHandle) -> Result<EngineSnapshot, EngineError> {
        {
            let mut inner = lock_engine(&self.inner)?;
            inner.reconcile(app);
        }

        #[cfg(not(any(target_os = "android", target_os = "macos", target_os = "ios")))]
        if super::windows::plan_connect_elevation(super::windows::is_elevated())
            == super::windows::ConnectElevation::NeedsRelaunch
        {
            match super::windows::elevate_for_connect() {
                Ok(()) => std::process::exit(0),
                Err(err) => {
                    let mut inner = lock_engine(&self.inner)?;
                    let detail = err.to_string();
                    inner.last_error = Some(detail.clone());
                    log_connect_error(&detail);
                    return Err(err);
                }
            }
        }

        #[cfg(target_os = "android")]
        super::android::prepare(app)?;

        #[cfg(target_os = "ios")]
        super::ios::prepare(app)?;

        {
            let mut inner = lock_engine(&self.inner)?;
            inner.reconcile(app);
            inner.machine.begin_start()?;
            publish_status(app, inner.machine.status());
        }

        let prepared = (|| -> Result<StartPlan, EngineError> {
            let profile = resolve_connect_profile(app)?;
            profile.validate()?;
            let server = profile.server.clone();
            let server_port = profile.server_port;
            let dns_settings = crate::dns::store::load_dns_settings(app)?;
            let dns_servers = crate::dns::tun_dns_servers(&dns_settings);
            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|err| EngineError::Config(err.to_string()))?;
            let mut extra_env = Vec::new();
            let sidecar = resolve_start_sidecar(app, &mut extra_env)?;
            let (builder, warnings) = prepare_builder(app, profile)?;
            #[allow(unused_mut)]
            let mut config = builder.build();
            #[cfg(target_os = "ios")]
            {
                let shared = super::ios::shared_dir(app)?;
                let _ = super::ios::clear_marker(&shared, super::ios::USER_STOP_FILE);
                let log_path = shared.join("run").join(LOG_FILE_NAME);
                config["log"]["error"] = serde_json::json!(log_path);
                config["log"]["access"] = serde_json::json!("none");
            }
            let config_path = write_runtime_config(app, &config)?;
            let workdir = config_path
                .parent()
                .ok_or_else(|| EngineError::Config("config path has no parent".into()))?
                .to_path_buf();
            let config_arg = config_path.to_string_lossy().into_owned();
            let asset_dir = ruleset::ensure_geo_assets(
                &route_store::geo_asset_dir(app)?,
                &route_store::bundled_geo_dirs(app),
            )?;
            start_android_sidecar(app, &server, &config_path, &workdir, &asset_dir, &sidecar)?;
            start_ios_tunnel(app, &server, server_port, &config_path, &asset_dir)?;
            Ok(StartPlan {
                sidecar,
                workdir,
                config_arg,
                asset_dir,
                warnings,
                extra_env,
                server,
                server_port,
                dns_servers,
                app_data_dir,
            })
        })();

        let StartPlan {
            sidecar,
            workdir,
            config_arg,
            asset_dir,
            warnings,
            extra_env,
            server,
            server_port,
            dns_servers,
            app_data_dir,
        } = match prepared {
            Ok(ready) => ready,
            Err(err) => {
                teardown_mobile_vpn(app);
                let mut inner = lock_engine(&self.inner)?;
                if inner.machine.status() == EngineStatus::Starting {
                    inner.machine.mark_failed();
                    publish_status(app, inner.machine.status());
                    let detail = err.to_string();
                    inner.last_error = Some(detail.clone());
                    log_connect_error(&detail);
                }
                return Err(err);
            }
        };

        {
            #[cfg(not(target_os = "macos"))]
            let _ = (&server, server_port, &dns_servers, &app_data_dir);
            #[cfg(target_os = "macos")]
            let _ = &extra_env;
            #[cfg(any(target_os = "android", target_os = "ios"))]
            let _ = (&config_arg, &asset_dir, extra_env);

            #[cfg(target_os = "macos")]
            let inner = lock_engine(&self.inner)?;
            #[cfg(not(target_os = "macos"))]
            let mut inner = lock_engine(&self.inner)?;
            if inner.machine.status() != EngineStatus::Starting {
                return Ok(inner.snapshot());
            }
            #[cfg(any(target_os = "android", target_os = "ios"))]
            {
                inner.process.mark_plugin_owned();
                #[cfg(target_os = "ios")]
                {
                    if let Ok(shared) = super::ios::shared_dir(app) {
                        inner
                            .process
                            .attach_log_path(shared.join("run").join(LOG_FILE_NAME));
                    }
                }
                #[cfg(not(target_os = "ios"))]
                inner.process.attach_log_path(workdir.join(LOG_FILE_NAME));
            }
            #[cfg(target_os = "macos")]
            {
                drop(inner);
                let started = super::macos::start_tunnel(super::macos::StartSpec {
                    sidecar: sidecar.to_string_lossy().into_owned(),
                    config_path: config_arg.clone(),
                    workdir: workdir.to_string_lossy().into_owned(),
                    asset_dir: asset_dir.to_string_lossy().into_owned(),
                    app_data_dir: app_data_dir.to_string_lossy().into_owned(),
                    server_host: server,
                    server_port,
                    dns_servers,
                });
                let mut inner = lock_engine(&self.inner)?;
                if inner.machine.status() != EngineStatus::Starting {
                    if started.is_ok() {
                        let _ = super::macos::stop_tunnel();
                    }
                    return Ok(inner.snapshot());
                }
                if let Err(err) = started {
                    return abort_engine(&mut inner, app, &err.to_string(), true);
                }
                inner.process.mark_helper_owned();
                inner.process.attach_log_path(workdir.join(LOG_FILE_NAME));
                if !inner.process.is_alive() {
                    return abort_engine(&mut inner, app, "xray exited immediately", true);
                }
            }
            #[cfg(not(any(target_os = "android", target_os = "macos", target_os = "ios")))]
            {
                // Before the sidecar, not after: the guard has to be in place by
                // the time the TUN takes the default route, or the window
                // between the two is a leak.
                if let Err(err) = super::leak_guard::apply() {
                    return abort_engine(&mut inner, app, &err.to_string(), true);
                }
                match inner.process.start_with_env(
                    &sidecar,
                    &run_config_args(&config_arg),
                    &workdir,
                    &asset_dir,
                    &extra_env,
                ) {
                    Ok(()) => {
                        if !inner.process.is_alive() {
                            return abort_engine(&mut inner, app, "xray exited immediately", true);
                        }
                    }
                    Err(err) => return abort_engine(&mut inner, app, &err.to_string(), true),
                }
            }
        }

        self.wait_until_running(app, &sidecar, &warnings, true)
    }

    pub fn stop(&self, app: &AppHandle) -> Result<EngineSnapshot, EngineError> {
        begin_ios_user_stop(app);
        let mut inner = lock_engine(&self.inner)?;
        inner.reconcile(app);
        let snapshot = match inner.machine.status() {
            EngineStatus::Stopped => {
                drop(inner);
                teardown_mobile_vpn(app);
                release_network_guards(app);
                return self.status_snapshot(app);
            }
            EngineStatus::Failed => {
                let _ = inner.process.stop();
                log_leftover_tun();
                inner.machine.mark_stopped();
                inner.end_session();
                publish_status(app, inner.machine.status());
                inner.snapshot()
            }
            _ => {
                inner.machine.begin_stop()?;
                publish_status(app, inner.machine.status());
                if let Err(err) = inner.process.stop() {
                    inner.machine.mark_failed();
                    publish_status(app, inner.machine.status());
                    inner.last_error = Some(err.to_string());
                    log_leftover_tun();
                    drop(inner);
                    teardown_mobile_vpn(app);
                    return Err(err);
                }
                log_leftover_tun();
                inner.machine.mark_stopped();
                inner.end_session();
                publish_status(app, inner.machine.status());
                inner.snapshot()
            }
        };
        drop(inner);
        teardown_mobile_vpn(app);
        release_network_guards(app);
        Ok(snapshot)
    }

    fn status_snapshot(&self, app: &AppHandle) -> Result<EngineSnapshot, EngineError> {
        let mut inner = lock_engine(&self.inner)?;
        inner.reconcile(app);
        Ok(inner.snapshot())
    }

    pub fn stop_best_effort(&self, app: &AppHandle) {
        let _ = self.stop(app);
    }

    pub fn engine_log(&self, app: &AppHandle) -> Result<String, EngineError> {
        let mut inner = lock_engine(&self.inner)?;
        if let Ok(path) = runtime_xray_log_path(app) {
            if path.is_file() {
                inner.process.attach_log_path(path);
            }
        }
        inner.process.refresh_log();
        let log = inner.process.last_log();
        if log.starts_with("failed to read sidecar log:") {
            return Ok(String::new());
        }
        Ok(log.to_string())
    }

    pub fn status(&self, app: &AppHandle) -> Result<EngineSnapshot, EngineError> {
        let started = std::time::Instant::now();
        boot_trace("engine_status lock+reconcile start");
        let mut inner = lock_engine(&self.inner)?;
        inner.reconcile(app);
        boot_trace(&format!(
            "engine_status adopt start {}ms",
            started.elapsed().as_millis()
        ));
        adopt_android_vpn(&mut inner, app);
        adopt_ios_vpn(&mut inner, app);
        boot_trace(&format!(
            "engine_status adopt done {}ms status={:?}",
            started.elapsed().as_millis(),
            inner.machine.status()
        ));
        let should_probe = inner.machine.status() == EngineStatus::Running
            && !inner.probe_inflight
            && inner
                .last_probe_at
                .map(|at| at.elapsed() >= PROBE_INTERVAL)
                .unwrap_or(true);
        if should_probe {
            inner.probe_inflight = true;
        }
        let snapshot = inner.snapshot();
        drop(inner);
        if should_probe {
            self.spawn_running_probe();
        }
        boot_trace(&format!(
            "engine_status return {}ms",
            started.elapsed().as_millis()
        ));
        Ok(snapshot)
    }

    fn spawn_running_probe(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::spawn(move || {
            // A failed probe at start means the config never worked, so start
            // stops the process. A failed probe later may be a passing blip:
            // set last_error and leave the tunnel up.
            let outcome = probe_tunnel();
            let Ok(mut guard) = inner.lock() else {
                return;
            };
            guard.probe_inflight = false;
            guard.last_probe_at = Some(Instant::now());
            if guard.machine.status() != EngineStatus::Running {
                return;
            }
            match classify_probe(outcome) {
                None => {
                    if guard
                        .last_error
                        .as_deref()
                        .is_some_and(|err| err.starts_with("tunnel_"))
                    {
                        guard.last_error = None;
                    }
                }
                Some(code) => guard.last_error = Some(code.to_string()),
            }
        });
    }

    pub fn switch_server(
        &self,
        app: &AppHandle,
        profile: VlessReality,
    ) -> Result<EngineSnapshot, EngineError> {
        profile.validate()?;
        let persist = super::settings::load_engine_settings(app)?.kill_switch;
        let running = {
            let mut inner = lock_engine(&self.inner)?;
            inner.reconcile(app);
            inner.machine.status() == EngineStatus::Running
        };
        if !running {
            crate::profile::save_manual_profile(app, &profile)?;
            return self.status(app);
        }
        crate::profile::save_manual_profile(app, &profile)?;
        match switch_strategy(persist) {
            SwitchStrategy::KeepVpnRestartXray => self.restart_kept_tunnel(app, profile),
            SwitchStrategy::FullRestart => {
                let _ = self.stop(app);
                self.start(app)
            }
            SwitchStrategy::ApiReload => match self.reload_outbound(app, profile) {
                Ok(snapshot) => Ok(snapshot),
                Err(err) => {
                    let _ = self.stop(app);
                    match self.start(app) {
                        Ok(mut snapshot) => {
                            snapshot.last_error = Some(err.to_string());
                            Ok(snapshot)
                        }
                        Err(start_err) => Err(start_err),
                    }
                }
            },
        }
    }

    fn wait_until_running(
        &self,
        app: &AppHandle,
        sidecar: &Path,
        warnings: &[String],
        teardown_vpn_on_fail: bool,
    ) -> Result<EngineSnapshot, EngineError> {
        #[cfg(target_os = "ios")]
        {
            let shared = match super::ios::shared_dir(app) {
                Ok(shared) => shared,
                Err(err) => {
                    let mut inner = lock_engine(&self.inner)?;
                    return abort_engine(&mut inner, app, &err.to_string(), teardown_vpn_on_fail);
                }
            };
            if let Err(err) = super::ios::wait_ready_file(&shared, API_READY_TIMEOUT, || {
                lock_engine(&self.inner)
                    .map(|mut inner| inner.machine.status() == EngineStatus::Starting)
                    .unwrap_or(false)
            }) {
                let mut inner = lock_engine(&self.inner)?;
                if inner.machine.status() != EngineStatus::Starting {
                    return Ok(inner.snapshot());
                }
                return abort_engine(&mut inner, app, &err.to_string(), teardown_vpn_on_fail);
            }
        }
        #[cfg(not(target_os = "ios"))]
        {
            let wait = super::xray_api::wait_api(API_LISTEN, API_READY_TIMEOUT, || {
                lock_engine(&self.inner)
                    .map(|mut inner| {
                        inner.machine.status() == EngineStatus::Starting && inner.process.is_alive()
                    })
                    .unwrap_or(false)
            });
            if wait != super::xray_api::ApiWait::Ready {
                let mut inner = lock_engine(&self.inner)?;
                if inner.machine.status() != EngineStatus::Starting {
                    return Ok(inner.snapshot());
                }
                let alive = inner.process.is_alive();
                let message = if wait == super::xray_api::ApiWait::Exited || !alive {
                    "xray exited before the API became ready"
                } else {
                    "xray started but API did not become ready"
                };
                return abort_engine(&mut inner, app, message, teardown_vpn_on_fail);
            }
            let asset_dir = ruleset::ensure_geo_assets(
                &route_store::geo_asset_dir(app)?,
                &route_store::bundled_geo_dirs(app),
            )?;
            if let Err(err) = super::xray_api::confirm_api(sidecar, API_LISTEN, &asset_dir) {
                let mut inner = lock_engine(&self.inner)?;
                if inner.machine.status() != EngineStatus::Starting {
                    return Ok(inner.snapshot());
                }
                return abort_engine(&mut inner, app, &err.to_string(), teardown_vpn_on_fail);
            }
        }
        #[cfg(target_os = "ios")]
        {
            let _ = sidecar;
        }

        {
            let inner = lock_engine(&self.inner)?;
            if inner.machine.status() != EngineStatus::Starting {
                return Ok(inner.snapshot());
            }
        }

        // wait_api / confirm_api are a liveness probe (gRPC port is up), not
        // a health check. This request leaves the app, follows the TUN
        // default route, and answers: does user traffic survive the tunnel?
        match probe_tunnel() {
            ProbeOutcome::Ok => {}
            outcome => {
                let mut inner = lock_engine(&self.inner)?;
                if inner.machine.status() != EngineStatus::Starting {
                    return Ok(inner.snapshot());
                }
                return abort_engine(
                    &mut inner,
                    app,
                    classify_probe(outcome).unwrap_or("tunnel_unreachable"),
                    teardown_vpn_on_fail,
                );
            }
        }

        {
            let inner = lock_engine(&self.inner)?;
            if inner.machine.status() != EngineStatus::Starting {
                return Ok(inner.snapshot());
            }
        }

        if let Err(err) = hold_kill_switch(app) {
            let mut inner = lock_engine(&self.inner)?;
            if inner.machine.status() != EngineStatus::Starting {
                drop(inner);
                release_network_guards(app);
                return Ok(lock_engine(&self.inner)?.snapshot());
            }
            return abort_engine(&mut inner, app, &err.to_string(), teardown_vpn_on_fail);
        }

        let mut inner = lock_engine(&self.inner)?;
        if inner.machine.status() != EngineStatus::Starting {
            drop(inner);
            release_network_guards(app);
            return Ok(lock_engine(&self.inner)?.snapshot());
        }
        if !inner.process.is_alive() {
            return abort_engine(
                &mut inner,
                app,
                "xray exited immediately",
                teardown_vpn_on_fail,
            );
        }
        inner.process.refresh_log();
        inner.machine.mark_running()?;
        inner.begin_session();
        publish_status(app, inner.machine.status());
        inner.last_error = running_last_error(inner.process.last_log(), warnings);
        inner.last_probe_at = Some(Instant::now());
        let snapshot = inner.snapshot();
        drop(inner);
        note_ios_connected(app);
        self.spawn_supervisor(app);
        Ok(snapshot)
    }

    /// Flips out of `Running` when the sidecar dies, without waiting for the
    /// frontend to poll. The guards are deliberately left in place: the sidecar
    /// takes the TUN default route down with it, so every unmatched packet is
    /// about to reach the physical NIC.
    fn spawn_supervisor(&self, app: &AppHandle) {
        // Android's sidecar belongs to the VpnService, so `is_alive` is always
        // true there and `adopt_android_vpn` owns that reconciliation instead.
        if cfg!(any(target_os = "android", target_os = "ios")) {
            return;
        }
        let inner = Arc::clone(&self.inner);
        let app = app.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(SUPERVISOR_POLL);
            let Ok(mut guard) = inner.lock() else {
                return;
            };
            // A restart spawns its own supervisor, so anything that is no longer
            // `Running` belongs to a previous session and this thread retires.
            if guard.machine.status() != EngineStatus::Running {
                return;
            }
            if guard.process.is_alive() {
                continue;
            }
            guard.reconcile(&app);
            drop(guard);
            hold_guards_after_crash(&app);
            return;
        });
    }

    fn restart_kept_tunnel(
        &self,
        app: &AppHandle,
        profile: VlessReality,
    ) -> Result<EngineSnapshot, EngineError> {
        #[cfg(target_os = "ios")]
        {
            return self.restart_ios_tunnel(app, profile);
        }
        #[cfg(not(target_os = "ios"))]
        {
            self.restart_android_tunnel(app, profile)
        }
    }

    #[cfg_attr(target_os = "ios", allow(dead_code))]
    fn restart_android_tunnel(
        &self,
        app: &AppHandle,
        profile: VlessReality,
    ) -> Result<EngineSnapshot, EngineError> {
        #[cfg(not(target_os = "android"))]
        {
            let _ = profile;
            let _ = self.stop(app);
            return self.start(app);
        }

        #[cfg(target_os = "android")]
        {
            let (builder, warnings) = prepare_builder(app, profile.clone())?;
            let config = builder.build();
            let config_path = write_runtime_config(app, &config)?;
            let workdir = config_path
                .parent()
                .ok_or_else(|| EngineError::Config("config path has no parent".into()))?
                .to_path_buf();
            let config_arg = config_path.to_string_lossy().into_owned();
            let sidecar = sidecar_path(app)?;
            let asset_dir = ruleset::ensure_geo_assets(
                &route_store::geo_asset_dir(app)?,
                &route_store::bundled_geo_dirs(app),
            )?;
            validate_config(&sidecar, &config_path, &asset_dir)?;

            {
                let mut inner = lock_engine(&self.inner)?;
                inner.reconcile(app);
                inner.machine.begin_reload()?;
                publish_status(app, inner.machine.status());
                inner.last_error = None;
                inner.process.clear_plugin_owned();
            }

            if let Err(err) = start_android_sidecar(
                app,
                &profile.server,
                &config_path,
                &workdir,
                &asset_dir,
                &sidecar,
            ) {
                let mut inner = lock_engine(&self.inner)?;
                return abort_engine(&mut inner, app, &err.to_string(), false);
            }

            {
                let mut inner = lock_engine(&self.inner)?;
                if inner.machine.status() != EngineStatus::Starting {
                    return Ok(inner.snapshot());
                }
                inner.process.mark_plugin_owned();
                inner.process.attach_log_path(workdir.join(LOG_FILE_NAME));
            }

            let _ = config_arg;
            self.wait_until_running(app, &sidecar, &warnings, false)
        }
    }

    #[cfg(target_os = "ios")]
    fn restart_ios_tunnel(
        &self,
        app: &AppHandle,
        profile: VlessReality,
    ) -> Result<EngineSnapshot, EngineError> {
        #[cfg(not(target_os = "ios"))]
        {
            let _ = (self, app, profile);
            Err(EngineError::Process(
                "ios tunnel restart is unavailable on this platform".into(),
            ))
        }
        #[cfg(target_os = "ios")]
        {
            let (builder, warnings) = prepare_builder(app, profile.clone())?;
            let mut config = builder.build();
            let shared = super::ios::shared_dir(app)?;
            let log_path = shared.join("run").join(LOG_FILE_NAME);
            config["log"]["error"] = serde_json::json!(log_path);
            config["log"]["access"] = serde_json::json!("none");
            let config_path = write_runtime_config(app, &config)?;
            let asset_src = ruleset::ensure_geo_assets(
                &route_store::geo_asset_dir(app)?,
                &route_store::bundled_geo_dirs(app),
            )?;
            let _ = super::ios::clear_marker(&shared, super::ios::READY_FILE);
            write_ios_session(
                app,
                &profile.server,
                profile.server_port,
                &config_path,
                &asset_src,
            )?;
            {
                let mut inner = lock_engine(&self.inner)?;
                inner.reconcile(app);
                inner.machine.begin_reload()?;
                publish_status(app, inner.machine.status());
                inner.last_error = None;
                inner.process.mark_plugin_owned();
                inner.process.attach_log_path(log_path);
            }
            if let Err(err) = super::ios::reload(app) {
                let mut inner = lock_engine(&self.inner)?;
                return abort_engine(&mut inner, app, &err.to_string(), false);
            }
            let sidecar = PathBuf::from("ios-extension");
            self.wait_until_running(app, &sidecar, &warnings, false)
        }
    }

    fn reload_outbound(
        &self,
        app: &AppHandle,
        profile: VlessReality,
    ) -> Result<EngineSnapshot, EngineError> {
        let (builder, _warnings) = prepare_builder(app, profile.clone())?;
        let config = builder.build();

        let run_dir = runtime_config_path(app)?;
        let next_path = run_dir.with_file_name("config.next.json");
        write_json(&next_path, &config)?;

        let sidecar = sidecar_path(app)?;
        let asset_dir = ruleset::ensure_geo_assets(
            &route_store::geo_asset_dir(app)?,
            &route_store::bundled_geo_dirs(app),
        )?;
        validate_config(&sidecar, &next_path, &asset_dir)?;

        #[cfg(target_os = "macos")]
        if super::settings::load_engine_settings(app)?.kill_switch {
            super::macos::allow_server(&profile.server, profile.server_port)?;
        }

        let rules_path = run_dir.with_file_name("routing.next.json");
        write_json(
            &rules_path,
            &serde_json::json!({ "routing": config["routing"].clone() }),
        )?;
        super::xray_api::add_rules(&sidecar, API_LISTEN, &rules_path, &asset_dir)?;

        super::xray_api::remove_outbound(&sidecar, API_LISTEN, PROXY_OUTBOUND_TAG, &asset_dir)?;

        let outbound_path = run_dir.with_file_name("outbound.next.json");
        write_json(&outbound_path, &outbound_payload(&profile))?;
        super::xray_api::add_outbound(&sidecar, API_LISTEN, &outbound_path, &asset_dir)?;

        fs::copy(&next_path, &run_dir)?;
        let traffic = super::xray_api::query_proxy_traffic(&sidecar, API_LISTEN, &asset_dir)
            .unwrap_or_default();
        {
            let mut inner = lock_engine(&self.inner)?;
            inner.begin_session_from(traffic.uplink, traffic.downlink);
        }
        self.status(app)
    }

    pub fn session_stats(&self, app: &AppHandle) -> Result<SessionTraffic, EngineError> {
        let mut inner = lock_engine(&self.inner)?;
        inner.reconcile(app);
        if inner.machine.status() != EngineStatus::Running {
            return Ok(SessionTraffic {
                uplink: 0,
                downlink: 0,
                total: 0,
                started_at_ms: None,
            });
        }
        if inner.session_started_at.is_none() {
            inner.begin_session();
        }
        let started_at_ms = inner.session_started_at.and_then(system_time_ms);
        let baseline_up = inner.baseline_up;
        let baseline_down = inner.baseline_down;
        drop(inner);

        #[cfg(target_os = "ios")]
        {
            let stats = super::ios::shared_dir(app)
                .map(|shared| super::ios::read_stats(&shared))
                .unwrap_or_default();
            let uplink = stats.uplink.saturating_sub(baseline_up);
            let downlink = stats.downlink.saturating_sub(baseline_down);
            return Ok(SessionTraffic {
                uplink,
                downlink,
                total: uplink.saturating_add(downlink),
                started_at_ms,
            });
        }
        #[cfg(not(target_os = "ios"))]
        {
            let sidecar = sidecar_path(app)?;
            let asset_dir = ruleset::ensure_geo_assets(
                &route_store::geo_asset_dir(app)?,
                &route_store::bundled_geo_dirs(app),
            )?;
            let traffic = super::xray_api::query_proxy_traffic(&sidecar, API_LISTEN, &asset_dir)?;
            let uplink = traffic.uplink.saturating_sub(baseline_up);
            let downlink = traffic.downlink.saturating_sub(baseline_down);
            Ok(SessionTraffic {
                uplink,
                downlink,
                total: uplink.saturating_add(downlink),
                started_at_ms,
            })
        }
    }
}

fn publish_status(app: &AppHandle, status: EngineStatus) {
    crate::tray::sync_icons(app, status);
}

impl EngineInner {
    fn begin_session(&mut self) {
        self.begin_session_from(0, 0);
    }

    fn begin_session_from(&mut self, uplink: u64, downlink: u64) {
        self.session_started_at = Some(SystemTime::now());
        self.baseline_up = uplink;
        self.baseline_down = downlink;
    }

    fn end_session(&mut self) {
        self.session_started_at = None;
        self.baseline_up = 0;
        self.baseline_down = 0;
    }

    fn reconcile(&mut self, app: &AppHandle) {
        if self.machine.status() == EngineStatus::Running && !self.process.is_alive() {
            self.machine.mark_failed();
            self.end_session();
            publish_status(app, self.machine.status());
            let tail = self.process.last_log();
            self.last_error = Some(if tail.is_empty() {
                "xray exited unexpectedly".into()
            } else {
                tail.to_string()
            });
            return;
        }
        if self.machine.status() == EngineStatus::Running {
            self.process.refresh_log();
            if let Some(code) = handshake_failure_code(self.process.last_log()) {
                self.last_error = Some(code.to_string());
            }
        }
    }

    fn snapshot(&self) -> EngineSnapshot {
        EngineSnapshot {
            status: self.machine.status(),
            pid: self.process.pid(),
            last_error: self.last_error.clone(),
        }
    }
}

fn running_last_error(log: &str, warnings: &[String]) -> Option<String> {
    if let Some(code) = handshake_failure_code(log) {
        return Some(code.to_string());
    }
    if warnings.is_empty() {
        None
    } else {
        Some(warnings.join("; "))
    }
}

fn abort_engine(
    inner: &mut EngineInner,
    app: &AppHandle,
    message: &str,
    teardown_vpn: bool,
) -> Result<EngineSnapshot, EngineError> {
    let _ = inner.process.stop();
    if teardown_vpn {
        teardown_mobile_vpn(app);
    }
    release_network_guards(app);
    fail_start(inner, app, message)
}

fn hold_kill_switch(app: &AppHandle) -> Result<(), EngineError> {
    if !super::settings::load_engine_settings(app)?.kill_switch {
        return Ok(());
    }
    #[cfg(windows)]
    {
        super::kill_switch::apply(app)?;
    }
    #[cfg(target_os = "macos")]
    {
        super::macos::set_kill_switch(true)?;
    }
    #[cfg(target_os = "ios")]
    {
        super::ios::set_kill_switch(app, true)?;
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "ios")))]
    {
        let _ = app;
    }
    Ok(())
}

fn release_kill_switch(app: &AppHandle) {
    #[cfg(windows)]
    super::kill_switch::release_leftover(app);
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

/// The kill switch and the leak guard are released together. Both exist only
/// while a tunnel is expected, and leaving either behind strands the machine —
/// firewall rules survive a reboot.
fn release_network_guards(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    super::macos::release_guards();
    release_kill_switch(app);
    super::leak_guard::release();
}

/// A crash is not a user-requested stop, so the guards stay up and the kill
/// switch is re-asserted rather than released. `sync_kill_switch` only applies
/// while `Running`, so a toggle flipped at the wrong moment can otherwise leave
/// the setting on with no rules behind it.
fn hold_guards_after_crash(app: &AppHandle) {
    let kill_switch = super::settings::load_engine_settings(app)
        .map(|settings| settings.kill_switch)
        .unwrap_or(false);
    if !super::kill_switch::should_hold_tun(kill_switch, false) {
        return;
    }
    #[cfg(windows)]
    if let Err(err) = super::kill_switch::apply(app) {
        log_connect_error(&format!("kill switch hold after crash failed: {err}"));
    }
    #[cfg(target_os = "macos")]
    if let Err(err) = super::macos::set_kill_switch(true) {
        log_connect_error(&format!("kill switch hold after crash failed: {err}"));
    }
}

fn sync_kill_switch(app: &AppHandle, enabled: bool) -> Result<(), EngineError> {
    #[cfg(windows)]
    {
        if !enabled {
            return super::kill_switch::release(app);
        }
        let running = app
            .try_state::<Engine>()
            .and_then(|engine| engine.status(app).ok())
            .is_some_and(|snapshot| snapshot.status == EngineStatus::Running);
        if running {
            super::kill_switch::apply(app)?;
        }
    }
    #[cfg(target_os = "android")]
    {
        super::android::set_kill_switch(app, enabled)?;
    }
    #[cfg(target_os = "ios")]
    {
        super::ios::set_kill_switch(app, enabled)?;
        return sync_ios_on_demand(app);
    }
    #[cfg(target_os = "macos")]
    {
        if !enabled {
            return super::macos::set_kill_switch(false);
        }
        let running = app
            .try_state::<Engine>()
            .and_then(|engine| engine.status(app).ok())
            .is_some_and(|snapshot| snapshot.status == EngineStatus::Running);
        if running {
            super::macos::set_kill_switch(true)?;
        }
    }
    #[cfg(not(any(windows, target_os = "android", target_os = "macos", target_os = "ios")))]
    {
        let _ = (app, enabled);
    }
    Ok(())
}

fn fail_start(
    inner: &mut EngineInner,
    app: &AppHandle,
    message: &str,
) -> Result<EngineSnapshot, EngineError> {
    inner.machine.mark_failed();
    inner.end_session();
    publish_status(app, inner.machine.status());
    inner.process.refresh_log();
    let mut detail = message.to_string();
    let tail = inner.process.last_log();
    if !tail.is_empty() {
        detail = format!("{detail}: {tail}");
    }
    inner.last_error = Some(detail.clone());
    log_connect_error(&detail);
    Err(EngineError::Process(detail))
}

fn log_connect_error(detail: &str) {
    eprintln!("xraybox last_error: {detail}");
    super::app_log::append(&format!("xraybox last_error: {detail}"));
    #[cfg(target_os = "android")]
    super::android::log_error(&format!("xraybox last_error: {detail}"));
}

fn resolve_start_sidecar(
    app: &AppHandle,
    extra_env: &mut Vec<(String, String)>,
) -> Result<PathBuf, EngineError> {
    let _ = extra_env;
    #[cfg(target_os = "android")]
    {
        return super::android::sidecar_path(app);
    }
    #[cfg(target_os = "ios")]
    {
        let _ = app;
        return Ok(PathBuf::from("ios-extension"));
    }
    #[cfg(windows)]
    {
        let sidecar = sidecar_path(app)?;
        require_wintun_beside_sidecar(&sidecar)?;
        return Ok(sidecar);
    }
    #[cfg(not(any(windows, target_os = "android", target_os = "ios")))]
    {
        sidecar_path(app)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwitchStrategy {
    KeepVpnRestartXray,
    ApiReload,
    FullRestart,
}

fn switch_strategy(persist: bool) -> SwitchStrategy {
    switch_strategy_for(persist, cfg!(any(target_os = "android", target_os = "ios")))
}

fn switch_strategy_for(persist: bool, keep_os_tunnel: bool) -> SwitchStrategy {
    // Android and iOS own the OS tunnel outside this process. A new server
    // still has to be excluded from that tunnel, so the core is restarted
    // while the tunnel stays up: traffic is blackholed, not leaked.
    if keep_os_tunnel {
        SwitchStrategy::KeepVpnRestartXray
    } else if persist {
        SwitchStrategy::ApiReload
    } else {
        SwitchStrategy::FullRestart
    }
}

fn android_exclude_hosts(
    server: &str,
    dns: &DnsSettings,
    extra: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut exclude = Vec::with_capacity(2 + dns.remote_servers.len());
    exclude.push(server.to_string());
    exclude.extend(dns.remote_servers.iter().cloned());
    for host in extra {
        let host = host.trim();
        if host.is_empty() {
            continue;
        }
        if !exclude.iter().any(|existing| existing == host) {
            exclude.push(host.to_string());
        }
    }
    exclude
}

#[cfg(target_os = "android")]
fn android_dev_exclude_hosts(app: &AppHandle) -> Vec<String> {
    app.config()
        .build
        .dev_url
        .as_ref()
        .and_then(|url| url.host_str().map(str::to_string))
        .filter(|host| !matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1"))
        .into_iter()
        .collect()
}

fn start_android_sidecar(
    app: &AppHandle,
    server: &str,
    config_path: &Path,
    workdir: &Path,
    asset_dir: &Path,
    sidecar: &Path,
) -> Result<(), EngineError> {
    #[cfg(target_os = "android")]
    {
        let dns = dns_store::load_dns_settings(app)?;
        let exclude = android_exclude_hosts(server, &dns, android_dev_exclude_hosts(app));
        let kill_switch = super::settings::load_engine_settings(app)?.kill_switch;
        super::android::start_vpn(
            app,
            super::android::AndroidStart {
                dns: dns.remote_servers,
                exclude_hosts: exclude,
                config_path: config_path.to_string_lossy().into_owned(),
                workdir: workdir.to_string_lossy().into_owned(),
                asset_dir: asset_dir.to_string_lossy().into_owned(),
                sidecar_path: sidecar.to_string_lossy().into_owned(),
                kill_switch,
            },
        )
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, server, config_path, workdir, asset_dir, sidecar);
        Ok(())
    }
}

fn adopt_android_vpn(inner: &mut EngineInner, app: &AppHandle) {
    #[cfg(target_os = "android")]
    {
        let up = super::android::is_running(app).unwrap_or(false);
        match (inner.machine.status(), up) {
            (EngineStatus::Stopped | EngineStatus::Failed, true) => {
                inner.process.mark_plugin_owned();
                inner.machine.adopt_running();
                if inner.session_started_at.is_none() {
                    inner.begin_session();
                }
                publish_status(app, inner.machine.status());
                inner.last_error = None;
            }
            (EngineStatus::Running | EngineStatus::Starting, false) => {
                inner.process.clear_plugin_owned();
                inner.machine.mark_failed();
                publish_status(app, inner.machine.status());
                inner.last_error = Some("vpn service is not running".into());
            }
            _ => {}
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (inner, app);
    }
}

fn adopt_ios_vpn(inner: &mut EngineInner, app: &AppHandle) {
    #[cfg(target_os = "ios")]
    {
        let up = super::ios::is_running(app).unwrap_or(false);
        match (inner.machine.status(), up) {
            (EngineStatus::Stopped | EngineStatus::Failed, true) => {
                inner.process.mark_plugin_owned();
                if let Ok(shared) = super::ios::shared_dir(app) {
                    inner
                        .process
                        .attach_log_path(shared.join("run").join(LOG_FILE_NAME));
                }
                inner.machine.adopt_running();
                if inner.session_started_at.is_none() {
                    inner.begin_session();
                }
                publish_status(app, inner.machine.status());
                inner.last_error = None;
            }
            (EngineStatus::Running, false) => {
                inner.process.clear_plugin_owned();
                inner.machine.mark_stopped();
                inner.end_session();
                publish_status(app, inner.machine.status());
                inner.last_error = None;
            }
            _ => {}
        }
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (inner, app);
    }
}

fn begin_ios_user_stop(app: &AppHandle) {
    #[cfg(target_os = "ios")]
    {
        if let Ok(shared) = super::ios::shared_dir(app) {
            let _ = super::ios::write_marker(&shared, super::ios::USER_STOP_FILE);
            let _ = super::ios::clear_marker(&shared, super::ios::DESIRED_FILE);
        }
        let _ = super::ios::set_on_demand(app, false);
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = app;
    }
}

fn note_ios_connected(app: &AppHandle) {
    #[cfg(target_os = "ios")]
    {
        let Ok(shared) = super::ios::shared_dir(app) else {
            return;
        };
        let _ = super::ios::write_marker(&shared, super::ios::DESIRED_FILE);
        let _ = super::ios::write_marker(&shared, super::ios::LAST_FILE);
        let _ = sync_ios_on_demand(app);
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = app;
    }
}

#[cfg(target_os = "ios")]
fn sync_ios_on_demand(app: &AppHandle) -> Result<(), EngineError> {
    let settings = super::settings::load_engine_settings(app)?;
    let shared = super::ios::shared_dir(app)?;
    let desired = super::ios::has_marker(&shared, super::ios::DESIRED_FILE);
    let enabled = super::ios::ios_on_demand_enabled(
        false,
        settings.kill_switch,
        desired,
        settings.start_on_boot,
    );
    super::ios::set_on_demand(app, enabled)
}

#[cfg(target_os = "ios")]
fn write_ios_session(
    app: &AppHandle,
    server: &str,
    server_port: u16,
    config_path: &Path,
    asset_src: &Path,
) -> Result<super::ios::IosSession, EngineError> {
    let shared = super::ios::shared_dir(app)?;
    let asset_dir = shared.join("assets");
    super::ios::copy_geo_assets(asset_src, &asset_dir)?;
    let settings = super::settings::load_engine_settings(app)?;
    let dns = crate::dns::store::load_dns_settings(app)?;
    let dev_host = app
        .config()
        .build
        .dev_url
        .as_ref()
        .and_then(|url| url.host_str().map(str::to_string));
    let session = super::ios::IosSession {
        config_path: config_path.to_string_lossy().into_owned(),
        asset_dir: asset_dir.to_string_lossy().into_owned(),
        log_path: shared
            .join("run")
            .join(LOG_FILE_NAME)
            .to_string_lossy()
            .into_owned(),
        server_host: server.to_string(),
        server_port,
        exclude_hosts: super::ios::ios_exclude_hosts(
            server,
            super::ios::ios_dev_exclude_hosts(dev_host.as_deref()),
        ),
        dns: dns.remote_servers,
        kill_switch: settings.kill_switch,
        socks_port: super::ios::SOCKS_PORT,
        on_demand: super::ios::ios_on_demand_enabled(
            false,
            settings.kill_switch,
            true,
            settings.start_on_boot,
        ),
    };
    super::ios::write_session(&shared, &session)?;
    let _ = super::ios::clear_marker(&shared, super::ios::READY_FILE);
    Ok(session)
}

fn start_ios_tunnel(
    app: &AppHandle,
    server: &str,
    server_port: u16,
    config_path: &Path,
    asset_src: &Path,
) -> Result<(), EngineError> {
    #[cfg(target_os = "ios")]
    {
        let session = write_ios_session(app, server, server_port, config_path, asset_src)?;
        super::ios::establish(app, session.kill_switch, session.on_demand)
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (app, server, server_port, config_path, asset_src);
        Ok(())
    }
}

fn teardown_mobile_vpn(app: &AppHandle) {
    #[cfg(target_os = "android")]
    {
        if let Err(err) = super::android::teardown(app) {
            eprintln!("xraybox: android vpn teardown failed: {err}");
        }
    }
    #[cfg(target_os = "ios")]
    {
        if let Err(err) = super::ios::teardown(app) {
            eprintln!("xraybox: ios vpn teardown failed: {err}");
        }
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = app;
    }
}

fn classify_probe(outcome: ProbeOutcome) -> Option<&'static str> {
    match outcome {
        ProbeOutcome::Ok => None,
        ProbeOutcome::Timeout => Some("tunnel_no_response"),
        ProbeOutcome::Refused => Some("tunnel_refused"),
        ProbeOutcome::Hijacked => Some("tunnel_hijacked"),
    }
}

fn system_time_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_millis() as u64)
}

fn lookup_public_ip_sync() -> Result<PublicIpInfo, EngineError> {
    let response = ureq::get(TRACE_URL)
        .timeout(PROBE_TIMEOUT)
        .call()
        .map_err(|err| EngineError::Process(format!("public ip lookup failed: {err}")))?;
    let body = response
        .into_string()
        .map_err(|err| EngineError::Process(format!("public ip lookup failed: {err}")))?;
    let info = parse_cdn_trace(&body);
    if info.ip.is_none() {
        return Err(EngineError::Process(
            "public ip lookup returned no ip field".into(),
        ));
    }
    Ok(info)
}

pub fn parse_cdn_trace(body: &str) -> PublicIpInfo {
    let mut ip = None;
    let mut country_code = None;
    for line in body.lines() {
        if let Some(value) = line.strip_prefix("ip=") {
            let value = value.trim();
            if !value.is_empty() {
                ip = Some(value.to_string());
            }
        } else if let Some(value) = line.strip_prefix("loc=") {
            let value = value.trim();
            if value.len() == 2 && value.chars().all(|ch| ch.is_ascii_alphabetic()) {
                country_code = Some(value.to_ascii_lowercase());
            }
        }
    }
    PublicIpInfo { ip, country_code }
}

fn probe_tunnel() -> ProbeOutcome {
    for attempt in 0..PROBE_ATTEMPTS {
        match ureq::get(PROBE_URL).timeout(PROBE_TIMEOUT).call() {
            Ok(response) if response.status() == 204 => return ProbeOutcome::Ok,
            Ok(_) => return ProbeOutcome::Hijacked,
            Err(_) if attempt + 1 < PROBE_ATTEMPTS => {
                std::thread::sleep(Duration::from_millis(300));
            }
            Err(err) => return outcome_from(err),
        }
    }
    ProbeOutcome::Timeout
}

fn outcome_from(err: ureq::Error) -> ProbeOutcome {
    match err {
        ureq::Error::Status(_, _) => ProbeOutcome::Hijacked,
        ureq::Error::Transport(transport) => {
            let detail = transport.to_string().to_ascii_lowercase();
            match transport.kind() {
                ureq::ErrorKind::ConnectionFailed => ProbeOutcome::Refused,
                _ if detail.contains("refused") => ProbeOutcome::Refused,
                _ if detail.contains("timed out") || detail.contains("timeout") => {
                    ProbeOutcome::Timeout
                }
                _ => ProbeOutcome::Timeout,
            }
        }
    }
}

fn log_leftover_tun() {
    #[cfg(windows)]
    if super::windows::tun_interface_present() {
        eprintln!("xraybox-tun adapter still present after xray stop");
    }
    #[cfg(target_os = "macos")]
    if super::macos::tun_present() {
        eprintln!("utun10 still present after xray stop");
    }
}

fn lock_engine(
    inner: &Mutex<EngineInner>,
) -> Result<std::sync::MutexGuard<'_, EngineInner>, EngineError> {
    inner
        .lock()
        .map_err(|_| EngineError::Process("engine lock poisoned".into()))
}

fn prepare_builder(
    app: &AppHandle,
    profile: VlessReality,
) -> Result<(ConfigBuilder, Vec<String>), EngineError> {
    let dns = dns_store::load_dns_settings(app)?;
    dns.validate()?;
    let mut route = route_store::load_route_settings(app)?;
    route.bypass_lan = true;
    let packs = route_store::load_packs(app)?;
    let cache_dir = route_store::geo_asset_dir(app)?;
    let bundle_dirs = route_store::bundled_geo_dirs(app);
    let bound = bind_route(packs, &cache_dir, &bundle_dirs)?;
    route.extra_rules = bound.extra_rules;
    let log_level = match super::settings::load_engine_settings(app) {
        Ok(settings) if settings.logs_enabled => "info",
        _ => "warning",
    };
    Ok((
        ConfigBuilder::default()
            .with_log_level(log_level)
            .with_proxy(profile)
            .with_tun()
            .with_dns(dns)
            .with_local_dns_domains(bound.local_dns_domains)
            .with_route(route)
            .with_mixed_inbound_if_enabled(),
        bound.warnings,
    ))
}

fn outbound_payload(profile: &VlessReality) -> serde_json::Value {
    serde_json::json!({
        "outbounds": [profile.to_outbound(PROXY_OUTBOUND_TAG)]
    })
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), EngineError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(value)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn write_runtime_config(
    app: &AppHandle,
    value: &serde_json::Value,
) -> Result<PathBuf, EngineError> {
    let path = runtime_config_path(app)?;
    write_json(&path, value)?;
    let _ = fs::remove_file(path.with_file_name("clash.secret"));
    Ok(path)
}

fn runtime_config_path(app: &AppHandle) -> Result<PathBuf, EngineError> {
    #[cfg(target_os = "ios")]
    {
        return Ok(super::ios::shared_dir(app)?.join("run").join("config.json"));
    }
    #[cfg(not(target_os = "ios"))]
    {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|err| EngineError::Config(err.to_string()))?;
        Ok(dir.join("run").join("config.json"))
    }
}

fn runtime_xray_log_path(app: &AppHandle) -> Result<PathBuf, EngineError> {
    let config = runtime_config_path(app)?;
    let dir = config
        .parent()
        .ok_or_else(|| EngineError::Config("config path has no parent".into()))?;
    Ok(dir.join(LOG_FILE_NAME))
}

fn block_on_engine<T, F>(
    app: AppHandle,
    work: F,
) -> impl std::future::Future<Output = Result<T, String>>
where
    T: Send + 'static,
    F: FnOnce(&Engine) -> Result<T, EngineError> + Send + 'static,
{
    async move {
        tauri::async_runtime::spawn_blocking(move || {
            let engine = app.state::<Engine>();
            work(&engine).map_err(|err| err.to_string())
        })
        .await
        .map_err(|err| err.to_string())?
    }
}

#[tauri::command]
pub async fn start_engine(app: AppHandle) -> Result<EngineSnapshot, String> {
    let app_for_start = app.clone();
    block_on_engine(app, move |engine| engine.start(&app_for_start)).await
}

#[tauri::command]
pub async fn stop_engine(app: AppHandle) -> Result<EngineSnapshot, String> {
    let app_for_stop = app.clone();
    block_on_engine(app, move |engine| engine.stop(&app_for_stop)).await
}

#[tauri::command]
pub async fn engine_status(app: AppHandle) -> Result<EngineSnapshot, String> {
    let app_for_status = app.clone();
    block_on_engine(app, move |engine| engine.status(&app_for_status)).await
}

#[tauri::command]
pub async fn session_stats(app: AppHandle) -> Result<SessionTraffic, String> {
    let app_for_stats = app.clone();
    block_on_engine(app, move |engine| engine.session_stats(&app_for_stats)).await
}

#[tauri::command]
pub async fn lookup_public_ip() -> Result<PublicIpInfo, String> {
    tauri::async_runtime::spawn_blocking(|| lookup_public_ip_sync().map_err(|err| err.to_string()))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn append_boot_log(message: String) {
    boot_trace(&format!("js {message}"));
}

static BOOT_SPLASH_CLAIMED: AtomicBool = AtomicBool::new(false);

/// True only for the first page load of this process. A reload of the
/// webview inside a live process (for example iOS reclaiming the web content
/// process while the app is in the background) gets false, so the frontend
/// can skip the splash and show the app right away.
#[tauri::command]
pub fn claim_boot_splash() -> bool {
    !BOOT_SPLASH_CLAIMED.swap(true, Ordering::SeqCst)
}

#[tauri::command]
pub fn connect_after_elevate() -> bool {
    super::windows::connect_after_elevate_requested()
}

#[tauri::command]
pub async fn get_engine_log(app: AppHandle, state: State<'_, Engine>) -> Result<String, String> {
    state.engine_log(&app).map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn get_app_log(app: AppHandle) -> Result<String, String> {
    Ok(super::app_log::read_app_log(&app))
}

fn boot_trace(message: &str) {
    eprintln!("xraybox-boot: {message}");
    super::app_log::append(&format!("xraybox-boot: {message}"));
    #[cfg(target_os = "android")]
    super::android::log_info(&format!("boot: {message}"));
}

#[tauri::command]
pub async fn save_manual_endpoint(app: AppHandle, profile: VlessReality) -> Result<(), String> {
    save_manual_profile(&app, &profile).map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn get_manual_endpoint(app: AppHandle) -> Result<Option<VlessReality>, String> {
    load_manual_profile(&app).map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn get_dns_settings(app: AppHandle) -> Result<DnsSettings, String> {
    dns_store::load_dns_settings(&app).map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn save_dns_settings(
    app: AppHandle,
    settings: DnsSettings,
) -> Result<DnsSettings, String> {
    dns_store::save_dns_settings(&app, &settings).map_err(|err| err.to_string())?;
    Ok(settings)
}

#[tauri::command]
pub async fn update_geo_assets(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = route_store::geo_asset_dir(&app).map_err(|err| err.to_string())?;
        ruleset::update_geo_assets(&dir).map_err(|err| err.to_string())?;
        Ok(())
    })
    .await
    .map_err(|err| err.to_string())?
}

pub fn spawn_background_ruleset_update(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = update_geo_assets(app).await {
            eprintln!("xraybox: geo asset background update failed: {err}");
        }
    });
}

#[tauri::command]
pub async fn get_route_packs(app: AppHandle) -> Result<Vec<PackState>, String> {
    route_store::load_packs(&app).map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn save_route_packs(
    app: AppHandle,
    packs: Vec<PackState>,
) -> Result<Vec<PackState>, String> {
    route_store::save_packs(&app, &packs).map_err(|err| err.to_string())?;
    Ok(packs)
}

#[tauri::command]
pub async fn switch_server(
    app: AppHandle,
    profile: VlessReality,
) -> Result<EngineSnapshot, String> {
    let app_for_switch = app.clone();
    block_on_engine(app, move |engine| {
        engine.switch_server(&app_for_switch, profile)
    })
    .await
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileListItem {
    pub id: String,
    pub remark: String,
    pub server: String,
    pub server_port: u16,
    pub country_code: String,
    pub latency_ms: Option<u32>,
    pub profile: VlessReality,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionState {
    pub subscription: SubscriptionSettings,
    pub profiles: Vec<ProfileListItem>,
}

fn profile_list_item(node: &ProfileNode) -> ProfileListItem {
    ProfileListItem {
        id: node.id.clone(),
        remark: node.profile.remark.clone(),
        server: node.profile.server.clone(),
        server_port: node.profile.server_port,
        country_code: infer_country_code(&node.profile.remark),
        latency_ms: latest_latency(&node.ping_ms),
        profile: node.profile.clone(),
    }
}

fn subscription_state_from(
    settings: SubscriptionSettings,
    nodes: Vec<ProfileNode>,
) -> SubscriptionState {
    SubscriptionState {
        subscription: settings,
        profiles: nodes.iter().map(profile_list_item).collect(),
    }
}

fn load_subscription_state(app: &AppHandle) -> Result<SubscriptionState, EngineError> {
    let mut settings = subscription::load_subscription(app)?;
    let nodes = subscription::load_nodes(app)?;
    sync_smart_selected_id(app, &mut settings, &nodes, tunnel_running(app))?;
    Ok(subscription_state_from(settings, nodes))
}

fn refresh_subscription(app: &AppHandle) -> Result<SubscriptionState, EngineError> {
    let settings = subscription::load_subscription(app)?;
    if settings.url.is_empty() {
        return Err(EngineError::Config("no subscription url saved".into()));
    }
    let document = subscription::fetch_subscription_document(&settings.url)?;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| EngineError::Config(err.to_string()))?;
    let (settings, nodes) =
        subscription::apply_fetched_body(&dir, &document.body, subscription::now_unix())?;
    crate::community::persist_fetched_api(app, document.api.as_ref())?;
    Ok(subscription_state_from(settings, nodes))
}

#[tauri::command]
pub async fn save_subscription_url(
    app: AppHandle,
    url: String,
) -> Result<SubscriptionState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = app.path().app_data_dir().map_err(|err| err.to_string())?;
        subscription::save_url_to(&dir, &url).map_err(|err| err.to_string())?;
        refresh_subscription(&app).map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn get_subscription(app: AppHandle) -> Result<SubscriptionState, String> {
    load_subscription_state(&app).map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn fetch_subscription(app: AppHandle) -> Result<SubscriptionState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        refresh_subscription(&app).map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn list_profiles(app: AppHandle) -> Result<Vec<ProfileListItem>, String> {
    let nodes = subscription::load_nodes(&app).map_err(|err| err.to_string())?;
    Ok(nodes.iter().map(profile_list_item).collect())
}

#[tauri::command]
pub async fn select_profile(app: AppHandle, id: String) -> Result<SubscriptionState, String> {
    let mut settings = subscription::load_subscription(&app).map_err(|err| err.to_string())?;
    let nodes = subscription::load_nodes(&app).map_err(|err| err.to_string())?;
    if !nodes.iter().any(|node| node.id == id) {
        return Err("profile not found".into());
    }
    settings.selected_profile_id = Some(id);
    subscription::save_subscription(&app, &settings).map_err(|err| err.to_string())?;
    Ok(subscription_state_from(settings, nodes))
}

fn tunnel_running(app: &AppHandle) -> bool {
    app.try_state::<Engine>()
        .and_then(|engine| engine.status(app).ok())
        .is_some_and(|snapshot| snapshot.status == EngineStatus::Running)
}

fn kill_switch_enabled(app: &AppHandle) -> bool {
    super::settings::load_engine_settings(app)
        .map(|settings| settings.kill_switch)
        .unwrap_or(false)
}

#[tauri::command]
pub async fn ping_profiles(app: AppHandle) -> Result<SubscriptionState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut nodes = subscription::load_nodes(&app).map_err(|err| err.to_string())?;
        if ping::probes_are_deliverable(tunnel_running(&app), kill_switch_enabled(&app)) {
            ping::ping_nodes(&mut nodes);
            subscription::save_nodes(&app, &nodes).map_err(|err| err.to_string())?;
        }
        let mut settings = subscription::load_subscription(&app).map_err(|err| err.to_string())?;
        sync_smart_selected_id(&app, &mut settings, &nodes, tunnel_running(&app))
            .map_err(|err| err.to_string())?;
        Ok(subscription_state_from(settings, nodes))
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn clear_profiles(app: AppHandle) -> Result<SubscriptionState, String> {
    let dir = app.path().app_data_dir().map_err(|err| err.to_string())?;
    let (settings, nodes) =
        subscription::clear_profiles_from(&dir).map_err(|err| err.to_string())?;
    Ok(subscription_state_from(settings, nodes))
}

#[tauri::command]
pub async fn get_community_usage(
    app: AppHandle,
) -> Result<Option<crate::community::CommunityUsage>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::community::usage(&app).map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn verify_community(
    app: AppHandle,
) -> Result<Option<crate::community::CommunityVerify>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::community::verify(&app).map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn set_fetch_interval(
    app: AppHandle,
    interval: FetchInterval,
) -> Result<SubscriptionSettings, String> {
    let mut settings = subscription::load_subscription(&app).map_err(|err| err.to_string())?;
    settings.fetch_interval = interval;
    subscription::save_subscription(&app, &settings).map_err(|err| err.to_string())?;
    Ok(settings)
}

#[tauri::command]
pub async fn get_engine_settings(app: AppHandle) -> Result<EngineSettings, String> {
    super::settings::load_engine_settings(&app).map_err(|err| err.to_string())
}

fn apply_start_on_boot(app: &AppHandle, enabled: bool) -> Result<(), EngineError> {
    #[cfg(target_os = "android")]
    {
        return super::android::apply_start_on_boot(app, enabled);
    }
    #[cfg(target_os = "ios")]
    {
        let _ = enabled;
        return sync_ios_on_demand(app);
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = app;
        super::autostart::apply_start_on_boot(enabled)
    }
}

#[tauri::command]
pub async fn save_engine_settings(
    app: AppHandle,
    settings: EngineSettings,
) -> Result<EngineSettings, String> {
    let previous = super::settings::load_engine_settings(&app).map_err(|err| err.to_string())?;
    if settings.start_on_boot != previous.start_on_boot {
        apply_start_on_boot(&app, settings.start_on_boot).map_err(|err| err.to_string())?;
    }
    if settings.kill_switch != previous.kill_switch {
        sync_kill_switch(&app, settings.kill_switch).map_err(|err| err.to_string())?;
    }
    super::settings::save_engine_settings(&app, &settings).map_err(|err| err.to_string())?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_last_error_prefers_reality_over_warnings() {
        assert_eq!(
            running_last_error(
                "REALITY: processed invalid connection",
                &["missing geo file".into()]
            ),
            Some("reality_invalid_connection".into())
        );
        assert_eq!(
            running_last_error("xray started", &["missing geo file".into()]),
            Some("missing geo file".into())
        );
        assert_eq!(running_last_error("xray started", &[]), None);
    }

    fn sample_profile() -> VlessReality {
        VlessReality {
            remark: "edge".into(),
            server: "edge.example.com".into(),
            server_port: 443,
            uuid: "11111111-1111-1111-1111-111111111111".into(),
            server_name: "www.cloudflare.com".into(),
            public_key: "abc".into(),
            short_id: "dead".into(),
            flow: Some("xtls-rprx-vision".into()),
            fingerprint: "chrome".into(),
            spider_x: None,
            mldsa65_verify: None,
        }
    }

    #[test]
    fn probe_result_maps_to_codes() {
        assert_eq!(classify_probe(ProbeOutcome::Ok), None);
        assert_eq!(
            classify_probe(ProbeOutcome::Timeout),
            Some("tunnel_no_response")
        );
        assert_eq!(
            classify_probe(ProbeOutcome::Refused),
            Some("tunnel_refused")
        );
        assert_eq!(
            classify_probe(ProbeOutcome::Hijacked),
            Some("tunnel_hijacked")
        );
    }

    #[test]
    fn probe_url_is_a_204_endpoint_over_http() {
        // Plain HTTP + 204: no TLS handshake and no body to parse, so a captive
        // portal or an injected error page cannot read as success.
        assert!(PROBE_URL.starts_with("http://"));
    }

    #[test]
    fn parse_cdn_trace_reads_ip_and_country() {
        let info = parse_cdn_trace("fl=xxx\nip=1.2.3.4\nloc=MY\nwarp=off\n");
        assert_eq!(info.ip.as_deref(), Some("1.2.3.4"));
        assert_eq!(info.country_code.as_deref(), Some("my"));
    }

    #[test]
    fn parse_cdn_trace_ignores_invalid_loc() {
        let info = parse_cdn_trace("ip=10.0.0.1\nloc=XXX\n");
        assert_eq!(info.ip.as_deref(), Some("10.0.0.1"));
        assert_eq!(info.country_code, None);
    }

    #[test]
    fn add_outbound_payload_wraps_a_single_outbound() {
        let payload = outbound_payload(&sample_profile());
        let outbounds = payload["outbounds"].as_array().unwrap();
        assert_eq!(outbounds.len(), 1);
        assert_eq!(outbounds[0]["tag"], "proxy");
        assert_eq!(outbounds[0]["protocol"], "vless");
        assert!(payload.get("inbounds").is_none());
        assert!(payload.get("routing").is_none());
    }

    #[test]
    fn android_switch_keeps_vpn_even_when_persist_is_off() {
        assert_eq!(
            switch_strategy_for(false, true),
            SwitchStrategy::KeepVpnRestartXray
        );
        assert_eq!(
            switch_strategy_for(true, true),
            SwitchStrategy::KeepVpnRestartXray
        );
        assert_eq!(
            switch_strategy_for(false, false),
            SwitchStrategy::FullRestart
        );
        assert_eq!(switch_strategy_for(true, false), SwitchStrategy::ApiReload);
    }

    #[test]
    fn android_exclude_hosts_lists_server_then_remote_dns() {
        let dns = DnsSettings {
            remote_servers: vec!["1.1.1.1".into(), "8.8.8.8".into()],
            fallback_to_system: false,
        };
        assert_eq!(
            android_exclude_hosts("edge.example.com", &dns, None::<String>),
            vec!["edge.example.com", "1.1.1.1", "8.8.8.8"]
        );
        assert_eq!(
            android_exclude_hosts("edge.example.com", &dns, ["192.168.1.8".into()]),
            vec!["edge.example.com", "1.1.1.1", "8.8.8.8", "192.168.1.8"]
        );
    }
}
