use std::fmt;

pub const WINTUN_DLL_MISSING: &str = "wintun_dll_missing";
pub const VPN_DENIED: &str = "vpn_denied";
pub const SIDECAR_MISSING: &str = "xray_sidecar_missing";
pub const NO_UNDERLYING_IFACE: &str = "vpn_no_underlying_iface";
pub const HELPER_INSTALL_FAILED: &str = "helper_install_failed";
pub const HELPER_UNAVAILABLE: &str = "helper_unavailable";

#[derive(Debug)]
pub enum EngineError {
    InvalidState { from: String, attempted: String },
    Io(std::io::Error),
    Config(String),
    Process(String),
    NoProfile,
    NeedAdmin,
    UacCancelled,
    WintunMissing,
    VpnDenied,
    SidecarMissing,
    NoUnderlyingIface,
    HelperInstallFailed(String),
    HelperUnavailable(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::InvalidState { from, attempted } => {
                write!(f, "invalid engine state: cannot {attempted} from {from}")
            }
            EngineError::Io(err) => write!(f, "io error: {err}"),
            EngineError::Config(msg) => write!(f, "config error: {msg}"),
            EngineError::Process(msg) => write!(f, "process error: {msg}"),
            EngineError::NoProfile => write!(
                f,
                "no VLESS Reality profile saved; add a subscription or a manual endpoint"
            ),
            EngineError::NeedAdmin => write!(f, "need_admin"),
            EngineError::UacCancelled => write!(f, "uac_cancelled"),
            EngineError::WintunMissing => write!(f, "{WINTUN_DLL_MISSING}"),
            EngineError::VpnDenied => write!(f, "{VPN_DENIED}"),
            EngineError::SidecarMissing => write!(f, "{SIDECAR_MISSING}"),
            EngineError::NoUnderlyingIface => write!(f, "{NO_UNDERLYING_IFACE}"),
            EngineError::HelperInstallFailed(msg) => write!(f, "{HELPER_INSTALL_FAILED}: {msg}"),
            EngineError::HelperUnavailable(msg) => write!(f, "{HELPER_UNAVAILABLE}: {msg}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<std::io::Error> for EngineError {
    fn from(err: std::io::Error) -> Self {
        EngineError::Io(err)
    }
}

impl From<serde_json::Error> for EngineError {
    fn from(err: serde_json::Error) -> Self {
        EngineError::Config(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vpn_denied_and_sidecar_missing_are_stable_codes() {
        assert_eq!(EngineError::VpnDenied.to_string(), VPN_DENIED);
        assert_eq!(EngineError::SidecarMissing.to_string(), SIDECAR_MISSING);
        assert_eq!(
            EngineError::NoUnderlyingIface.to_string(),
            NO_UNDERLYING_IFACE
        );
        assert_eq!(
            EngineError::HelperInstallFailed("copy failed".into()).to_string(),
            "helper_install_failed: copy failed"
        );
        assert_eq!(
            EngineError::HelperUnavailable("socket".into()).to_string(),
            "helper_unavailable: socket"
        );
    }
}
