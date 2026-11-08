use std::fs;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use super::VlessReality;
use crate::engine::error::EngineError;

const PROFILE_RELATIVE: &str = "profiles/manual.json";

pub fn profile_path_in(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(PROFILE_RELATIVE)
}

pub fn save_manual_profile_to(
    app_data_dir: &Path,
    profile: &VlessReality,
) -> Result<(), EngineError> {
    profile.validate()?;
    let path = profile_path_in(app_data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(profile)?)?;
    Ok(())
}

pub fn load_manual_profile_from(app_data_dir: &Path) -> Result<Option<VlessReality>, EngineError> {
    let path = profile_path_in(app_data_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)?;
    let profile: VlessReality = serde_json::from_str(&text)?;
    profile.validate()?;
    Ok(Some(profile))
}

pub fn save_manual_profile(app: &AppHandle, profile: &VlessReality) -> Result<(), EngineError> {
    save_manual_profile_to(&app_data_dir(app)?, profile)
}

pub fn load_manual_profile(app: &AppHandle) -> Result<Option<VlessReality>, EngineError> {
    let dir = app_data_dir(app)?;
    if let Some(profile) = load_manual_profile_from(&dir)? {
        return Ok(Some(profile));
    }
    #[cfg(debug_assertions)]
    {
        let seed = debug_seed_profile();
        save_manual_profile_to(&dir, &seed)?;
        return Ok(Some(seed));
    }
    #[cfg(not(debug_assertions))]
    Ok(None)
}

#[cfg(debug_assertions)]
fn debug_seed_profile() -> VlessReality {
    VlessReality {
        remark: "Manual endpoint".into(),
        server: "edge.example.com".into(),
        server_port: 443,
        uuid: "11111111-1111-1111-1111-111111111111".into(),
        server_name: "www.cloudflare.com".into(),
        public_key: "abc".into(),
        short_id: "dead".into(),
        flow: None,
        fingerprint: "chrome".into(),
        spider_x: None,
        mldsa65_verify: None,
    }
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, EngineError> {
    app.path()
        .app_data_dir()
        .map_err(|err| EngineError::Config(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::vless::VlessReality;

    fn sample_profile() -> VlessReality {
        VlessReality {
            remark: "manual".into(),
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
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "xraybox-profile-store-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let profile = sample_profile();
        save_manual_profile_to(&dir, &profile).unwrap();
        let loaded = load_manual_profile_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(loaded, Some(profile));
    }

    #[test]
    fn load_missing_returns_none() {
        let dir =
            std::env::temp_dir().join(format!("xraybox-profile-missing-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let loaded = load_manual_profile_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(loaded, None);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_seed_profile_is_valid() {
        debug_seed_profile().validate().unwrap();
    }
}
