use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::error::EngineError;

pub const GEO_FILES: [&str; 2] = ["geoip.dat", "geosite.dat"];

pub const GEOSITE_DAT: &str =
    "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geosite.dat";
pub const GEOIP_DAT: &str =
    "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geoip.dat";

/// Cache (updated `.dat`) wins; otherwise copy a bundled seed in. Never hits the network.
pub fn ensure_geo_assets(
    cache_dir: &Path,
    bundle_dirs: &[PathBuf],
) -> Result<PathBuf, EngineError> {
    fs::create_dir_all(cache_dir)?;
    for name in GEO_FILES {
        let target = cache_dir.join(name);
        if is_usable_geo_file(&target) {
            continue;
        }
        let seed = bundle_dirs
            .iter()
            .map(|dir| dir.join(name))
            .find(|path| is_usable_geo_file(path))
            .ok_or_else(|| {
                EngineError::Config(format!(
                    "missing {name}: no app-data copy and no bundled seed. Run scripts/fetch-xray.ps1 or scripts/fetch-xray.sh"
                ))
            })?;
        fs::copy(&seed, &target)?;
    }
    Ok(cache_dir.to_path_buf())
}

pub fn is_usable_geo_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.len() > 0)
        .unwrap_or(false)
}

fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|err| format!("{url}: {err}"))?;
    if response.status() != 200 {
        return Err(format!("{url}: HTTP {}", response.status()));
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|err| format!("{url}: {err}"))?;
    Ok(bytes)
}

/// Background updater only. Writes into the app-data cache; Connect never calls this.
pub fn update_geo_assets(cache_dir: &Path) -> Result<PathBuf, EngineError> {
    update_geo_assets_with(cache_dir, download_bytes)
}

pub fn update_geo_assets_with<F>(cache_dir: &Path, download: F) -> Result<PathBuf, EngineError>
where
    F: Fn(&str) -> Result<Vec<u8>, String>,
{
    fs::create_dir_all(cache_dir)?;
    for (name, url) in [("geoip.dat", GEOIP_DAT), ("geosite.dat", GEOSITE_DAT)] {
        let bytes = download(url)
            .map_err(|err| EngineError::Config(format!("download {name} failed: {err}")))?;
        if bytes.is_empty() {
            return Err(EngineError::Config(format!(
                "download {name} returned empty file from {url}"
            )));
        }
        write_geo_file_atomic(&cache_dir.join(name), name, &bytes)?;
    }
    Ok(cache_dir.to_path_buf())
}

fn write_geo_file_atomic(path: &Path, name: &str, bytes: &[u8]) -> Result<(), EngineError> {
    let parent = path.parent().unwrap_or(path);
    let tmp = parent.join(format!("{name}.part"));
    fs::write(&tmp, bytes)?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_pair(label: &str) -> (PathBuf, PathBuf) {
        let stamp = format!(
            "{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let bundle = std::env::temp_dir().join(format!("xraybox-geo-bundle-{stamp}"));
        let cache = std::env::temp_dir().join(format!("xraybox-geo-cache-{stamp}"));
        fs::create_dir_all(&bundle).unwrap();
        fs::create_dir_all(&cache).unwrap();
        (bundle, cache)
    }

    #[test]
    fn geo_asset_names_are_exact() {
        assert_eq!(GEO_FILES, ["geoip.dat", "geosite.dat"]);
    }

    #[test]
    fn seeds_missing_geo_files_into_cache() {
        let (bundle, cache) = temp_pair("seed");
        fs::write(bundle.join("geoip.dat"), b"x").unwrap();
        fs::write(bundle.join("geosite.dat"), b"y").unwrap();
        let dir = ensure_geo_assets(&cache, &[bundle.clone()]).unwrap();
        assert_eq!(dir, cache);
        assert!(cache.join("geoip.dat").is_file());
        assert!(cache.join("geosite.dat").is_file());
        let _ = fs::remove_dir_all(&bundle);
        let _ = fs::remove_dir_all(&cache);
    }

    #[test]
    fn cache_copy_wins_over_bundled_seed() {
        let (bundle, cache) = temp_pair("cache-wins");
        fs::write(bundle.join("geoip.dat"), b"old").unwrap();
        fs::write(bundle.join("geosite.dat"), b"old").unwrap();
        fs::write(cache.join("geoip.dat"), b"new").unwrap();
        fs::write(cache.join("geosite.dat"), b"new").unwrap();
        ensure_geo_assets(&cache, &[bundle.clone()]).unwrap();
        assert_eq!(fs::read(cache.join("geoip.dat")).unwrap(), b"new");
        let _ = fs::remove_dir_all(&bundle);
        let _ = fs::remove_dir_all(&cache);
    }

    #[test]
    fn missing_everywhere_is_an_error_naming_the_script() {
        let (bundle, cache) = temp_pair("missing");
        let err = ensure_geo_assets(&cache, &[]).unwrap_err();
        assert!(format!("{err}").contains("fetch-xray.ps1"));
        let _ = fs::remove_dir_all(&bundle);
        let _ = fs::remove_dir_all(&cache);
    }

    #[test]
    fn update_writes_both_files_atomically() {
        let cache = std::env::temp_dir().join(format!(
            "xraybox-geo-update-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&cache).unwrap();
        update_geo_assets_with(&cache, |url| {
            if url.contains("geoip.dat") {
                Ok(b"ip-bytes".to_vec())
            } else {
                Ok(b"site-bytes".to_vec())
            }
        })
        .unwrap();
        assert_eq!(fs::read(cache.join("geoip.dat")).unwrap(), b"ip-bytes");
        assert_eq!(fs::read(cache.join("geosite.dat")).unwrap(), b"site-bytes");
        let _ = fs::remove_dir_all(&cache);
    }
}
