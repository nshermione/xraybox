use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::client_api::{self, ClientCredentials};
use crate::engine::error::EngineError;
use crate::profile::subscription;

const SETTINGS_RELATIVE: &str = "settings/community.json";
pub const COMMUNITY_EVENT: &str = "community-event";

pub type CommunityApi = ClientCredentials;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityUsage {
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub expiry_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityVerify {
    pub ok: bool,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
struct SubscriptionJson {
    #[serde(default)]
    links: Vec<String>,
    api: Option<CommunityApi>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SubscriptionDocument {
    pub body: String,
    pub api: Option<CommunityApi>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CommunityEvent {
    r#type: String,
    version: Option<String>,
}

pub fn bind_client_api() {
    client_api::socket::configure(client_api::socket::SocketHooks {
        load: load_from_app,
        refresh: refresh_credentials,
        on_message: on_socket_message,
    });
}

pub fn parse_subscription_document(text: &str) -> SubscriptionDocument {
    match serde_json::from_str::<SubscriptionJson>(text) {
        Ok(json) if !json.links.is_empty() || json.api.is_some() => SubscriptionDocument {
            body: json.links.join("\n"),
            api: json.api.filter(client_api::credentials::is_usable),
        },
        _ => SubscriptionDocument {
            body: text.to_string(),
            api: None,
        },
    }
}

pub fn load_api(app_data_dir: &Path) -> Result<Option<CommunityApi>, EngineError> {
    client_api::credentials::load(&app_data_dir.join(SETTINGS_RELATIVE))
}

pub fn clear_api(app_data_dir: &Path) -> Result<(), EngineError> {
    client_api::credentials::clear(&app_data_dir.join(SETTINGS_RELATIVE))
}

pub fn save_api(app_data_dir: &Path, api: &CommunityApi) -> Result<(), EngineError> {
    client_api::credentials::save(&app_data_dir.join(SETTINGS_RELATIVE), api)
}

pub fn persist_fetched_api(app: &AppHandle, api: Option<&CommunityApi>) -> Result<(), EngineError> {
    persist_fetched_api_to(&app_data_dir(app)?, api)?;
    if api.is_some_and(client_api::credentials::is_usable) {
        client_api::socket::restart(app);
    } else {
        client_api::socket::stop();
    }
    Ok(())
}

pub fn persist_fetched_api_to(
    app_data_dir: &Path,
    api: Option<&CommunityApi>,
) -> Result<(), EngineError> {
    match api.filter(|item| client_api::credentials::is_usable(item)) {
        Some(api) => save_api(app_data_dir, api),
        None => clear_api(app_data_dir),
    }
}

pub fn refresh_credentials(app: &AppHandle) -> Result<CommunityApi, EngineError> {
    let settings = subscription::load_subscription(app)?;
    if settings.url.is_empty() {
        return Err(EngineError::Config("no subscription url saved".into()));
    }
    let document = subscription::fetch_subscription_document(&settings.url)?;
    let dir = app_data_dir(app)?;
    subscription::apply_fetched_body(&dir, &document.body, subscription::now_unix())?;
    match document.api.filter(client_api::credentials::is_usable) {
        Some(api) => {
            save_api(&dir, &api)?;
            Ok(api)
        }
        None => {
            clear_api(&dir)?;
            Err(EngineError::Config(
                "subscription did not return api credentials".into(),
            ))
        }
    }
}

pub fn verify(app: &AppHandle) -> Result<Option<CommunityVerify>, EngineError> {
    request_json(app, "verify")
}

pub fn usage(app: &AppHandle) -> Result<Option<CommunityUsage>, EngineError> {
    request_json(app, "usage")
}

fn request_json<T: serde::de::DeserializeOwned>(
    app: &AppHandle,
    path: &str,
) -> Result<Option<T>, EngineError> {
    let dir = app_data_dir(app)?;
    client_api::refresh::with_token_refresh(
        || load_api(&dir),
        || refresh_credentials(app),
        |api| client_api::http::get_json(api, path),
        || client_api::socket::restart(app),
        request_error,
    )
}

fn request_error(error: client_api::ApiError) -> EngineError {
    match error {
        client_api::ApiError::Expired => EngineError::Config("token_expired".into()),
        client_api::ApiError::Unavailable(message) => EngineError::Config(message),
    }
}

fn load_from_app(app: &AppHandle) -> Option<CommunityApi> {
    load_api(&app_data_dir(app).ok()?).ok().flatten()
}

fn on_socket_message(app: &AppHandle, text: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let event_type = value
        .get("type")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    if event_type == "sub.update" {
        let _ = app.emit(
            COMMUNITY_EVENT,
            CommunityEvent {
                r#type: event_type.to_string(),
                version: value
                    .get("version")
                    .and_then(|item| item.as_str())
                    .map(ToOwned::to_owned),
            },
        );
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
    use std::fs;

    #[test]
    fn parses_json_subscription_and_keeps_plaintext() {
        let json = r#"{"links":["vless://a","vless://b"],"api":{"apiUrl":"https://a1.example/api","token":"tok","socketUrl":"wss://a1.example/api/ws"}}"#;
        let parsed = parse_subscription_document(json);
        assert_eq!(parsed.body, "vless://a\nvless://b");
        assert_eq!(
            parsed.api,
            Some(CommunityApi {
                api_url: "https://a1.example/api".into(),
                token: "tok".into(),
                socket_url: "wss://a1.example/api/ws".into(),
            })
        );
        assert!(parse_subscription_document("vless://plain").api.is_none());
    }

    #[test]
    fn json_without_api_keeps_links_and_clears_stored_credentials() {
        let parsed = parse_subscription_document(r#"{"links":["vless://a"]}"#);
        assert_eq!(parsed.body, "vless://a");
        assert_eq!(parsed.api, None);

        let dir = std::env::temp_dir().join(format!(
            "xraybox-community-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        save_api(
            &dir,
            &CommunityApi {
                api_url: "https://old.example/api".into(),
                token: "old".into(),
                socket_url: "wss://old.example/api/ws".into(),
            },
        )
        .unwrap();
        persist_fetched_api_to(&dir, None).unwrap();
        assert_eq!(load_api(&dir).unwrap(), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
