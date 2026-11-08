use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::engine::error::EngineError;

const RELATIVE: &str = "settings/engine.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EngineSettings {
    pub auto_connect: bool,
    pub start_on_boot: bool,
    #[serde(alias = "persistTunnelOnServerChange")]
    pub kill_switch: bool,
    pub smart_connect: bool,
    pub logs_enabled: bool,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            auto_connect: false,
            start_on_boot: false,
            kill_switch: false,
            smart_connect: false,
            logs_enabled: true,
        }
    }
}

pub fn save_engine_settings_to(
    app_data_dir: &Path,
    settings: &EngineSettings,
) -> Result<(), EngineError> {
    let path = app_data_dir.join(RELATIVE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(settings)?)?;
    Ok(())
}

pub fn load_engine_settings_from(app_data_dir: &Path) -> Result<EngineSettings, EngineError> {
    let path = app_data_dir.join(RELATIVE);
    if !path.is_file() {
        return Ok(EngineSettings::default());
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

pub fn save_engine_settings(app: &AppHandle, settings: &EngineSettings) -> Result<(), EngineError> {
    save_engine_settings_to(&app_data_dir(app)?, settings)
}

pub fn load_engine_settings(app: &AppHandle) -> Result<EngineSettings, EngineError> {
    load_engine_settings_from(&app_data_dir(app)?)
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
    fn flags_default_false() {
        let settings = EngineSettings::default();
        assert!(!settings.auto_connect);
        assert!(!settings.start_on_boot);
        assert!(!settings.kill_switch);
        assert!(!settings.smart_connect);
        assert!(settings.logs_enabled);
    }

    #[test]
    fn load_missing_returns_defaults() {
        let dir = std::env::temp_dir().join(format!(
            "xraybox-engine-settings-missing-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let loaded = load_engine_settings_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(loaded, EngineSettings::default());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "xraybox-engine-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let settings = EngineSettings {
            auto_connect: true,
            start_on_boot: true,
            kill_switch: true,
            smart_connect: false,
            logs_enabled: false,
        };
        save_engine_settings_to(&dir, &settings).unwrap();
        let loaded = load_engine_settings_from(&dir).unwrap();
        let written = fs::read_to_string(dir.join(RELATIVE)).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(loaded, settings);
        assert!(written.contains("\"killSwitch\": true"));
        assert!(!written.contains("persistTunnelOnServerChange"));
    }

    #[test]
    fn reads_legacy_persist_tunnel_flag() {
        let json = r#"{"persistTunnelOnServerChange":true}"#;
        let settings: EngineSettings = serde_json::from_str(json).unwrap();
        assert!(settings.kill_switch);
        assert!(!settings.auto_connect);
        assert!(!settings.start_on_boot);
        assert!(settings.logs_enabled);
    }
}
