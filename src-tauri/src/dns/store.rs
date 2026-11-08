use std::fs;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use super::DnsSettings;
use crate::engine::error::EngineError;

const RELATIVE: &str = "settings/dns.json";

pub fn save_dns_settings_to(
    app_data_dir: &Path,
    settings: &DnsSettings,
) -> Result<(), EngineError> {
    settings.validate()?;
    let path = app_data_dir.join(RELATIVE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(settings)?)?;
    Ok(())
}

pub fn load_dns_settings_from(app_data_dir: &Path) -> Result<DnsSettings, EngineError> {
    let path = app_data_dir.join(RELATIVE);
    if !path.is_file() {
        return Ok(DnsSettings::default());
    }
    let settings: DnsSettings = serde_json::from_str(&fs::read_to_string(path)?)?;
    settings.validate()?;
    Ok(settings)
}

pub fn save_dns_settings(app: &AppHandle, settings: &DnsSettings) -> Result<(), EngineError> {
    save_dns_settings_to(&app_data_dir(app)?, settings)
}

pub fn load_dns_settings(app: &AppHandle) -> Result<DnsSettings, EngineError> {
    load_dns_settings_from(&app_data_dir(app)?)
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
    fn missing_file_returns_defaults() {
        let dir = std::env::temp_dir().join(format!("xraybox-dns-missing-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let loaded = load_dns_settings_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(loaded, DnsSettings::default());
        assert!(!loaded.fallback_to_system);
    }

    #[test]
    fn old_file_without_fallback_still_loads() {
        let dir = std::env::temp_dir().join(format!("xraybox-dns-old-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings").join("dns.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"remoteServers":["1.1.1.1","8.8.8.8"]}"#).unwrap();
        let loaded = load_dns_settings_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(loaded.remote_servers, ["1.1.1.1", "8.8.8.8"]);
        assert!(!loaded.fallback_to_system);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "xraybox-dns-store-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let settings = DnsSettings {
            remote_servers: vec!["1.0.0.1".into(), "8.8.4.4".into()],
            fallback_to_system: false,
        };
        save_dns_settings_to(&dir, &settings).unwrap();
        let loaded = load_dns_settings_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(loaded, settings);
    }
}
