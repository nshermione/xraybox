mod dns;
mod paths;
mod pf;

#[cfg(target_os = "macos")]
mod helper;

use serde::{Deserialize, Serialize};

pub use pf::TUN_NAME;

pub const PROTOCOL: u32 = 1;
pub const HELPER_LABEL: &str = "io.github.xraybox.helper";
pub const HELPER_BIN: &str = "/Library/PrivilegedHelperTools/io.github.xraybox.helper";
pub const HELPER_PLIST: &str = "/Library/LaunchDaemons/io.github.xraybox.helper.plist";
pub const HELPER_STATE_PATH: &str =
    "/Library/Application Support/io.github.xraybox/helper-state.json";
pub const SOCKET_PATH: &str = "/var/run/xraybox/helper.sock";
pub const HELPER_ARG: &str = "--xraybox-helper";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSpec {
    pub sidecar: String,
    pub config_path: String,
    pub workdir: String,
    pub asset_dir: String,
    pub app_data_dir: String,
    pub server_host: String,
    pub server_port: u16,
    pub dns_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "camelCase")]
enum Request {
    InstallCheck,
    Start(StartSpec),
    Stop,
    Status,
    SetKillSwitch { enabled: bool },
    AllowServer { host: String, port: u16 },
    ReleaseGuards,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    ok: bool,
    #[serde(default)]
    error: String,
    #[serde(default)]
    protocol: u32,
    #[serde(default)]
    exe_hash: String,
    #[serde(default)]
    alive: bool,
    #[serde(default)]
    pid: u32,
}

impl Reply {
    fn ok() -> Self {
        Self {
            ok: true,
            error: String::new(),
            protocol: PROTOCOL,
            exe_hash: String::new(),
            alive: false,
            pid: 0,
        }
    }

    fn fail(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: error.into(),
            protocol: PROTOCOL,
            exe_hash: String::new(),
            alive: false,
            pid: 0,
        }
    }

    fn install_check(hash: &str) -> Self {
        Self {
            ok: true,
            error: String::new(),
            protocol: PROTOCOL,
            exe_hash: hash.to_string(),
            alive: false,
            pid: 0,
        }
    }

    fn status(alive: bool, pid: u32) -> Self {
        Self {
            ok: true,
            error: String::new(),
            protocol: PROTOCOL,
            exe_hash: String::new(),
            alive,
            pid,
        }
    }
}

pub fn launch_daemon_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{HELPER_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{HELPER_BIN}</string>
    <string>{HELPER_ARG}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
</dict>
</plist>
"#
    )
}

pub fn helper_install_script(src: &str) -> String {
    let src = shell_quote(src);
    let plist = launch_daemon_plist();
    format!(
        r#"#!/bin/bash
set -euo pipefail
src={src}
mkdir -p "/Library/PrivilegedHelperTools" "/Library/Application Support/io.github.xraybox" /var/run/xraybox
cp -f "$src" "{HELPER_BIN}"
chown root:wheel "{HELPER_BIN}"
chmod 544 "{HELPER_BIN}"
xattr -dr com.apple.quarantine "{HELPER_BIN}" || true
cat > "{HELPER_PLIST}" <<'PLIST'
{plist}
PLIST
chown root:wheel "{HELPER_PLIST}"
chmod 644 "{HELPER_PLIST}"
launchctl bootout system "{HELPER_PLIST}" 2>/dev/null || true
launchctl bootstrap system "{HELPER_PLIST}"
"#
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
mod client {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::process::Command;
    use std::sync::Mutex;
    use std::time::Duration;

    use super::paths::{helper_needs_reinstall, osascript_was_cancelled};
    use super::{
        helper_install_script, Reply, Request, StartSpec, HELPER_ARG, PROTOCOL, SOCKET_PATH,
    };
    use crate::engine::error::EngineError;

    static WATCH: Mutex<Option<UnixStream>> = Mutex::new(None);

    pub fn helper_requested() -> bool {
        std::env::args().any(|arg| arg == HELPER_ARG)
    }

    pub fn run_helper() {
        super::helper::run();
    }

    pub fn start_tunnel(spec: StartSpec) -> Result<(), EngineError> {
        ensure_helper()?;
        let _ = stop_tunnel();
        let mut stream = connect()?;
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .map_err(|err| EngineError::HelperUnavailable(err.to_string()))?;
        write_request(&mut stream, &Request::Start(spec))?;
        let reply = read_reply(&mut stream)?;
        if !reply.ok {
            return Err(reply_error(&reply));
        }
        let _ = stream.set_read_timeout(None);
        *watch().lock().unwrap_or_else(|p| p.into_inner()) = Some(stream);
        Ok(())
    }

    pub fn stop_tunnel() -> Result<(), EngineError> {
        let result = (|| {
            let mut stream = connect()?;
            write_request(&mut stream, &Request::Stop)?;
            let reply = read_reply(&mut stream)?;
            if reply.ok {
                Ok(())
            } else {
                Err(reply_error(&reply))
            }
        })();
        *watch().lock().unwrap_or_else(|p| p.into_inner()) = None;
        result
    }

    pub fn sidecar_is_alive() -> bool {
        status().map(|reply| reply.alive).unwrap_or(false)
    }

    pub fn sidecar_pid() -> Option<u32> {
        status()
            .ok()
            .filter(|reply| reply.alive && reply.pid != 0)
            .map(|reply| reply.pid)
    }

    pub fn set_kill_switch(enabled: bool) -> Result<(), EngineError> {
        ensure_helper()?;
        let mut stream = connect()?;
        write_request(&mut stream, &Request::SetKillSwitch { enabled })?;
        let reply = read_reply(&mut stream)?;
        if reply.ok {
            Ok(())
        } else {
            Err(reply_error(&reply))
        }
    }

    pub fn allow_server(host: &str, port: u16) -> Result<(), EngineError> {
        ensure_helper()?;
        let mut stream = connect()?;
        write_request(
            &mut stream,
            &Request::AllowServer {
                host: host.to_string(),
                port,
            },
        )?;
        let reply = read_reply(&mut stream)?;
        if reply.ok {
            Ok(())
        } else {
            Err(reply_error(&reply))
        }
    }

    pub fn release_guards() {
        if let Ok(mut stream) = connect() {
            let _ = write_request(&mut stream, &Request::ReleaseGuards);
            let _ = read_reply(&mut stream);
        }
    }

    pub fn tun_present() -> bool {
        interface_names().iter().any(|name| name == super::TUN_NAME)
    }

    pub fn physical_ipv4() -> Option<std::net::Ipv4Addr> {
        use crate::engine::windows::{is_tun_ipv4, is_usable_physical_ipv4};
        let preferred = default_device();
        let mut fallback = None;
        let mut matched = None;
        unsafe {
            let mut head = std::ptr::null_mut();
            if libc::getifaddrs(&mut head) != 0 {
                return None;
            }
            let mut cursor = head;
            while !cursor.is_null() {
                let ifa = &*cursor;
                cursor = ifa.ifa_next;
                if ifa.ifa_addr.is_null() || (*ifa.ifa_addr).sa_family != libc::AF_INET as u8 {
                    continue;
                }
                let name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy();
                if skipped_interface(&name) {
                    continue;
                }
                let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                // s_addr is network byte order. from_be undoes the swap on little-endian.
                let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                if !is_usable_physical_ipv4(ip) || is_tun_ipv4(ip) {
                    continue;
                }
                if preferred.as_deref() == Some(name.as_ref()) {
                    matched = Some(ip);
                    break;
                }
                if fallback.is_none() {
                    fallback = Some(ip);
                }
            }
            libc::freeifaddrs(head);
        }
        matched.or(fallback)
    }

    fn status() -> Result<Reply, EngineError> {
        let mut stream = connect()?;
        write_request(&mut stream, &Request::Status)?;
        read_reply(&mut stream)
    }

    fn ensure_helper() -> Result<(), EngineError> {
        let local = sha256_current_exe()?;
        if let Ok(reply) = install_check() {
            if !helper_needs_reinstall(&local, reply.protocol, &reply.exe_hash) {
                return Ok(());
            }
        }
        install_helper()?;
        let reply = wait_for_check()?;
        if helper_needs_reinstall(&local, reply.protocol, &reply.exe_hash) {
            return Err(EngineError::HelperInstallFailed(
                "installed helper does not match this app".into(),
            ));
        }
        Ok(())
    }

    fn install_helper() -> Result<(), EngineError> {
        let exe = std::env::current_exe().map_err(|err| {
            EngineError::HelperInstallFailed(format!("cannot resolve current exe: {err}"))
        })?;
        let script = helper_install_script(&exe.to_string_lossy());
        let path =
            std::env::temp_dir().join(format!("xraybox-helper-install-{}.sh", std::process::id()));
        std::fs::write(&path, script)?;
        let quoted = path.display().to_string().replace('\'', "'\\''");
        let source = format!("do shell script \"bash '{quoted}'\" with administrator privileges");
        let output = Command::new("/usr/bin/osascript")
            .args(["-e", &source])
            .output()
            .map_err(|err| EngineError::HelperInstallFailed(err.to_string()))?;
        let _ = std::fs::remove_file(&path);
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if osascript_was_cancelled(&stderr, &stdout) {
            return Err(EngineError::UacCancelled);
        }
        Err(EngineError::HelperInstallFailed(
            format!("{stderr} {stdout}").trim().to_string(),
        ))
    }

    fn wait_for_check() -> Result<Reply, EngineError> {
        let mut last = EngineError::HelperUnavailable("helper socket did not appear".into());
        for _ in 0..50 {
            match install_check() {
                Ok(reply) => return Ok(reply),
                Err(err) => last = err,
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err(last)
    }

    fn install_check() -> Result<Reply, EngineError> {
        let mut stream = connect()?;
        write_request(&mut stream, &Request::InstallCheck)?;
        let reply = read_reply(&mut stream)?;
        if reply.ok {
            Ok(reply)
        } else {
            Err(reply_error(&reply))
        }
    }

    fn connect() -> Result<UnixStream, EngineError> {
        UnixStream::connect(SOCKET_PATH)
            .map_err(|err| EngineError::HelperUnavailable(err.to_string()))
    }

    fn write_request(stream: &mut UnixStream, request: &Request) -> Result<(), EngineError> {
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|err| EngineError::HelperUnavailable(err.to_string()))?;
        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        stream.write_all(line.as_bytes())?;
        stream.flush()?;
        Ok(())
    }

    fn read_reply(stream: &mut UnixStream) -> Result<Reply, EngineError> {
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .map_err(|err| EngineError::HelperUnavailable(err.to_string()))?;
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let n = stream.read(&mut byte)?;
            if n == 0 || byte[0] == b'\n' {
                break;
            }
            buf.push(byte[0]);
            if buf.len() > 1024 * 1024 {
                return Err(EngineError::HelperUnavailable(
                    "helper reply is too large".into(),
                ));
            }
        }
        if buf.is_empty() {
            return Err(EngineError::HelperUnavailable(
                "helper closed the socket".into(),
            ));
        }
        let reply: Reply = serde_json::from_slice(&buf)?;
        if reply.protocol != 0 && reply.protocol != PROTOCOL && reply.ok {
            return Err(EngineError::HelperInstallFailed(
                "helper protocol does not match this app".into(),
            ));
        }
        Ok(reply)
    }

    fn reply_error(reply: &Reply) -> EngineError {
        if reply.error.starts_with("helper_unavailable") {
            EngineError::HelperUnavailable(reply.error.clone())
        } else if reply.error.starts_with("helper_install_failed") {
            EngineError::HelperInstallFailed(reply.error.clone())
        } else {
            EngineError::Process(reply.error.clone())
        }
    }

    fn sha256_current_exe() -> Result<String, EngineError> {
        let exe = std::env::current_exe().map_err(|err| {
            EngineError::HelperInstallFailed(format!("cannot resolve current exe: {err}"))
        })?;
        super::sha256_file(&exe)
    }

    fn watch() -> &'static Mutex<Option<UnixStream>> {
        &WATCH
    }

    fn default_device() -> Option<String> {
        let output = Command::new("/sbin/route")
            .args(["-n", "get", "default"])
            .output()
            .ok()?;
        super::dns::default_route_device(&String::from_utf8_lossy(&output.stdout))
    }

    fn interface_names() -> Vec<String> {
        let mut names = Vec::new();
        unsafe {
            let mut head = std::ptr::null_mut();
            if libc::getifaddrs(&mut head) != 0 {
                return names;
            }
            let mut cursor = head;
            while !cursor.is_null() {
                let ifa = &*cursor;
                cursor = ifa.ifa_next;
                if ifa.ifa_name.is_null() {
                    continue;
                }
                let name = std::ffi::CStr::from_ptr(ifa.ifa_name)
                    .to_string_lossy()
                    .into_owned();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            libc::freeifaddrs(head);
        }
        names
    }

    fn skipped_interface(name: &str) -> bool {
        name == "lo0"
            || name.starts_with("utun")
            || name.starts_with("awdl")
            || name.starts_with("llw")
            || name.starts_with("gif")
            || name.starts_with("stf")
            || name.starts_with("bridge")
            || name.starts_with("ap")
            || name.starts_with("pktap")
            || name.starts_with("anpi")
    }
}

#[cfg(target_os = "macos")]
pub use client::{
    allow_server, helper_requested, physical_ipv4, release_guards, run_helper, set_kill_switch,
    sidecar_is_alive, sidecar_pid, start_tunnel, stop_tunnel, tun_present,
};

#[cfg(target_os = "macos")]
pub(crate) fn sha256_file(path: &Path) -> Result<String, EngineError> {
    let output = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .map_err(|err| EngineError::HelperUnavailable(format!("shasum: {err}")))?;
    if !output.status.success() {
        return Err(EngineError::HelperUnavailable(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let hash = text.split_whitespace().next().unwrap_or("");
    if hash.len() != 64 {
        return Err(EngineError::HelperUnavailable(format!(
            "unexpected shasum output: {text}"
        )));
    }
    Ok(hash.to_string())
}

#[cfg(target_os = "macos")]
use crate::engine::error::EngineError;
#[cfg(target_os = "macos")]
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_daemon_plist_runs_the_helper_at_load() {
        let plist = launch_daemon_plist();
        assert!(plist.contains(HELPER_LABEL));
        assert!(plist.contains(HELPER_BIN));
        assert!(plist.contains(HELPER_ARG));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
    }

    #[test]
    fn install_script_copies_into_privileged_helper_tools_and_bootstraps() {
        let script =
            helper_install_script("/Users/ada/Applications/xraybox.app/Contents/MacOS/xraybox");
        assert!(script.contains("/Library/PrivilegedHelperTools/io.github.xraybox.helper"));
        assert!(script.contains("launchctl bootstrap system"));
        assert!(script.contains("launchctl bootout system"));
        assert!(script.contains("xattr -dr com.apple.quarantine"));
        assert!(script.contains("chmod 544"));
        assert!(script.contains("/Users/ada/Applications/xraybox.app/Contents/MacOS/xraybox"));
    }

    #[test]
    fn install_script_quotes_spaces_in_the_source_path() {
        let script = helper_install_script("/Users/ada/My Apps/xraybox");
        assert!(script.contains("'/Users/ada/My Apps/xraybox'"));
    }

    #[test]
    fn start_request_carries_the_spec_beside_the_command() {
        let request = Request::Start(StartSpec {
            sidecar: "/tmp/xray".into(),
            config_path: "/tmp/app/run/config.json".into(),
            workdir: "/tmp/app/run".into(),
            asset_dir: "/tmp/app/assets".into(),
            app_data_dir: "/tmp/app".into(),
            server_host: "edge.example.com".into(),
            server_port: 443,
            dns_servers: vec!["1.1.1.1".into()],
        });
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["cmd"], "start");
        assert_eq!(value["args"]["serverPort"], 443);
        assert_eq!(value["args"]["serverHost"], "edge.example.com");
        let back: Request = serde_json::from_value(value).unwrap();
        match back {
            Request::Start(spec) => assert_eq!(spec.server_port, 443),
            other => panic!("unexpected {other:?}"),
        }
    }
}
