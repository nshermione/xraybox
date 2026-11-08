use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use tauri::Manager;

use crate::engine::error::EngineError;

const LOG_TAIL_MAX: usize = 96 * 1024;
pub const LOG_FILE_NAME: &str = "xray.log";

pub struct EngineProcess {
    child: Option<Child>,
    log_path: Option<PathBuf>,
    last_log: String,
    plugin_owned: bool,
    /// macOS: xray was spawned by the root helper, so there is no local `Child`.
    helper_owned: bool,
}

impl Default for EngineProcess {
    fn default() -> Self {
        Self {
            child: None,
            log_path: None,
            last_log: String::new(),
            plugin_owned: false,
            helper_owned: false,
        }
    }
}

impl EngineProcess {
    pub fn start(
        &mut self,
        sidecar: &Path,
        args: &[&str],
        workdir: &Path,
        asset_dir: &Path,
    ) -> Result<(), EngineError> {
        self.start_with_env(sidecar, args, workdir, asset_dir, &[])
    }

    pub fn start_with_env(
        &mut self,
        sidecar: &Path,
        args: &[&str],
        workdir: &Path,
        asset_dir: &Path,
        extra_env: &[(String, String)],
    ) -> Result<(), EngineError> {
        if self.is_alive() {
            return Err(EngineError::Process(
                "xray process is already running".into(),
            ));
        }
        self.plugin_owned = false;
        self.helper_owned = false;

        fs::create_dir_all(workdir)?;
        let log_path = workdir.join(LOG_FILE_NAME);
        let log_file = fs::File::create(&log_path)?;
        // Xray v26.9.9 writes every log line to stdout and nothing to stderr
        // (startup banner, [Info], [Warning], and fatal config errors).
        // try_clone() duplicates the handle and shares the file pointer, so
        // both streams append to the same file instead of overwriting.
        let stdout_file = log_file.try_clone()?;

        let mut cmd = Command::new(sidecar);
        cmd.args(args)
            .current_dir(workdir)
            .env("XRAY_LOCATION_ASSET", asset_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(log_file));
        for (key, value) in extra_env {
            cmd.env(key, value);
        }
        if let Some(dir) = sidecar.parent() {
            cmd.env("LD_LIBRARY_PATH", dir);
        }
        inherit_tun_fd(&mut cmd, extra_env)?;
        crate::engine::windows::apply_hidden_console(&mut cmd);
        let child = cmd
            .spawn()
            .map_err(|err| EngineError::Process(format!("failed to spawn sidecar: {err}")))?;

        self.child = Some(child);
        self.log_path = Some(log_path);
        self.last_log.clear();
        Ok(())
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn mark_plugin_owned(&mut self) {
        self.plugin_owned = true;
    }

    pub fn mark_helper_owned(&mut self) {
        self.helper_owned = true;
        self.plugin_owned = false;
        self.child = None;
    }

    pub fn attach_log_path(&mut self, path: PathBuf) {
        self.log_path = Some(path);
    }

    #[allow(dead_code)]
    pub fn clear_plugin_owned(&mut self) {
        self.plugin_owned = false;
    }

    pub fn stop(&mut self) -> Result<(), EngineError> {
        self.plugin_owned = false;
        if self.helper_owned {
            self.helper_owned = false;
            #[cfg(target_os = "macos")]
            {
                return crate::engine::macos::stop_tunnel();
            }
            #[cfg(not(target_os = "macos"))]
            {
                return Ok(());
            }
        }
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };

        let pid = child.id();
        if let Err(err) = child.kill() {
            if err.kind() != std::io::ErrorKind::InvalidInput {
                self.try_taskkill(pid);
            }
        }

        let _ = child.wait();
        self.refresh_log_tail();

        if self.is_process_alive(pid) {
            self.try_taskkill(pid);
            let _ = child.wait();
        }

        Ok(())
    }

    pub fn is_alive(&mut self) -> bool {
        if self.helper_owned {
            #[cfg(target_os = "macos")]
            {
                return crate::engine::macos::sidecar_is_alive();
            }
            #[cfg(not(target_os = "macos"))]
            {
                return false;
            }
        }
        if self.plugin_owned {
            return true;
        }
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(Some(_)) => {
                self.refresh_log_tail();
                self.child = None;
                false
            }
            Ok(None) => true,
            Err(_) => {
                self.refresh_log_tail();
                self.child = None;
                false
            }
        }
    }

    pub fn pid(&self) -> Option<u32> {
        if self.helper_owned {
            #[cfg(target_os = "macos")]
            {
                return crate::engine::macos::sidecar_pid();
            }
            #[cfg(not(target_os = "macos"))]
            {
                return None;
            }
        }
        self.child.as_ref().map(Child::id)
    }

    pub fn last_log(&self) -> &str {
        &self.last_log
    }

    pub fn refresh_log(&mut self) {
        self.refresh_log_tail();
    }

    fn refresh_log_tail(&mut self) {
        let Some(path) = &self.log_path else {
            return;
        };
        match read_tail(path, LOG_TAIL_MAX) {
            Ok(tail) => self.last_log = tail,
            Err(err) => {
                self.last_log = format!("failed to read sidecar log: {err}");
            }
        }
    }

    fn is_process_alive(&self, pid: u32) -> bool {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("tasklist");
            cmd.args(["/FI", &format!("PID eq {pid}"), "/NH"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            crate::engine::windows::apply_hidden_console(&mut cmd);
            return cmd
                .output()
                .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
                .unwrap_or(false);
        }
        #[cfg(not(windows))]
        {
            let _ = pid;
            false
        }
    }

    fn try_taskkill(&self, pid: u32) {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("taskkill");
            cmd.args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            crate::engine::windows::apply_hidden_console(&mut cmd);
            let _ = cmd.status();
        }
        #[cfg(not(windows))]
        {
            let _ = pid;
        }
    }
}

fn inherit_tun_fd(cmd: &mut Command, extra_env: &[(String, String)]) -> Result<(), EngineError> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = cmd;
        let _ = extra_env;
        return Ok(());
    }
    #[cfg(target_os = "android")]
    {
        let Some((_, value)) = extra_env.iter().find(|(key, _)| key == "XRAY_TUN_FD") else {
            return Ok(());
        };
        let fd: i32 = value
            .parse()
            .map_err(|_| EngineError::Process(format!("XRAY_TUN_FD is not an fd: {value}")))?;
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags < 0 {
                return Err(EngineError::Process(format!(
                    "tun fd {fd} is not open: {}",
                    std::io::Error::last_os_error()
                )));
            }
            if libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                return Err(EngineError::Process(format!(
                    "failed to keep tun fd {fd} across exec: {}",
                    std::io::Error::last_os_error()
                )));
            }
        }
        // Forces fork+exec so posix_spawn cannot close every extra fd.
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        eprintln!("xraybox: inheriting tun fd {fd}");
        Ok(())
    }
}

impl Drop for EngineProcess {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub const REALITY_INVALID_CONNECTION: &str = "reality_invalid_connection";
pub const WINTUN_DLL_NAME: &str = "wintun.dll";

/// Wintun's loader searches only the directory of the running executable
/// image, plus System32. cwd and PATH are ignored, so the DLL must sit
/// beside `xray.exe` before we spawn.
pub fn require_wintun_beside_sidecar(sidecar: &Path) -> Result<(), EngineError> {
    let Some(dir) = sidecar.parent() else {
        return Err(EngineError::WintunMissing);
    };
    if dir.join(WINTUN_DLL_NAME).is_file() {
        Ok(())
    } else {
        Err(EngineError::WintunMissing)
    }
}

pub fn handshake_failure_code(log: &str) -> Option<&'static str> {
    // Source-justified from XTLS/Xray-core v26.9.9
    // transport/internet/reality/reality.go: a failed REALITY handshake
    // (wrong password / shortId, or the server diverted us to dest) logs
    // "REALITY: received real certificate..." then returns
    // "REALITY: processed invalid connection". Not captured from a live
    // session — see the cutover manual-verification list.
    if log.contains("REALITY: processed invalid connection")
        || log.contains("REALITY: received real certificate")
    {
        Some(REALITY_INVALID_CONNECTION)
    } else {
        None
    }
}

pub fn validate_config_args(config_path: &str) -> [&str; 4] {
    ["run", "-test", "-c", config_path]
}

pub fn run_config_args(config_path: &str) -> [&str; 3] {
    ["run", "-c", config_path]
}

pub fn validate_config(
    sidecar: &Path,
    config_path: &Path,
    asset_dir: &Path,
) -> Result<(), EngineError> {
    let path = config_path.to_string_lossy();
    let mut cmd = Command::new(sidecar);
    cmd.args(validate_config_args(&path))
        .env("XRAY_LOCATION_ASSET", asset_dir);
    super::windows::apply_hidden_console(&mut cmd);
    let output = cmd.output()?;
    if output.status.success() {
        return Ok(());
    }
    // v26.3.27 writes `run -test` failures to stdout; stderr is empty.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = format!("{stderr}\n{stdout}");
    Err(EngineError::Config(format!(
        "xray run -test failed: {}",
        detail.trim()
    )))
}

pub(crate) fn read_tail(path: &Path, max_bytes: usize) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len > max_bytes as u64 {
        file.seek(SeekFrom::End(-(max_bytes as i64)))?;
    }
    let mut buf = Vec::new();
    file.take(max_bytes as u64).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Name Tauri's externalBin lookup uses in dev and in `src-tauri/binaries/`.
/// The bundled resource copy drops the triple and is `xray` / `xray.exe`.
pub fn sidecar_file_name() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "xray-aarch64-apple-darwin"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "xray-x86_64-apple-darwin"
    }
    #[cfg(windows)]
    {
        "xray-x86_64-pc-windows-msvc.exe"
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        "xray"
    }
}

pub fn sidecar_path_from_resource_dir(resource_dir: &Path) -> PathBuf {
    let file_name = if cfg!(windows) { "xray.exe" } else { "xray" };
    resource_dir.join(file_name)
}

pub fn debug_sidecar_path(exe_dir: &Path) -> PathBuf {
    exe_dir.join(sidecar_file_name())
}

fn default_sidecar_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(debug_sidecar_path(dir));
            paths.push(sidecar_path_from_resource_dir(dir));
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    paths.push(manifest_dir.join("binaries").join(sidecar_file_name()));

    paths
}

pub fn resolve_sidecar_path() -> Result<PathBuf, EngineError> {
    first_existing_sidecar(default_sidecar_search_paths())
}

pub fn sidecar_path(app: &tauri::AppHandle) -> Result<PathBuf, EngineError> {
    #[cfg(target_os = "android")]
    {
        crate::engine::android::sidecar_path(app)
    }
    #[cfg(not(target_os = "android"))]
    {
        let mut candidates = Vec::new();

        if let Ok(resource) = app.path().resource_dir() {
            candidates.push(sidecar_path_from_resource_dir(&resource));
            candidates.push(debug_sidecar_path(&resource));
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(debug_sidecar_path(dir));
                candidates.push(sidecar_path_from_resource_dir(dir));
            }
        }

        candidates.extend(default_sidecar_search_paths());
        first_existing_sidecar(candidates)
    }
}

fn first_existing_sidecar(
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Result<PathBuf, EngineError> {
    for path in candidates {
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(EngineError::SidecarMissing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::config::ConfigBuilder;
    use crate::engine::error::WINTUN_DLL_MISSING;
    use crate::engine::xray_api::{wait_api, ApiWait};
    use std::time::Duration;

    #[test]
    #[ignore = "sidecar integration; spawns xray to prove stdout reaches xray.log"]
    fn engine_process_captures_xray_stdout_in_xray_log() {
        let sidecar = resolve_sidecar_path().expect("sidecar missing; run scripts/fetch-xray.ps1");
        let workdir = std::env::temp_dir().join(format!(
            "xraybox-xray-log-capture-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&workdir).unwrap();
        let config_path = workdir.join("config.json");
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let listen = format!("127.0.0.1:{port}");
        // "API server listening" is [Info]; the production default is warning.
        let mut value = ConfigBuilder::default().with_log_level("info").build();
        value["api"]["listen"] = serde_json::Value::String(listen.clone());
        fs::write(&config_path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        let asset_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("assets");
        let config_arg = config_path.to_string_lossy().into_owned();
        let mut process = EngineProcess::default();
        process
            .start(
                &sidecar,
                &run_config_args(&config_arg),
                &workdir,
                &asset_dir,
            )
            .expect("spawn xray");

        let ready = wait_api(&listen, Duration::from_secs(8), || process.is_alive());
        process.refresh_log();
        let log = fs::read_to_string(workdir.join(LOG_FILE_NAME)).unwrap_or_default();
        let stale_name = workdir.join("xray.stderr.log").exists();
        let _ = process.stop();
        let _ = fs::remove_dir_all(&workdir);

        assert_eq!(
            ready,
            ApiWait::Ready,
            "API did not become ready; log:\n{log}"
        );
        assert!(!stale_name, "engine still wrote xray.stderr.log");
        assert!(
            log.contains("Xray 26.9.9") || log.contains("Xray "),
            "xray.log missing banner:\n{log}"
        );
        assert!(
            log.contains("API server listening"),
            "xray.log missing API listen line:\n{log}"
        );
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xraybox-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Runs `xray run -test` over a minimal TUN config and returns everything the
    /// sidecar printed, so a test can assert on the decoder's own wording.
    fn run_test_with_tun_settings(settings: serde_json::Value) -> String {
        let sidecar = resolve_sidecar_path().expect("sidecar missing; run scripts/fetch-xray.ps1");
        let dir = unique_temp_dir("tun-field-probe");
        let config_path = dir.join("config.json");
        let config = serde_json::json!({
            "log": { "loglevel": "warning" },
            "inbounds": [{ "tag": "tun-in", "protocol": "tun", "settings": settings }],
            "outbounds": [{ "tag": "direct", "protocol": "freedom" }]
        });
        fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
        let path = config_path.to_string_lossy().into_owned();
        let output = Command::new(&sidecar)
            .args(validate_config_args(&path))
            .output()
            .expect("failed to run xray run -test");
        let _ = fs::remove_dir_all(&dir);
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    /// `run -test` exiting 0 proves nothing about a field name — see the test
    /// below. Handing the field a wrong *type* does prove it: Go's decoder names
    /// the struct field it could not fill. `settings.dns` is what keeps the OS
    /// resolver inside the tunnel, so if a future Xray renames or drops it the
    /// DNS leak comes back with no other symptom.
    #[test]
    #[ignore = "sidecar integration; pins the TUN dns field name against the pinned Xray"]
    fn tun_dns_field_is_named_dns_and_takes_a_string_list() {
        let detail = run_test_with_tun_settings(serde_json::json!({
            "name": "xraybox-tun",
            "dns": 12345
        }));
        assert!(
            detail.contains("TunConfig.dns"),
            "expected the decoder to name TunConfig.dns; got:\n{detail}"
        );
        assert!(
            detail.contains("[]string"),
            "expected TunConfig.dns to be a string list; got:\n{detail}"
        );
    }

    /// Documents why the probe above cannot be replaced by "the config validates".
    #[test]
    #[ignore = "sidecar integration; documents that run -test cannot check field names"]
    fn unknown_tun_setting_is_silently_accepted() {
        let detail = run_test_with_tun_settings(serde_json::json!({
            "name": "xraybox-tun",
            "thisFieldDoesNotExist": ["x"]
        }));
        assert!(
            detail.contains("Configuration OK"),
            "expected an unknown key to pass; got:\n{detail}"
        );
    }

    #[test]
    fn require_wintun_errors_when_dll_absent() {
        let dir = std::env::temp_dir().join(format!(
            "xraybox-wintun-absent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let sidecar = dir.join("xray.exe");
        fs::write(&sidecar, []).unwrap();
        let err = require_wintun_beside_sidecar(&sidecar).unwrap_err();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(err.to_string(), WINTUN_DLL_MISSING);
    }

    #[test]
    fn require_wintun_ok_when_dll_beside_sidecar() {
        let dir = std::env::temp_dir().join(format!(
            "xraybox-wintun-present-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let sidecar = dir.join("xray.exe");
        fs::write(&sidecar, []).unwrap();
        fs::write(dir.join(WINTUN_DLL_NAME), []).unwrap();
        let result = require_wintun_beside_sidecar(&sidecar);
        let _ = fs::remove_dir_all(&dir);
        assert!(result.is_ok());
    }

    #[test]
    fn require_wintun_ignores_dll_in_cwd_or_nested_folder() {
        let root = std::env::temp_dir().join(format!(
            "xraybox-wintun-cwd-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sidecar_dir = root.join("sidecar");
        let nested = sidecar_dir.join("binaries");
        fs::create_dir_all(&nested).unwrap();
        let sidecar = sidecar_dir.join("xray.exe");
        fs::write(&sidecar, []).unwrap();
        fs::write(root.join(WINTUN_DLL_NAME), []).unwrap();
        fs::write(nested.join(WINTUN_DLL_NAME), []).unwrap();
        let err = require_wintun_beside_sidecar(&sidecar).unwrap_err();
        let _ = fs::remove_dir_all(&root);
        assert_eq!(err.to_string(), WINTUN_DLL_MISSING);
    }

    #[test]
    #[ignore = "needs fetched binaries"]
    fn fetched_sidecar_dir_contains_wintun() {
        let sidecar = resolve_sidecar_path().expect("sidecar missing; run scripts/fetch-xray.ps1");
        require_wintun_beside_sidecar(&sidecar)
            .expect("wintun.dll missing beside sidecar; run scripts/fetch-xray.ps1");
    }

    #[test]
    fn sidecar_file_name_matches_the_host_triple() {
        let name = sidecar_file_name();
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert_eq!(name, "xray-aarch64-apple-darwin");
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        assert_eq!(name, "xray-x86_64-apple-darwin");
        #[cfg(windows)]
        assert_eq!(name, "xray-x86_64-pc-windows-msvc.exe");
        #[cfg(not(any(windows, target_os = "macos")))]
        assert_eq!(name, "xray");
    }

    #[test]
    fn detects_reality_invalid_connection() {
        assert_eq!(
            handshake_failure_code(
                "transport/internet/reality: REALITY: processed invalid connection"
            ),
            Some(REALITY_INVALID_CONNECTION)
        );
        assert_eq!(
            handshake_failure_code(
                "REALITY: received real certificate (potential MITM or redirection)"
            ),
            Some(REALITY_INVALID_CONNECTION)
        );
        assert_eq!(handshake_failure_code("xray started"), None);
        assert_eq!(handshake_failure_code("reality verification failed"), None);
    }

    #[test]
    #[ignore = "needs elevated Wintun; schema already accepted without tun inbound"]
    fn xray_test_accepts_tun_dns_config() {
        use crate::dns::DnsSettings;
        use crate::profile::VlessReality;

        let sidecar = resolve_sidecar_path().expect("sidecar missing; run scripts/fetch-xray.ps1");
        let workdir =
            std::env::temp_dir().join(format!("xraybox-xray-tun-dns-test-{}", std::process::id()));
        fs::create_dir_all(&workdir).unwrap();
        let config_path = workdir.join("config.json");
        let profile = VlessReality {
            remark: "test".into(),
            server: "edge.example.com".into(),
            server_port: 443,
            uuid: "11111111-1111-1111-1111-111111111111".into(),
            server_name: "www.cloudflare.com".into(),
            public_key: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            short_id: "dead".into(),
            flow: Some("xtls-rprx-vision".into()),
            fingerprint: "chrome".into(),
            spider_x: None,
            mldsa65_verify: None,
        };
        let value = crate::engine::config::ConfigBuilder::default()
            .with_proxy(profile)
            .with_tun()
            .with_dns(DnsSettings::default())
            .with_local_dns_domains(vec![crate::dns::dns_geosite_to_local("geosite:cn")])
            .with_route(crate::routing::RouteSettings::default())
            .build();
        fs::write(&config_path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        let asset_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("assets");
        let config_arg = config_path.to_str().unwrap();
        let output = Command::new(&sidecar)
            .args(validate_config_args(config_arg))
            .env("XRAY_LOCATION_ASSET", &asset_dir)
            .output()
            .expect("failed to run xray run -test");

        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let _ = fs::remove_dir_all(&workdir);
        assert!(
            output.status.success(),
            "xray run -test failed: {stderr}{stdout}"
        );
    }

    #[test]
    #[ignore = "needs elevated Wintun; schema already accepted"]
    fn xray_test_accepts_tun_config() {
        use crate::profile::VlessReality;

        let sidecar = resolve_sidecar_path().expect("sidecar missing; run scripts/fetch-xray.ps1");
        let workdir =
            std::env::temp_dir().join(format!("xraybox-xray-tun-test-{}", std::process::id()));
        fs::create_dir_all(&workdir).unwrap();
        let config_path = workdir.join("config.json");
        let profile = VlessReality {
            remark: "test".into(),
            server: "edge.example.com".into(),
            server_port: 443,
            uuid: "11111111-1111-1111-1111-111111111111".into(),
            server_name: "www.cloudflare.com".into(),
            public_key: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            short_id: "dead".into(),
            flow: Some("xtls-rprx-vision".into()),
            fingerprint: "chrome".into(),
            spider_x: None,
            mldsa65_verify: None,
        };
        let value = crate::engine::config::ConfigBuilder::default()
            .with_proxy(profile)
            .with_tun()
            .build();
        fs::write(&config_path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        let config_arg = config_path.to_str().unwrap();
        let output = Command::new(&sidecar)
            .args(validate_config_args(config_arg))
            .output()
            .expect("failed to run xray run -test");

        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let _ = fs::remove_dir_all(&workdir);
        assert!(
            output.status.success(),
            "xray run -test failed: {stderr}{stdout}"
        );
    }

    #[test]
    #[ignore = "sidecar integration; vless outbound schema"]
    fn xray_test_accepts_vless_config() {
        use crate::profile::VlessReality;

        let sidecar = resolve_sidecar_path().expect("sidecar missing; run scripts/fetch-xray.ps1");
        let workdir =
            std::env::temp_dir().join(format!("xraybox-xray-vless-test-{}", std::process::id()));
        fs::create_dir_all(&workdir).unwrap();
        let config_path = workdir.join("config.json");
        let profile = VlessReality {
            remark: "test".into(),
            server: "edge.example.com".into(),
            server_port: 443,
            uuid: "11111111-1111-1111-1111-111111111111".into(),
            server_name: "www.cloudflare.com".into(),
            public_key: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            short_id: "dead".into(),
            flow: Some("xtls-rprx-vision".into()),
            fingerprint: "chrome".into(),
            spider_x: None,
            mldsa65_verify: None,
        };
        let value = crate::engine::config::ConfigBuilder::default()
            .with_proxy(profile)
            .with_smoke_mixed(true)
            .build();
        fs::write(&config_path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        let config_arg = config_path.to_str().unwrap();
        let output = Command::new(&sidecar)
            .args(validate_config_args(config_arg))
            .output()
            .expect("failed to run xray run -test");

        let _ = fs::remove_dir_all(&workdir);
        assert!(
            output.status.success(),
            "xray run -test failed: {}{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
    }

    #[test]
    #[ignore = "sidecar integration; minimal skeleton"]
    fn xray_test_accepts_minimal_config() {
        let sidecar = resolve_sidecar_path().expect("sidecar missing; run scripts/fetch-xray.ps1");
        let workdir =
            std::env::temp_dir().join(format!("xraybox-xray-test-{}", std::process::id()));
        fs::create_dir_all(&workdir).unwrap();
        let config_path = workdir.join("config.json");
        let value = ConfigBuilder::default().build();
        fs::write(&config_path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        let config_arg = config_path.to_str().unwrap();
        let output = Command::new(&sidecar)
            .args(validate_config_args(config_arg))
            .output()
            .expect("failed to run xray run -test");

        let _ = fs::remove_dir_all(&workdir);
        assert!(
            output.status.success(),
            "xray run -test failed: {}{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
