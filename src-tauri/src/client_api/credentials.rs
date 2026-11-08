use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::engine::error::EngineError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientCredentials {
    pub api_url: String,
    pub token: String,
    pub socket_url: String,
}

pub fn is_usable(api: &ClientCredentials) -> bool {
    !api.api_url.trim().is_empty() && !api.token.trim().is_empty()
}

pub fn load(path: &Path) -> Result<Option<ClientCredentials>, EngineError> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    match serde_json::from_str::<ClientCredentials>(&raw) {
        Ok(api) if is_usable(&api) => Ok(Some(api)),
        _ => Ok(None),
    }
}

pub fn save(path: &Path, api: &ClientCredentials) -> Result<(), EngineError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(api)?)?;
    Ok(())
}

pub fn clear(path: &Path) -> Result<(), EngineError> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!(
                "xraybox-client-api-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .join("settings")
            .join("client.json")
    }

    fn sample() -> ClientCredentials {
        ClientCredentials {
            api_url: "https://a1.example/api".into(),
            token: "tok".into(),
            socket_url: "wss://a1.example/api/ws".into(),
        }
    }

    #[test]
    fn load_save_and_clear_round_trip() {
        let path = temp_path();
        assert_eq!(load(&path).unwrap(), None);

        save(&path, &sample()).unwrap();
        assert_eq!(load(&path).unwrap(), Some(sample()));

        clear(&path).unwrap();
        assert_eq!(load(&path).unwrap(), None);
        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn unusable_credentials_are_treated_as_absent() {
        let path = temp_path();
        save(
            &path,
            &ClientCredentials {
                api_url: "https://a1.example/api".into(),
                token: String::new(),
                socket_url: "wss://a1.example/api/ws".into(),
            },
        )
        .unwrap();
        assert_eq!(load(&path).unwrap(), None);
        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
}
