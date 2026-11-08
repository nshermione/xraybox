use std::net::Ipv4Addr;
use std::process::Command;

use crate::engine::error::EngineError;

pub const TUN_INTERFACE_NAME: &str = "xraybox-tun";
pub const TUN_IPV4_PREFIX: &str = "172.19.0.0/30";
pub const TUN_IPV4_GATEWAY: &str = "172.19.0.1/30";
pub const SKIP_ELEVATE_FLAG: &str = "--xraybox-skip-elevate";
pub const CONNECT_AFTER_ELEVATE_FLAG: &str = "--xraybox-connect";
pub const ERROR_CANCELLED: u32 = 1223;
/// Win32 `CREATE_NO_WINDOW` — spawn helpers/sidecars without a console.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn windows_hide_console_flags() -> u32 {
    CREATE_NO_WINDOW
}

pub fn apply_hidden_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(windows_hide_console_flags());
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupElevation {
    AlreadyElevated,
    NeedsRelaunch,
    SkipRelaunch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectElevation {
    AlreadyElevated,
    NeedsRelaunch,
}

pub fn plan_startup_elevation(elevated: bool, skip_flag_present: bool) -> StartupElevation {
    if elevated {
        StartupElevation::AlreadyElevated
    } else if skip_flag_present {
        StartupElevation::SkipRelaunch
    } else {
        StartupElevation::NeedsRelaunch
    }
}

pub fn args_request_skip_elevate<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == SKIP_ELEVATE_FLAG)
}

pub fn relaunch_args_from(existing: &[&str]) -> Vec<String> {
    existing
        .iter()
        .filter(|arg| **arg != SKIP_ELEVATE_FLAG)
        .map(|arg| (*arg).to_string())
        .chain(std::iter::once(SKIP_ELEVATE_FLAG.to_string()))
        .collect()
}

pub fn plan_connect_elevation(elevated: bool) -> ConnectElevation {
    if elevated {
        ConnectElevation::AlreadyElevated
    } else {
        ConnectElevation::NeedsRelaunch
    }
}

pub fn args_request_connect_after_elevate<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == CONNECT_AFTER_ELEVATE_FLAG)
}

pub fn connect_after_elevate_requested() -> bool {
    args_request_connect_after_elevate(std::env::args())
}

pub fn relaunch_args_for_connect(existing: &[&str]) -> Vec<String> {
    let mut args = relaunch_args_from(existing);
    if !args.iter().any(|arg| arg == CONNECT_AFTER_ELEVATE_FLAG) {
        args.push(CONNECT_AFTER_ELEVATE_FLAG.to_string());
    }
    args
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn elevate_for_connect() -> Result<(), EngineError> {
    #[cfg(windows)]
    {
        match plan_connect_elevation(is_elevated()) {
            ConnectElevation::AlreadyElevated => Ok(()),
            ConnectElevation::NeedsRelaunch => {
                let existing: Vec<String> = std::env::args().skip(1).collect();
                let existing_refs: Vec<&str> = existing.iter().map(String::as_str).collect();
                let owned = relaunch_args_for_connect(&existing_refs);
                let args: Vec<&str> = owned.iter().map(String::as_str).collect();
                relaunch_self_elevated(&args)
            }
        }
    }
    #[cfg(not(windows))]
    {
        Err(EngineError::NeedAdmin)
    }
}

pub fn map_shell_execute_error(success: bool, last_error: u32) -> Result<(), EngineError> {
    if success {
        Ok(())
    } else if last_error == ERROR_CANCELLED {
        Err(EngineError::UacCancelled)
    } else {
        Err(EngineError::Process(format!(
            "elevation failed (win32 {last_error})"
        )))
    }
}

pub fn elevate_at_startup() {
    #[cfg(windows)]
    {
        match plan_startup_elevation(is_elevated(), args_request_skip_elevate(std::env::args())) {
            StartupElevation::AlreadyElevated | StartupElevation::SkipRelaunch => {}
            StartupElevation::NeedsRelaunch => {
                let existing: Vec<String> = std::env::args().skip(1).collect();
                let existing_refs: Vec<&str> = existing.iter().map(String::as_str).collect();
                let owned = relaunch_args_from(&existing_refs);
                let args: Vec<&str> = owned.iter().map(String::as_str).collect();
                match relaunch_self_elevated(&args) {
                    Ok(()) => std::process::exit(0),
                    Err(_) => {}
                }
            }
        }
    }
}

#[cfg(windows)]
pub fn is_elevated() -> bool {
    windows_imp::is_elevated()
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

#[cfg(windows)]
pub fn relaunch_self_elevated(args: &[&str]) -> Result<(), EngineError> {
    windows_imp::relaunch_self_elevated(args)
}

#[cfg(not(windows))]
#[cfg_attr(not(windows), allow(dead_code))]
pub fn relaunch_self_elevated(_args: &[&str]) -> Result<(), EngineError> {
    Err(EngineError::NeedAdmin)
}

pub fn tun_interface_present() -> bool {
    #[cfg(windows)]
    {
        return windows_tun_present();
    }
    #[cfg(target_os = "macos")]
    {
        return crate::engine::macos::tun_present();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

#[cfg(windows)]
fn windows_tun_present() -> bool {
    let mut cmd = Command::new("netsh");
    cmd.args(["interface", "show", "interface"]);
    apply_hidden_console(&mut cmd);
    cmd.output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(TUN_INTERFACE_NAME))
        .unwrap_or(false)
}

pub fn is_tun_adapter_name(name: &str) -> bool {
    name.to_ascii_lowercase().contains(TUN_INTERFACE_NAME)
}

pub fn is_tun_ipv4(ip: Ipv4Addr) -> bool {
    let Some((network, prefix)) = tun_ipv4_network() else {
        return false;
    };
    let mask = if prefix == 0 {
        0
    } else {
        !0u32 << (32 - prefix)
    };
    (u32::from(ip) & mask) == (u32::from(network) & mask)
}

pub fn is_usable_physical_ipv4(ip: Ipv4Addr) -> bool {
    !ip.is_unspecified() && !ip.is_loopback() && !ip.is_link_local() && !is_tun_ipv4(ip)
}

/// IPv4 on a real NIC. After TUN takes `0.0.0.0/0`, an unbound TCP connect
/// completes against the local stack (~0ms). Bind ping sockets here instead.
pub fn physical_ipv4() -> Option<Ipv4Addr> {
    #[cfg(windows)]
    {
        adapters::physical_ipv4()
    }
    #[cfg(target_os = "macos")]
    {
        crate::engine::macos::physical_ipv4()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

fn tun_ipv4_network() -> Option<(Ipv4Addr, u8)> {
    let (addr, prefix) = TUN_IPV4_PREFIX.split_once('/')?;
    Some((addr.parse().ok()?, prefix.parse().ok()?))
}

#[cfg(windows)]
mod adapters {
    use std::net::Ipv4Addr;
    use std::ptr;

    use windows_sys::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST,
        GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, IF_TYPE_SOFTWARE_LOOPBACK,
        IP_ADAPTER_ADDRESSES_LH, IP_ADAPTER_UNICAST_ADDRESS_LH,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    use super::{is_tun_adapter_name, is_usable_physical_ipv4};

    pub fn physical_ipv4() -> Option<Ipv4Addr> {
        let flags = GAA_FLAG_INCLUDE_GATEWAYS
            | GAA_FLAG_SKIP_ANYCAST
            | GAA_FLAG_SKIP_MULTICAST
            | GAA_FLAG_SKIP_DNS_SERVER;
        let mut size = 15 * 1024u32;
        let mut buffer = vec![0u8; size as usize];
        for _ in 0..3 {
            let status = unsafe {
                GetAdaptersAddresses(
                    AF_INET.into(),
                    flags,
                    ptr::null(),
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                )
            };
            if status == 0 {
                break;
            }
            if status != ERROR_BUFFER_OVERFLOW || size == 0 {
                return None;
            }
            buffer.resize(size as usize, 0);
        }

        let mut fallback = None;
        let mut current = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        while !current.is_null() {
            let adapter = unsafe { &*current };
            current = adapter.Next;
            if adapter.OperStatus != IfOperStatusUp || adapter.IfType == IF_TYPE_SOFTWARE_LOOPBACK {
                continue;
            }
            if is_tun_adapter_name(&wide_to_string(adapter.FriendlyName))
                || is_tun_adapter_name(&wide_to_string(adapter.Description))
            {
                continue;
            }
            let has_gateway = !adapter.FirstGatewayAddress.is_null();
            let mut unicast = adapter.FirstUnicastAddress;
            while !unicast.is_null() {
                let row = unsafe { &*unicast };
                unicast = row.Next;
                let Some(ip) = ipv4_from_unicast(row) else {
                    continue;
                };
                if !is_usable_physical_ipv4(ip) {
                    continue;
                }
                if has_gateway {
                    return Some(ip);
                }
                if fallback.is_none() {
                    fallback = Some(ip);
                }
            }
        }
        fallback
    }

    fn ipv4_from_unicast(row: &IP_ADAPTER_UNICAST_ADDRESS_LH) -> Option<Ipv4Addr> {
        let addr = row.Address.lpSockaddr;
        if addr.is_null() {
            return None;
        }
        unsafe {
            if (*addr).sa_family != AF_INET {
                return None;
            }
            let sin = addr.cast::<SOCKADDR_IN>();
            let bytes = (*sin).sin_addr.S_un.S_un_b;
            Some(Ipv4Addr::new(
                bytes.s_b1, bytes.s_b2, bytes.s_b3, bytes.s_b4,
            ))
        }
    }

    fn wide_to_string(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        unsafe {
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
        }
    }
}

#[cfg(windows)]
mod windows_imp {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NO_CONSOLE, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use super::{map_shell_execute_error, EngineError};

    #[repr(C)]
    struct TokenElevation {
        token_is_elevated: u32,
    }

    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_ELEVATION: u32 = 20;

    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(
            process: *mut c_void,
            desired_access: u32,
            token_handle: *mut *mut c_void,
        ) -> i32;
        fn GetTokenInformation(
            token_handle: *mut c_void,
            token_information_class: u32,
            token_information: *mut c_void,
            token_information_length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
    }

    pub fn is_elevated() -> bool {
        unsafe {
            let mut token = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation = TokenElevation {
                token_is_elevated: 0,
            };
            let mut size = 0u32;
            let ok = GetTokenInformation(
                token,
                TOKEN_ELEVATION,
                (&mut elevation as *mut TokenElevation).cast(),
                std::mem::size_of::<TokenElevation>() as u32,
                &mut size,
            );
            CloseHandle(token);
            ok != 0 && elevation.token_is_elevated != 0
        }
    }

    fn wide_os(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        value
            .as_ref()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn quote_arg(arg: &str) -> String {
        if arg.is_empty() {
            return "\"\"".into();
        }
        let needs_quotes = arg
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '\''));
        if !needs_quotes {
            return arg.to_string();
        }
        let escaped = arg.replace('"', "\\\"");
        format!("\"{escaped}\"")
    }

    pub fn relaunch_self_elevated(args: &[&str]) -> Result<(), EngineError> {
        let exe = std::env::current_exe()
            .map_err(|err| EngineError::Process(format!("cannot resolve current exe: {err}")))?;
        let exe_wide = wide_os(&exe);
        let verb = wide_os("runas");
        let params = args
            .iter()
            .map(|arg| quote_arg(arg))
            .collect::<Vec<_>>()
            .join(" ");
        let params_wide = wide_os(&params);

        let mut info = unsafe { std::mem::zeroed::<SHELLEXECUTEINFOW>() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        // Show the elevated GUI (`SW_SHOWNORMAL`). Only suppress a console —
        // `SW_HIDE` would hide the app window itself.
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NO_CONSOLE;
        info.lpVerb = verb.as_ptr();
        info.lpFile = exe_wide.as_ptr();
        info.lpParameters = if params.is_empty() {
            std::ptr::null()
        } else {
            params_wide.as_ptr()
        };
        info.nShow = SW_SHOWNORMAL;

        let ok = unsafe { ShellExecuteExW(&mut info) != 0 };
        if !ok {
            let last_error = unsafe { GetLastError() };
            return map_shell_execute_error(false, last_error);
        }
        if !info.hProcess.is_null() {
            unsafe {
                CloseHandle(info.hProcess);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::error::EngineError;

    #[test]
    fn windows_hide_console_flag_is_create_no_window() {
        assert_eq!(windows_hide_console_flags(), 0x0800_0000);
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
    }

    #[test]
    fn tun_prefix_matches_gateway_subnet() {
        assert_eq!(TUN_IPV4_PREFIX, "172.19.0.0/30");
        assert_eq!(TUN_IPV4_GATEWAY, "172.19.0.1/30");
    }

    #[test]
    fn tun_ipv4_covers_wintun_prefix_only() {
        assert!(is_tun_ipv4(Ipv4Addr::new(172, 19, 0, 0)));
        assert!(is_tun_ipv4(Ipv4Addr::new(172, 19, 0, 1)));
        assert!(is_tun_ipv4(Ipv4Addr::new(172, 19, 0, 2)));
        assert!(is_tun_ipv4(Ipv4Addr::new(172, 19, 0, 3)));
        assert!(!is_tun_ipv4(Ipv4Addr::new(172, 19, 0, 4)));
        assert!(!is_tun_ipv4(Ipv4Addr::new(192, 168, 1, 10)));
    }

    #[test]
    fn physical_ipv4_skips_loopback_link_local_and_tun() {
        assert!(!is_usable_physical_ipv4(Ipv4Addr::LOCALHOST));
        assert!(!is_usable_physical_ipv4(Ipv4Addr::UNSPECIFIED));
        assert!(!is_usable_physical_ipv4(Ipv4Addr::new(169, 254, 1, 1)));
        assert!(!is_usable_physical_ipv4(Ipv4Addr::new(172, 19, 0, 2)));
        assert!(is_usable_physical_ipv4(Ipv4Addr::new(192, 168, 1, 10)));
    }

    #[test]
    fn tun_adapter_name_matches_wintun_iface() {
        assert!(is_tun_adapter_name("xraybox-tun"));
        assert!(is_tun_adapter_name("XRAYBOX-TUN"));
        assert!(!is_tun_adapter_name("Ethernet"));
        assert!(!is_tun_adapter_name("Wi-Fi"));
    }

    #[test]
    fn is_elevated_returns_bool() {
        let _elevated = is_elevated();
    }

    #[test]
    fn need_admin_maps_to_snapshot_code() {
        assert_eq!(EngineError::NeedAdmin.to_string(), "need_admin");
    }

    #[test]
    fn uac_cancelled_maps_to_snapshot_code() {
        assert_eq!(EngineError::UacCancelled.to_string(), "uac_cancelled");
    }

    #[test]
    fn wintun_missing_maps_to_snapshot_code() {
        assert_eq!(EngineError::WintunMissing.to_string(), "wintun_dll_missing");
    }

    #[test]
    fn already_elevated_does_not_relaunch() {
        assert_eq!(
            plan_startup_elevation(true, false),
            StartupElevation::AlreadyElevated
        );
        assert_eq!(
            plan_startup_elevation(true, true),
            StartupElevation::AlreadyElevated
        );
    }

    #[test]
    fn not_elevated_plans_relaunch() {
        assert_eq!(
            plan_startup_elevation(false, false),
            StartupElevation::NeedsRelaunch
        );
    }

    #[test]
    fn skip_flag_prevents_elevation_loop() {
        assert_eq!(
            plan_startup_elevation(false, true),
            StartupElevation::SkipRelaunch
        );
        assert!(args_request_skip_elevate(["--foo", SKIP_ELEVATE_FLAG]));
        assert!(!args_request_skip_elevate(["--foo"]));
        let args = relaunch_args_from(&["--dev", SKIP_ELEVATE_FLAG]);
        assert_eq!(
            args,
            vec!["--dev".to_string(), SKIP_ELEVATE_FLAG.to_string()]
        );
    }

    #[test]
    fn cancel_does_not_retry_relaunch() {
        let err = map_shell_execute_error(false, ERROR_CANCELLED).unwrap_err();
        assert_eq!(err.to_string(), "uac_cancelled");
        assert_eq!(
            plan_startup_elevation(false, true),
            StartupElevation::SkipRelaunch
        );
    }

    #[test]
    fn other_shell_error_is_not_cancel() {
        let err = map_shell_execute_error(false, 5).unwrap_err();
        assert!(err.to_string().contains("win32 5"));
        assert!(map_shell_execute_error(true, 0).is_ok());
    }

    #[test]
    fn connect_retries_elevation_even_after_startup_skip() {
        assert_eq!(
            plan_connect_elevation(false),
            ConnectElevation::NeedsRelaunch
        );
        assert_eq!(
            plan_connect_elevation(true),
            ConnectElevation::AlreadyElevated
        );
        assert!(args_request_connect_after_elevate([
            "--foo",
            CONNECT_AFTER_ELEVATE_FLAG
        ]));
        assert!(!args_request_connect_after_elevate([
            "--foo",
            SKIP_ELEVATE_FLAG
        ]));
        let args = relaunch_args_for_connect(&["--dev", SKIP_ELEVATE_FLAG]);
        assert_eq!(
            args,
            vec![
                "--dev".to_string(),
                SKIP_ELEVATE_FLAG.to_string(),
                CONNECT_AFTER_ELEVATE_FLAG.to_string()
            ]
        );
    }
}
