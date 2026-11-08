use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::ToSocketAddrs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use super::dns::{
    default_route_device, get_dns_args, get_info_args, parse_dns_servers, parse_ipv6_config,
    restore_ipv6_args, service_for_device, set_dns_args, set_ipv6_off_args, Ipv6Config,
};
use super::paths::{config_path_allowed, directory_allowed, sidecar_allowed};
use super::pf::{
    is_plain_ipv4, parse_pf_token, pf_rules, route_delete_args, PF_ANCHOR, PROTECTED_IPV4_ROUTES,
};
use super::{Reply, Request, HELPER_STATE_PATH, SOCKET_PATH};

struct Persisted {
    dns_saved: bool,
    dns_service: String,
    dns_servers: Vec<String>,
    ipv6_saved: bool,
    ipv6: Ipv6Config,
    pf_token: String,
}

struct Runtime {
    generation: u64,
    pid: Option<u32>,
    kill_switch: bool,
    server_ips: Vec<String>,
    server_port: u16,
    persisted: Persisted,
}

static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime {
    generation: 0,
    pid: None,
    kill_switch: false,
    server_ips: Vec::new(),
    server_port: 0,
    persisted: Persisted {
        dns_saved: false,
        dns_service: String::new(),
        dns_servers: Vec::new(),
        ipv6_saved: false,
        ipv6: Ipv6Config {
            mode: String::new(),
            address: String::new(),
            prefix: String::new(),
            router: String::new(),
        },
        pf_token: String::new(),
    },
});

pub fn run() {
    let hash = hash_self().unwrap_or_default();
    cleanup_leftovers();
    let listener = wait_for_listener();
    thread::spawn(refresh_socket_owner);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let hash = hash.clone();
                thread::spawn(move || {
                    if let Err(err) = handle_client(stream, &hash) {
                        eprintln!("xraybox-helper: {err}");
                    }
                });
            }
            Err(err) => {
                eprintln!("xraybox-helper: accept failed: {err}");
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn handle_client(mut stream: UnixStream, hash: &str) -> Result<(), String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let peer = peer_uid(&stream)?;
    let console = console_uid().ok_or_else(|| "no console user".to_string())?;
    if peer != console {
        write_reply(
            &mut stream,
            &Reply::fail("helper_unavailable: connection is not from the console user"),
        )?;
        return Ok(());
    }
    let mut line = read_line(&mut stream)?;
    if line.ends_with('\n') {
        line.pop();
    }
    let request: Request = serde_json::from_str(&line).map_err(|err| err.to_string())?;
    match request {
        Request::InstallCheck => {
            write_reply(&mut stream, &Reply::install_check(hash))?;
        }
        Request::Start(spec) => {
            let generation = {
                let mut rt = lock_runtime();
                match rt.start(&spec, peer) {
                    Ok(()) => rt.generation,
                    Err(err) => {
                        write_reply(&mut stream, &Reply::fail(err))?;
                        return Ok(());
                    }
                }
            };
            write_reply(&mut stream, &Reply::ok())?;
            let _ = stream.set_read_timeout(None);
            let mut buf = [0u8; 64];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
            lock_runtime().watch_closed(generation);
        }
        Request::Stop => {
            lock_runtime().stop_session();
            write_reply(&mut stream, &Reply::ok())?;
        }
        Request::Status => {
            let rt = lock_runtime();
            let alive = rt.pid.is_some_and(process_alive);
            let pid = rt.pid.unwrap_or(0);
            drop(rt);
            write_reply(&mut stream, &Reply::status(alive, pid))?;
        }
        Request::SetKillSwitch { enabled } => match lock_runtime().set_kill_switch(enabled) {
            Ok(()) => write_reply(&mut stream, &Reply::ok())?,
            Err(err) => write_reply(&mut stream, &Reply::fail(err))?,
        },
        Request::AllowServer { host, port } => match lock_runtime().allow_server(&host, port) {
            Ok(()) => write_reply(&mut stream, &Reply::ok())?,
            Err(err) => write_reply(&mut stream, &Reply::fail(err))?,
        },
        Request::ReleaseGuards => {
            lock_runtime().release_guards();
            write_reply(&mut stream, &Reply::ok())?;
        }
    }
    Ok(())
}

impl Runtime {
    fn start(&mut self, spec: &super::StartSpec, peer: u32) -> Result<(), String> {
        self.stop_session();
        validate_spec(spec, peer)?;
        let server_ips = resolve_v4(&spec.server_host)?;
        if spec.server_port == 0 {
            return Err("server port is missing".into());
        }
        self.server_ips = server_ips;
        self.server_port = spec.server_port;
        self.kill_switch = false;
        self.apply_rules()?;
        if let Err(err) = self.prepare_service(&spec.dns_servers) {
            self.release_guards();
            return Err(err);
        }
        match spawn_xray(spec, peer) {
            Ok(child) => {
                let pid = child.id();
                self.pid = Some(pid);
                self.generation = self.generation.wrapping_add(1);
                let generation = self.generation;
                thread::spawn(move || {
                    let mut child = child;
                    let _ = child.wait();
                    lock_runtime().on_exit(pid, generation);
                });
                Ok(())
            }
            Err(err) => {
                self.release_guards();
                Err(err)
            }
        }
    }

    fn stop_session(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        let pid = self.pid.take();
        self.kill_switch = false;
        self.release_guards();
        if let Some(pid) = pid {
            kill_process(pid);
        }
    }

    fn watch_closed(&mut self, generation: u64) {
        if self.generation != generation {
            return;
        }
        self.stop_session();
    }

    fn on_exit(&mut self, pid: u32, generation: u64) {
        if self.pid != Some(pid) || self.generation != generation {
            return;
        }
        self.pid = None;
        delete_protected_routes();
        if !self.kill_switch {
            self.release_guards();
        }
    }

    fn set_kill_switch(&mut self, enabled: bool) -> Result<(), String> {
        self.kill_switch = enabled;
        self.apply_rules()
    }

    fn allow_server(&mut self, host: &str, port: u16) -> Result<(), String> {
        if port == 0 {
            return Err("server port is missing".into());
        }
        self.server_ips = resolve_v4(host)?;
        self.server_port = port;
        if self.kill_switch {
            self.apply_rules()?;
        }
        Ok(())
    }

    fn apply_rules(&mut self) -> Result<(), String> {
        self.enable_pf()?;
        let rules = pf_rules(self.kill_switch, &self.server_ips, self.server_port);
        load_rules(&rules)
    }

    fn enable_pf(&mut self) -> Result<(), String> {
        if !self.persisted.pf_token.is_empty() {
            return Ok(());
        }
        let output = Command::new("/sbin/pfctl")
            .arg("-E")
            .output()
            .map_err(|err| format!("pfctl -E failed: {err}"))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success() && parse_pf_token(&text).is_none() {
            return Err(format!("pfctl -E failed: {}", text.trim()));
        }
        let token = parse_pf_token(&text)
            .ok_or_else(|| format!("pfctl -E returned no token: {}", text.trim()))?;
        self.persisted.pf_token = token;
        self.save_state();
        Ok(())
    }

    fn prepare_service(&mut self, servers: &[String]) -> Result<(), String> {
        let service = active_service()?;
        self.apply_dns(&service, servers)?;
        self.disable_ipv6(&service)
    }

    fn apply_dns(&mut self, service: &str, servers: &[String]) -> Result<(), String> {
        if servers.is_empty() {
            return Ok(());
        }
        if !self.persisted.dns_saved {
            let current = read_dns(service)?;
            self.persisted.dns_saved = true;
            self.persisted.dns_service = service.to_string();
            self.persisted.dns_servers = current;
            self.save_state();
        }
        run_networksetup(&set_dns_args(service, servers))
    }

    fn disable_ipv6(&mut self, service: &str) -> Result<(), String> {
        if !self.persisted.ipv6_saved {
            let info = networksetup_output(&get_info_args(service))?;
            self.persisted.ipv6 = parse_ipv6_config(&info);
            self.persisted.ipv6_saved = true;
            if self.persisted.dns_service.is_empty() {
                self.persisted.dns_service = service.to_string();
            }
            self.save_state();
        }
        if self.persisted.ipv6.mode == "off" {
            return Ok(());
        }
        run_networksetup(&set_ipv6_off_args(service))
    }

    fn release_guards(&mut self) {
        flush_anchor();
        release_token(self);
        restore_ipv6(self);
        restore_dns(self);
        delete_protected_routes();
    }

    fn save_state(&self) {
        if let Err(err) = write_state(&self.persisted) {
            eprintln!("xraybox-helper: could not save state: {err}");
        }
    }
}

fn lock_runtime() -> std::sync::MutexGuard<'static, Runtime> {
    RUNTIME.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn validate_spec(spec: &super::StartSpec, peer: u32) -> Result<(), String> {
    let app_meta = fs::symlink_metadata(&spec.app_data_dir).map_err(|err| err.to_string())?;
    if app_meta.file_type().is_symlink() {
        return Err("app data directory is a symlink".into());
    }
    let app_data = fs::canonicalize(&spec.app_data_dir).map_err(|err| err.to_string())?;
    require_config(&spec.config_path, &app_data, peer)?;
    require_dir(&spec.workdir, &app_data, "workdir")?;
    require_dir(&spec.asset_dir, &app_data, "asset dir")?;
    require_sidecar(&spec.sidecar)?;
    Ok(())
}

fn require_config(path: &str, app_data: &Path, peer: u32) -> Result<(), String> {
    let facts = path_facts(Path::new(path))?;
    if config_path_allowed(
        facts.symlink,
        facts.file,
        facts.uid,
        &facts.canonical,
        app_data,
        peer,
    ) {
        Ok(())
    } else {
        Err("config path is not an owned file inside app data".into())
    }
}

fn require_dir(path: &str, app_data: &Path, label: &str) -> Result<(), String> {
    let facts = path_facts(Path::new(path))?;
    if directory_allowed(facts.symlink, facts.dir, &facts.canonical, app_data) {
        Ok(())
    } else {
        Err(format!("{label} is not a directory inside app data"))
    }
}

fn require_sidecar(path: &str) -> Result<(), String> {
    let facts = path_facts(Path::new(path))?;
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if sidecar_allowed(facts.symlink, facts.file, facts.executable, name) {
        Ok(())
    } else {
        Err("sidecar path is not an executable xray binary".into())
    }
}

struct Facts {
    symlink: bool,
    file: bool,
    dir: bool,
    executable: bool,
    uid: u32,
    canonical: PathBuf,
}

fn path_facts(path: &Path) -> Result<Facts, String> {
    let meta = fs::symlink_metadata(path).map_err(|err| err.to_string())?;
    let symlink = meta.file_type().is_symlink();
    let uid = file_uid(path)?;
    let canonical = if symlink {
        path.to_path_buf()
    } else {
        fs::canonicalize(path).map_err(|err| err.to_string())?
    };
    Ok(Facts {
        symlink,
        file: meta.is_file(),
        dir: meta.is_dir(),
        executable: meta.permissions().mode() & 0o111 != 0,
        uid,
        canonical,
    })
}

fn file_uid(path: &Path) -> Result<u32, String> {
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str());
    let c =
        std::ffi::CString::new(bytes.to_vec()).map_err(|_| "path contains a nul".to_string())?;
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::lstat(c.as_ptr(), &mut st) };
    if rc != 0 {
        return Err(format!("lstat failed: {}", std::io::Error::last_os_error()));
    }
    Ok(st.st_uid)
}

fn spawn_xray(spec: &super::StartSpec, uid: u32) -> Result<std::process::Child, String> {
    let log_path = Path::new(&spec.workdir).join(super::super::process::LOG_FILE_NAME);
    let log = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .map_err(|err| format!("xray log: {err}"))?;
    chown_path(&log_path, uid)?;
    let stderr = log.try_clone().map_err(|err| err.to_string())?;
    let mut cmd = Command::new(&spec.sidecar);
    cmd.args(["run", "-c", &spec.config_path])
        .current_dir(&spec.workdir)
        .env("XRAY_LOCATION_ASSET", &spec.asset_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .process_group(0);
    cmd.spawn()
        .map_err(|err| format!("failed to spawn xray: {err}"))
}

fn resolve_v4(host: &str) -> Result<Vec<String>, String> {
    if is_plain_ipv4(host) {
        return Ok(vec![host.to_string()]);
    }
    let mut ips = Vec::new();
    for addr in (host, 0u16)
        .to_socket_addrs()
        .map_err(|err| format!("resolve {host}: {err}"))?
    {
        if let std::net::IpAddr::V4(ip) = addr.ip() {
            let text = ip.to_string();
            if !ips.contains(&text) {
                ips.push(text);
            }
        }
    }
    if ips.is_empty() {
        return Err(format!("no IPv4 address for {host}"));
    }
    Ok(ips)
}

fn active_service() -> Result<String, String> {
    let route = Command::new("/sbin/route")
        .args(["-n", "get", "default"])
        .output()
        .map_err(|err| format!("route: {err}"))?;
    let route_text = String::from_utf8_lossy(&route.stdout);
    let device = default_route_device(&route_text)
        .ok_or_else(|| "no default route interface".to_string())?;
    let order = Command::new("/usr/sbin/networksetup")
        .arg("-listnetworkserviceorder")
        .output()
        .map_err(|err| format!("networksetup: {err}"))?;
    let order_text = String::from_utf8_lossy(&order.stdout);
    service_for_device(&order_text, &device)
        .ok_or_else(|| format!("no network service for {device}"))
}

fn networksetup_output(args: &[String]) -> Result<String, String> {
    let output = Command::new("/usr/sbin/networksetup")
        .args(args)
        .output()
        .map_err(|err| format!("networksetup: {err}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "networksetup {} failed: {}",
            args.join(" "),
            text.trim()
        ))
    }
}

fn read_dns(service: &str) -> Result<Vec<String>, String> {
    Ok(parse_dns_servers(&networksetup_output(&get_dns_args(
        service,
    ))?))
}

fn run_networksetup(args: &[String]) -> Result<(), String> {
    let output = Command::new("/usr/sbin/networksetup")
        .args(args)
        .output()
        .map_err(|err| format!("networksetup: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "networksetup {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn load_rules(rules: &str) -> Result<(), String> {
    let _ = fs::create_dir_all("/var/run/xraybox");
    let path = "/var/run/xraybox/rules.conf";
    fs::write(path, rules).map_err(|err| format!("rules file: {err}"))?;
    let output = Command::new("/sbin/pfctl")
        .args(["-a", PF_ANCHOR, "-f", path])
        .output()
        .map_err(|err| format!("pfctl -f failed: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "pfctl -a {PF_ANCHOR} -f failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn flush_anchor() {
    let _ = Command::new("/sbin/pfctl")
        .args(["-a", PF_ANCHOR, "-F", "all"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn release_token(rt: &mut Runtime) {
    if rt.persisted.pf_token.is_empty() {
        return;
    }
    let _ = Command::new("/sbin/pfctl")
        .args(["-X", &rt.persisted.pf_token])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    rt.persisted.pf_token.clear();
    rt.save_state();
}

fn restore_dns(rt: &mut Runtime) {
    if !rt.persisted.dns_saved || rt.persisted.dns_service.is_empty() {
        rt.persisted.dns_saved = false;
        rt.save_state();
        return;
    }
    let service = rt.persisted.dns_service.clone();
    let servers = rt.persisted.dns_servers.clone();
    if let Err(err) = run_networksetup(&set_dns_args(&service, &servers)) {
        eprintln!("xraybox-helper: restore DNS failed: {err}");
        return;
    }
    rt.persisted.dns_saved = false;
    rt.persisted.dns_service.clear();
    rt.persisted.dns_servers.clear();
    rt.save_state();
}

fn restore_ipv6(rt: &mut Runtime) {
    if !rt.persisted.ipv6_saved || rt.persisted.dns_service.is_empty() {
        rt.persisted.ipv6_saved = false;
        rt.save_state();
        return;
    }
    let service = rt.persisted.dns_service.clone();
    let config = rt.persisted.ipv6.clone();
    if let Err(err) = run_networksetup(&restore_ipv6_args(&service, &config)) {
        eprintln!("xraybox-helper: restore IPv6 failed: {err}");
        return;
    }
    rt.persisted.ipv6_saved = false;
    rt.persisted.ipv6 = Ipv6Config::automatic();
    rt.save_state();
}

fn delete_protected_routes() {
    let gateway = crate::engine::windows::TUN_IPV4_GATEWAY
        .split('/')
        .next()
        .unwrap_or("172.19.0.1");
    for prefix in PROTECTED_IPV4_ROUTES {
        let _ = Command::new("/sbin/route")
            .args(route_delete_args(prefix, gateway))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn write_state(state: &Persisted) -> Result<(), String> {
    let path = Path::new(HELPER_STATE_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let value = serde_json::json!({
        "dnsSaved": state.dns_saved,
        "dnsService": state.dns_service,
        "dnsServers": state.dns_servers,
        "ipv6Saved": state.ipv6_saved,
        "ipv6Mode": state.ipv6.mode,
        "ipv6Address": state.ipv6.address,
        "ipv6Prefix": state.ipv6.prefix,
        "ipv6Router": state.ipv6.router,
        "pfToken": state.pf_token,
    });
    fs::write(
        path,
        serde_json::to_string_pretty(&value).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    let bytes = std::ffi::CString::new(HELPER_STATE_PATH).map_err(|_| "state path".to_string())?;
    unsafe {
        libc::chown(bytes.as_ptr(), 0, 0);
        libc::chmod(bytes.as_ptr(), 0o600);
    }
    Ok(())
}

fn read_state() -> Option<Persisted> {
    let text = fs::read_to_string(HELPER_STATE_PATH).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Persisted {
        dns_saved: value
            .get("dnsSaved")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        dns_service: value
            .get("dnsService")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        dns_servers: value
            .get("dnsServers")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        ipv6_saved: value
            .get("ipv6Saved")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        ipv6: Ipv6Config {
            mode: value
                .get("ipv6Mode")
                .and_then(|v| v.as_str())
                .unwrap_or("automatic")
                .to_string(),
            address: value
                .get("ipv6Address")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            prefix: value
                .get("ipv6Prefix")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            router: value
                .get("ipv6Router")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        },
        pf_token: value
            .get("pfToken")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

fn cleanup_leftovers() {
    let Some(saved) = read_state() else {
        flush_anchor();
        delete_protected_routes();
        return;
    };
    let mut rt = Runtime {
        generation: 0,
        pid: None,
        kill_switch: false,
        server_ips: Vec::new(),
        server_port: 0,
        persisted: saved,
    };
    rt.release_guards();
}

fn process_alive(pid: u32) -> bool {
    let rc = unsafe { libc::kill(pid as i32, 0) };
    rc == 0
}

fn kill_process(pid: u32) {
    let pid = pid as i32;
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }
    for _ in 0..20 {
        if !process_alive(pid as u32) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

fn hash_self() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    super::sha256_file(&exe).map_err(|err| err.to_string())
}

fn wait_for_listener() -> UnixListener {
    loop {
        if console_uid().is_some() {
            if let Some(listener) = bind_socket() {
                return listener;
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn bind_socket() -> Option<UnixListener> {
    let _ = fs::create_dir_all("/var/run/xraybox");
    if UnixStream::connect(SOCKET_PATH).is_ok() {
        eprintln!("xraybox-helper: socket already served, not replacing it");
        thread::sleep(Duration::from_secs(2));
        return None;
    }
    let _ = fs::remove_file(SOCKET_PATH);
    let listener = UnixListener::bind(SOCKET_PATH).ok()?;
    if let Some(uid) = console_uid() {
        let _ = chown_path(Path::new(SOCKET_PATH), uid);
    }
    let bytes = std::ffi::CString::new(SOCKET_PATH).ok()?;
    unsafe {
        libc::chmod(bytes.as_ptr(), 0o600);
    }
    Some(listener)
}

fn refresh_socket_owner() {
    loop {
        if let Some(uid) = console_uid() {
            let _ = chown_path(Path::new(SOCKET_PATH), uid);
        }
        thread::sleep(Duration::from_secs(2));
    }
}

fn console_uid() -> Option<u32> {
    let path = std::ffi::CString::new("/dev/console").ok()?;
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::stat(path.as_ptr(), &mut st) };
    if rc != 0 || st.st_uid == 0 {
        None
    } else {
        Some(st.st_uid)
    }
}

fn peer_uid(stream: &UnixStream) -> Result<u32, String> {
    use std::os::unix::io::AsRawFd;
    let mut uid = 0u32;
    let mut gid = 0u32;
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if rc != 0 {
        return Err(format!(
            "getpeereid failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(uid)
}

fn chown_path(path: &Path, uid: u32) -> Result<(), String> {
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str());
    let c =
        std::ffi::CString::new(bytes.to_vec()).map_err(|_| "path contains a nul".to_string())?;
    let gid = console_gid().unwrap_or(u32::MAX);
    let rc = unsafe { libc::chown(c.as_ptr(), uid, gid) };
    if rc != 0 {
        return Err(format!("chown failed: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

fn console_gid() -> Option<u32> {
    let path = std::ffi::CString::new("/dev/console").ok()?;
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::stat(path.as_ptr(), &mut st) };
    if rc != 0 {
        None
    } else {
        Some(st.st_gid)
    }
}

fn read_line(stream: &mut UnixStream) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte).map_err(|err| err.to_string())?;
        if n == 0 || byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
        if buf.len() > 1024 * 1024 {
            return Err("helper request is too large".into());
        }
    }
    String::from_utf8(buf).map_err(|_| "helper request is not utf-8".into())
}

fn write_reply(stream: &mut UnixStream, reply: &Reply) -> Result<(), String> {
    let mut line = serde_json::to_string(reply).map_err(|err| err.to_string())?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|err| err.to_string())?;
    stream.flush().map_err(|err| err.to_string())
}
