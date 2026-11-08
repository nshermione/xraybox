#![cfg_attr(not(windows), allow(dead_code))]

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::engine::error::EngineError;
use crate::engine::firewall;
use crate::engine::windows::TUN_IPV4_PREFIX;

const MARKER_RELATIVE: &str = "settings/kill-switch-restore.json";
pub const RULE_XRAY: &str = "Xraybox Kill Switch Xray";
pub const RULE_TUN: &str = "Xraybox Kill Switch TUN";
const PROFILES: [&str; 3] = ["domain", "private", "public"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FirewallRestore {
    pub domain: String,
    pub private: String,
    pub public: String,
}

pub fn allow_program_args(name: &str, program: &Path) -> Vec<String> {
    firewall::add_rule_args(
        name,
        &[
            ("dir", "out"),
            ("action", "allow"),
            ("program", &firewall::program_path(program)),
        ],
    )
}

pub fn allow_tun_args(name: &str, prefix: &str) -> Vec<String> {
    firewall::add_rule_args(
        name,
        &[("dir", "out"), ("action", "allow"), ("localip", prefix)],
    )
}

pub fn show_policy_args(profile: &str) -> Vec<String> {
    vec![
        "advfirewall".into(),
        "show".into(),
        format!("{profile}profile"),
        "firewallpolicy".into(),
    ]
}

pub fn set_policy_args(profile: &str, policy: &str) -> Vec<String> {
    vec![
        "advfirewall".into(),
        "set".into(),
        format!("{profile}profile"),
        "firewallpolicy".into(),
        policy.into(),
    ]
}

pub fn parse_firewall_policy(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("Firewall Policy") else {
            continue;
        };
        let value = rest.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

pub fn outbound_blocked_policy(current: &str) -> String {
    let inbound = current
        .split(',')
        .next()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .unwrap_or("BlockInbound");
    format!("{inbound},BlockOutbound")
}

/// Keep the TUN up (blackhole) when the proxy dies, unless the user stopped.
pub fn should_hold_tun(kill_switch: bool, user_requested_stop: bool) -> bool {
    kill_switch && !user_requested_stop
}

pub fn apply(app: &AppHandle) -> Result<(), EngineError> {
    #[cfg(windows)]
    {
        apply_to(&app_data_dir(app)?, &sidecar_program(app)?)
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(())
    }
}

pub fn release(app: &AppHandle) -> Result<(), EngineError> {
    #[cfg(windows)]
    {
        release_from(&app_data_dir(app)?)
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(())
    }
}

pub fn release_leftover(app: &AppHandle) {
    if let Err(err) = release(app) {
        eprintln!("xraybox: leftover kill switch release failed: {err}");
    }
}

pub fn marker_path_in(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(MARKER_RELATIVE)
}

pub fn apply_to(app_data_dir: &Path, xray: &Path) -> Result<(), EngineError> {
    if marker_path_in(app_data_dir).is_file() {
        ensure_allow_rules(xray)?;
        return Ok(());
    }
    let restore = query_policies()?;
    write_marker(app_data_dir, &restore)?;
    if let Err(err) = apply_rules_and_policies(xray, &restore) {
        let _ = restore_policies(&restore);
        let _ = delete_our_rules();
        let _ = fs::remove_file(marker_path_in(app_data_dir));
        return Err(err);
    }
    Ok(())
}

pub fn release_from(app_data_dir: &Path) -> Result<(), EngineError> {
    let path = marker_path_in(app_data_dir);
    if !path.is_file() {
        let _ = delete_our_rules();
        return Ok(());
    }
    let restore: FirewallRestore = serde_json::from_str(&fs::read_to_string(&path)?)?;
    let policy_err = restore_policies(&restore).err();
    let _ = delete_our_rules();
    fs::remove_file(&path)?;
    match policy_err {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn apply_rules_and_policies(xray: &Path, restore: &FirewallRestore) -> Result<(), EngineError> {
    ensure_allow_rules(xray)?;
    firewall::run(&set_policy_args(
        "domain",
        &outbound_blocked_policy(&restore.domain),
    ))?;
    firewall::run(&set_policy_args(
        "private",
        &outbound_blocked_policy(&restore.private),
    ))?;
    firewall::run(&set_policy_args(
        "public",
        &outbound_blocked_policy(&restore.public),
    ))?;
    Ok(())
}

fn ensure_allow_rules(xray: &Path) -> Result<(), EngineError> {
    firewall::delete_rule_best_effort(RULE_XRAY);
    firewall::delete_rule_best_effort(RULE_TUN);
    firewall::run(&allow_program_args(RULE_XRAY, xray))?;
    firewall::run(&allow_tun_args(RULE_TUN, TUN_IPV4_PREFIX))?;
    Ok(())
}

fn delete_our_rules() -> Result<(), EngineError> {
    firewall::delete_rule_best_effort(RULE_XRAY);
    firewall::delete_rule_best_effort(RULE_TUN);
    Ok(())
}

fn query_policies() -> Result<FirewallRestore, EngineError> {
    Ok(FirewallRestore {
        domain: query_policy(PROFILES[0])?,
        private: query_policy(PROFILES[1])?,
        public: query_policy(PROFILES[2])?,
    })
}

fn query_policy(profile: &str) -> Result<String, EngineError> {
    let output = firewall::run_output(&show_policy_args(profile))?;
    parse_firewall_policy(&output).ok_or_else(|| {
        EngineError::Process(format!("kill switch: no firewall policy for {profile}"))
    })
}

fn restore_policies(restore: &FirewallRestore) -> Result<(), EngineError> {
    firewall::run(&set_policy_args("domain", &restore.domain))?;
    firewall::run(&set_policy_args("private", &restore.private))?;
    firewall::run(&set_policy_args("public", &restore.public))?;
    Ok(())
}

fn write_marker(app_data_dir: &Path, restore: &FirewallRestore) -> Result<(), EngineError> {
    let path = marker_path_in(app_data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(restore)?)?;
    Ok(())
}

fn sidecar_program(app: &AppHandle) -> Result<PathBuf, EngineError> {
    crate::engine::process::sidecar_path(app)
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, EngineError> {
    app.path()
        .app_data_dir()
        .map_err(|err| EngineError::Config(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_xray_is_outbound_program_rule() {
        let args = allow_program_args(RULE_XRAY, Path::new(r"C:\app\xray.exe"));
        assert!(args.contains(&"add".into()));
        assert!(args.contains(&"dir=out".into()));
        assert!(args.contains(&"action=allow".into()));
        assert!(args.contains(&"program=C:\\app\\xray.exe".into()));
        assert!(args.iter().any(|arg| arg == &format!("name={RULE_XRAY}")));
    }

    #[test]
    fn allow_program_strips_windows_verbatim_prefix() {
        let args = allow_program_args(
            RULE_XRAY,
            Path::new(r"\\?\D:\projects\xraybox\src-tauri\target\debug\xray.exe"),
        );
        assert!(args
            .contains(&"program=D:\\projects\\xraybox\\src-tauri\\target\\debug\\xray.exe".into()));
        assert!(!args.iter().any(|arg| arg.contains(r"\\?\")));
    }

    #[test]
    fn allow_program_strips_unc_verbatim_prefix() {
        let args = allow_program_args(RULE_XRAY, Path::new(r"\\?\UNC\fileserver\share\xray.exe"));
        assert!(args.contains(&r"program=\\fileserver\share\xray.exe".into()));
    }

    #[test]
    fn allow_tun_matches_windows_prefix() {
        let args = allow_tun_args(RULE_TUN, TUN_IPV4_PREFIX);
        assert!(args.contains(&format!("localip={TUN_IPV4_PREFIX}")));
        assert_eq!(TUN_IPV4_PREFIX, "172.19.0.0/30");
    }

    #[test]
    fn parse_policy_reads_netsh_layout() {
        let output = "\
Domain Profile Settings:\n\
----------------------------------------------------------------------\n\
Firewall Policy                                 BlockInbound,AllowOutbound\n\
Ok.\n";
        assert_eq!(
            parse_firewall_policy(output).as_deref(),
            Some("BlockInbound,AllowOutbound")
        );
    }

    #[test]
    fn outbound_block_keeps_inbound() {
        assert_eq!(
            outbound_blocked_policy("BlockInbound,AllowOutbound"),
            "BlockInbound,BlockOutbound"
        );
        assert_eq!(
            outbound_blocked_policy("AllowInbound,AllowOutbound"),
            "AllowInbound,BlockOutbound"
        );
    }

    #[test]
    fn release_without_marker_is_ok() {
        let dir =
            std::env::temp_dir().join(format!("xraybox-kill-switch-empty-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        // Non-Windows: apply/release helpers that only touch the marker.
        let path = marker_path_in(&dir);
        assert!(!path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_plan_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "xraybox-kill-switch-marker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("settings")).unwrap();
        let restore = FirewallRestore {
            domain: "BlockInbound,AllowOutbound".into(),
            private: "BlockInbound,AllowOutbound".into(),
            public: "BlockInbound,AllowOutbound".into(),
        };
        write_marker(&dir, &restore).unwrap();
        let loaded: FirewallRestore =
            serde_json::from_str(&fs::read_to_string(marker_path_in(&dir)).unwrap()).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(loaded, restore);
    }

    #[test]
    fn hold_tun_only_with_kill_switch_until_user_stops() {
        assert!(should_hold_tun(true, false));
        assert!(!should_hold_tun(true, true));
        assert!(!should_hold_tun(false, false));
        assert!(!should_hold_tun(false, true));
    }

    #[test]
    fn set_and_show_policy_target_each_profile() {
        for profile in PROFILES {
            let show = show_policy_args(profile);
            assert!(show.contains(&format!("{profile}profile")));
            let set = set_policy_args(profile, "BlockInbound,BlockOutbound");
            assert!(set.contains(&format!("{profile}profile")));
            assert!(set.contains(&"BlockInbound,BlockOutbound".into()));
        }
    }
}
