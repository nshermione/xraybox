use std::fs;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use super::catalog::{catalog_defaults, pack_def, validate_packs, PackState, RETIRED_PACK_IDS};
use super::compile::RouteSettings;
use crate::engine::error::EngineError;

const RELATIVE: &str = "settings/route.json";
const PACKS_RELATIVE: &str = "settings/packs.json";

pub fn load_route_settings_from(app_data_dir: &Path) -> Result<RouteSettings, EngineError> {
    let path = app_data_dir.join(RELATIVE);
    if !path.is_file() {
        return Ok(RouteSettings::default());
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

pub fn load_route_settings(app: &AppHandle) -> Result<RouteSettings, EngineError> {
    load_route_settings_from(&app_data_dir(app)?)
}

pub fn save_packs_to(app_data_dir: &Path, packs: &[PackState]) -> Result<(), EngineError> {
    validate_packs(packs)?;
    let path = app_data_dir.join(PACKS_RELATIVE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(packs)?)?;
    Ok(())
}

pub fn load_packs_from(app_data_dir: &Path) -> Result<Vec<PackState>, EngineError> {
    let path = app_data_dir.join(PACKS_RELATIVE);
    if !path.is_file() {
        return Ok(catalog_defaults());
    }
    let mut packs: Vec<PackState> = serde_json::from_str(&fs::read_to_string(path)?)?;
    packs.retain(|pack| !RETIRED_PACK_IDS.contains(&pack.id.as_str()));
    // Drop service overrides whose catalog entry was removed (e.g. gmail,
    // google-ads, baidu — China services are now google-cn / apple-cn).
    for pack in &mut packs {
        if let Some(def) = pack_def(&pack.id) {
            pack.services
                .retain(|service| def.services.iter().any(|known| known.id == service.id));
        }
    }
    validate_packs(&packs)?;
    Ok(merge_with_defaults(packs))
}

pub fn save_packs(app: &AppHandle, packs: &[PackState]) -> Result<(), EngineError> {
    save_packs_to(&app_data_dir(app)?, packs)
}

pub fn load_packs(app: &AppHandle) -> Result<Vec<PackState>, EngineError> {
    load_packs_from(&app_data_dir(app)?)
}

fn merge_with_defaults(saved: Vec<PackState>) -> Vec<PackState> {
    catalog_defaults()
        .into_iter()
        .map(|default| {
            saved
                .iter()
                .find(|pack| pack.id == default.id)
                .cloned()
                .unwrap_or(default)
        })
        .collect()
}

pub fn geo_asset_dir(app: &AppHandle) -> Result<PathBuf, EngineError> {
    Ok(app_data_dir(app)?.join("assets"))
}

/// Bundled seeds live under `src-tauri/resources/assets/` (dev) and the
/// packaged resource dir (`assets/` or `resources/assets/`).
pub fn bundled_geo_dirs(app: &AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(resource) = app.path().resource_dir() {
        dirs.push(resource.join("assets"));
        dirs.push(resource.join("resources").join("assets"));
    }
    dirs.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("assets"),
    );
    dirs
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
    fn existing_route_json_with_rule_sets_still_loads() {
        let dir = std::env::temp_dir().join(format!("xraybox-route-legacy-{}", std::process::id()));
        fs::create_dir_all(dir.join("settings")).unwrap();
        fs::write(
            dir.join("settings/route.json"),
            r#"{"bypassLan":true,"extraRules":[],"ruleSets":[{"tag":"geosite-cn"}]}"#,
        )
        .unwrap();
        let loaded = load_route_settings_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert!(loaded.bypass_lan);
        assert!(loaded.extra_rules.is_empty());
    }

    #[test]
    fn existing_packs_json_with_dropped_service_still_loads() {
        let dir =
            std::env::temp_dir().join(format!("xraybox-packs-dropped-{}", std::process::id()));
        fs::create_dir_all(dir.join("settings")).unwrap();
        fs::write(
            dir.join("settings/packs.json"),
            r#"[{"id":"google","enabled":true,"action":"proxy","services":[{"id":"gmail","action":"direct"},{"id":"youtube","action":"direct"}]}]"#,
        )
        .unwrap();
        let loaded = load_packs_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        let google = loaded.iter().find(|pack| pack.id == "google").unwrap();
        assert_eq!(google.services.len(), 1);
        assert_eq!(google.services[0].id, "youtube");
    }

    #[test]
    fn existing_packs_json_still_loads() {
        let dir = std::env::temp_dir().join(format!("xraybox-packs-legacy-{}", std::process::id()));
        fs::create_dir_all(dir.join("settings")).unwrap();
        fs::write(
            dir.join("settings/packs.json"),
            r#"[{"id":"china","enabled":true,"action":"direct","services":[]},{"id":"google","enabled":false,"action":"proxy","services":[]}]"#,
        )
        .unwrap();
        let loaded = load_packs_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        let china = loaded.iter().find(|pack| pack.id == "china").unwrap();
        assert!(china.enabled);
        assert!(loaded
            .iter()
            .any(|pack| pack.id == "telegram" && !pack.enabled));
        assert_eq!(loaded.len(), crate::routing::catalog::PACK_ORDER.len());
    }

    #[test]
    fn saved_ads_pack_is_discarded_even_when_enabled() {
        let dir = std::env::temp_dir().join(format!("xraybox-packs-ads-{}", std::process::id()));
        fs::create_dir_all(dir.join("settings")).unwrap();
        fs::write(
            dir.join("settings/packs.json"),
            r#"[{"id":"ads","enabled":true,"action":"block","services":[]},{"id":"china","enabled":true,"action":"direct","services":[]}]"#,
        )
        .unwrap();
        let loaded = load_packs_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert!(loaded.iter().all(|pack| pack.id != "ads"));
        assert!(loaded.iter().any(|pack| pack.id == "china" && pack.enabled));
        assert_eq!(loaded.len(), crate::routing::catalog::PACK_ORDER.len());
    }

    #[test]
    fn existing_china_baidu_override_is_dropped() {
        let dir = std::env::temp_dir().join(format!("xraybox-packs-baidu-{}", std::process::id()));
        fs::create_dir_all(dir.join("settings")).unwrap();
        fs::write(
            dir.join("settings/packs.json"),
            r#"[{"id":"china","enabled":true,"action":"direct","services":[{"id":"baidu","action":"proxy"}]}]"#,
        )
        .unwrap();
        let loaded = load_packs_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        let china = loaded.iter().find(|pack| pack.id == "china").unwrap();
        assert!(china.services.is_empty());
    }

    #[test]
    fn missing_file_defaults_bypass_lan_on() {
        let dir =
            std::env::temp_dir().join(format!("xraybox-route-missing-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let loaded = load_route_settings_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert!(loaded.bypass_lan);
    }
}
