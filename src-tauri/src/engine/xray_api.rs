use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::engine::error::EngineError;

/// Same env the sidecar spawn and `xray run -test` already set. `adrules`
/// parses the rule list in this process, so `geoip:private` looks for
/// `geoip.dat` here — not in the running core. Without the env, Xray walks
/// next to the sidecar (`target/debug/geoip.dat` in a Tauri debug build).
pub const LOCATION_ASSET_ENV: &str = "XRAY_LOCATION_ASSET";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProxyTraffic {
    pub uplink: u64,
    pub downlink: u64,
}

impl ProxyTraffic {
    #[allow(dead_code)]
    pub fn total(self) -> u64 {
        self.uplink.saturating_add(self.downlink)
    }
}

pub fn add_outbound_args(addr: &str, path: &Path) -> Vec<String> {
    vec![
        "api".into(),
        "ado".into(),
        format!("--server={addr}"),
        path.to_string_lossy().into_owned(),
    ]
}

pub fn remove_outbound_args(addr: &str, tag: &str) -> Vec<String> {
    vec![
        "api".into(),
        "rmo".into(),
        format!("--server={addr}"),
        tag.into(),
    ]
}

pub fn add_rules_args(addr: &str, path: &Path) -> Vec<String> {
    vec![
        "api".into(),
        "adrules".into(),
        format!("--server={addr}"),
        path.to_string_lossy().into_owned(),
    ]
}

pub fn list_outbounds_args(addr: &str) -> Vec<String> {
    vec!["api".into(), "lso".into(), format!("--server={addr}")]
}

pub fn statsquery_args(addr: &str, pattern: &str) -> Vec<String> {
    vec![
        "api".into(),
        "statsquery".into(),
        format!("--server={addr}"),
        format!("-pattern={pattern}"),
    ]
}

pub fn add_outbound(
    sidecar: &Path,
    addr: &str,
    path: &Path,
    asset_dir: &Path,
) -> Result<(), EngineError> {
    run_api(sidecar, &add_outbound_args(addr, path), asset_dir).map(|_| ())
}

pub fn remove_outbound(
    sidecar: &Path,
    addr: &str,
    tag: &str,
    asset_dir: &Path,
) -> Result<(), EngineError> {
    run_api(sidecar, &remove_outbound_args(addr, tag), asset_dir).map(|_| ())
}

pub fn add_rules(
    sidecar: &Path,
    addr: &str,
    path: &Path,
    asset_dir: &Path,
) -> Result<(), EngineError> {
    run_api(sidecar, &add_rules_args(addr, path), asset_dir).map(|_| ())
}

pub fn confirm_api(sidecar: &Path, addr: &str, asset_dir: &Path) -> Result<(), EngineError> {
    let stdout = run_api(sidecar, &list_outbounds_args(addr), asset_dir)?;
    if stdout.trim().is_empty() {
        return Err(EngineError::Process(
            "xray api lso returned an empty outbound list".into(),
        ));
    }
    Ok(())
}

pub fn query_proxy_traffic(
    sidecar: &Path,
    addr: &str,
    asset_dir: &Path,
) -> Result<ProxyTraffic, EngineError> {
    let stdout = run_api(
        sidecar,
        &statsquery_args(addr, "outbound>>>proxy>>>traffic>>>"),
        asset_dir,
    )?;
    Ok(parse_statsquery(&stdout))
}

pub fn parse_statsquery(stdout: &str) -> ProxyTraffic {
    let Ok(value) = serde_json::from_str::<Value>(stdout.trim()) else {
        return ProxyTraffic::default();
    };
    let mut traffic = ProxyTraffic::default();
    let stats = value.get("stat").or_else(|| value.get("Stat"));
    let items: Vec<&Value> = match stats {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(item) if item.is_object() => vec![item],
        _ => return traffic,
    };
    for item in items {
        let name = item
            .get("name")
            .or_else(|| item.get("Name"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !name.contains(">>>proxy>>>") {
            continue;
        }
        let amount = stat_value(item);
        if name.ends_with(">>>uplink") {
            traffic.uplink = amount;
        } else if name.ends_with(">>>downlink") {
            traffic.downlink = amount;
        }
    }
    traffic
}

fn stat_value(item: &Value) -> u64 {
    let raw = item.get("value").or_else(|| item.get("Value"));
    match raw {
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0),
        Some(Value::String(text)) => text.parse().unwrap_or(0),
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiWait {
    Ready,
    Timeout,
    Exited,
}

pub fn wait_api(addr: &str, timeout: Duration, mut still_alive: impl FnMut() -> bool) -> ApiWait {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !still_alive() {
            return ApiWait::Exited;
        }
        if let Some(socket) = resolve_first(addr) {
            if TcpStream::connect_timeout(&socket, Duration::from_millis(300)).is_ok() {
                return ApiWait::Ready;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    ApiWait::Timeout
}

fn run_api(sidecar: &Path, args: &[String], asset_dir: &Path) -> Result<String, EngineError> {
    let mut cmd = Command::new(sidecar);
    cmd.args(args).env(LOCATION_ASSET_ENV, asset_dir);
    super::windows::apply_hidden_console(&mut cmd);
    let output = cmd.output()?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(EngineError::Process(format!(
        "xray api {} failed: {}",
        args.get(1).map(String::as_str).unwrap_or("?"),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn resolve_first(addr: &str) -> Option<SocketAddr> {
    addr.to_socket_addrs().ok()?.next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_api_times_out_when_nothing_listens() {
        assert_eq!(
            wait_api("127.0.0.1:1", Duration::from_millis(50), || true),
            ApiWait::Timeout
        );
    }

    #[test]
    fn wait_api_returns_exited_when_child_dies() {
        let started = Instant::now();
        assert_eq!(
            wait_api("127.0.0.1:1", Duration::from_secs(30), || false),
            ApiWait::Exited
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "dead-child wait should not burn the readiness budget"
        );
    }

    #[test]
    fn add_outbound_args_point_at_the_api_and_the_file() {
        let args = add_outbound_args("127.0.0.1:9090", Path::new("C:/run/outbound.json"));
        assert_eq!(args[0], "api");
        assert_eq!(args[1], "ado");
        assert!(args.iter().any(|arg| arg == "--server=127.0.0.1:9090"));
        assert!(args.iter().any(|arg| arg.contains("outbound.json")));
    }

    #[test]
    fn remove_outbound_args_carry_the_tag() {
        let args = remove_outbound_args("127.0.0.1:9090", "proxy");
        assert_eq!(args[1], "rmo");
        assert!(args.iter().any(|arg| arg == "proxy"));
    }

    #[test]
    fn add_rules_args_replace_and_do_not_append() {
        let args = add_rules_args("127.0.0.1:9090", Path::new("C:/run/routing.next.json"));
        assert_eq!(args[0], "api");
        assert_eq!(args[1], "adrules");
        assert!(args.iter().any(|arg| arg == "--server=127.0.0.1:9090"));
        assert!(args.iter().any(|arg| arg.contains("routing.next.json")));
        assert!(!args.iter().any(|arg| arg == "-append" || arg == "--append"));
    }

    #[test]
    fn statsquery_args_target_proxy_traffic() {
        let args = statsquery_args("127.0.0.1:9090", "outbound>>>proxy>>>traffic>>>");
        assert_eq!(args[0], "api");
        assert_eq!(args[1], "statsquery");
        assert!(args.iter().any(|arg| arg == "--server=127.0.0.1:9090"));
        assert!(args
            .iter()
            .any(|arg| arg == "-pattern=outbound>>>proxy>>>traffic>>>"));
    }

    #[test]
    fn parse_statsquery_reads_proxy_uplink_and_downlink() {
        let traffic = parse_statsquery(
            r#"{
              "stat": [
                {"name": "outbound>>>proxy>>>traffic>>>uplink", "value": "1280"},
                {"name": "outbound>>>proxy>>>traffic>>>downlink", "value": 4096},
                {"name": "outbound>>>direct>>>traffic>>>uplink", "value": "99"}
              ]
            }"#,
        );
        assert_eq!(
            traffic,
            ProxyTraffic {
                uplink: 1280,
                downlink: 4096
            }
        );
    }

    #[test]
    fn parse_statsquery_returns_zero_on_empty_or_invalid() {
        assert_eq!(parse_statsquery(""), ProxyTraffic::default());
        assert_eq!(parse_statsquery("not-json"), ProxyTraffic::default());
        assert_eq!(parse_statsquery("{}"), ProxyTraffic::default());
    }

    #[test]
    fn api_cli_uses_the_same_asset_env_as_the_sidecar() {
        // `adrules` parses geoip:private in this process. The spawn path already
        // sets this env; the CLI must too, or Xray looks next to the sidecar.
        assert_eq!(LOCATION_ASSET_ENV, "XRAY_LOCATION_ASSET");
    }
}
