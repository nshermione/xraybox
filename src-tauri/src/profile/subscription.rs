use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::country::infer_country_code;
use super::share::{parse_share_link, profile_id, ShareError};
use super::VlessReality;
use crate::community::{parse_subscription_document, SubscriptionDocument};
use crate::engine::error::EngineError;

const SETTINGS_RELATIVE: &str = "settings/subscription.json";
const NODES_RELATIVE: &str = "profiles/nodes.json";
const SUBSCRIPTION_USER_AGENT: &str = "v2rayN/xraybox";
const FETCH_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FetchInterval {
    #[serde(rename = "3h")]
    ThreeHours,
    #[serde(rename = "1d")]
    #[default]
    OneDay,
    #[serde(rename = "7d")]
    SevenDays,
    #[serde(rename = "off")]
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SubscriptionSettings {
    pub url: String,
    pub fetch_interval: FetchInterval,
    pub last_fetched_at: Option<i64>,
    pub selected_profile_id: Option<String>,
    pub skipped_count: u32,
}

impl Default for SubscriptionSettings {
    fn default() -> Self {
        Self {
            url: String::new(),
            fetch_interval: FetchInterval::OneDay,
            last_fetched_at: None,
            selected_profile_id: None,
            skipped_count: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileNode {
    pub id: String,
    pub profile: VlessReality,
    pub country_code: String,
    #[serde(default)]
    pub ping_ms: Vec<Option<u32>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FetchOutcome {
    pub imported_count: u32,
    pub skipped_count: u32,
    pub nodes: Vec<ProfileNode>,
}

pub fn validate_subscription_url(url: &str) -> Result<String, EngineError> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(EngineError::Config(
            "subscription url must start with http:// or https://".into(),
        ));
    }
    Ok(trimmed.to_string())
}

pub fn parse_subscription_body(body: &str) -> FetchOutcome {
    let text = decode_subscription_text(body);
    let mut nodes = Vec::new();
    let mut skipped = 0u32;
    let mut seen = std::collections::HashSet::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_share_link(line) {
            Ok(profile) => {
                let id = profile_id(&profile);
                if !seen.insert(id.clone()) {
                    continue;
                }
                let country_code = infer_country_code(&profile.remark);
                nodes.push(ProfileNode {
                    id,
                    profile,
                    country_code,
                    ping_ms: Vec::new(),
                });
            }
            Err(ShareError::UnsupportedProtocol | ShareError::Invalid) => skipped += 1,
        }
    }

    FetchOutcome {
        imported_count: nodes.len() as u32,
        skipped_count: skipped,
        nodes,
    }
}

pub fn merge_fetched_nodes(previous: &[ProfileNode], fetched: FetchOutcome) -> FetchOutcome {
    let mut nodes = fetched.nodes;
    for node in &mut nodes {
        if let Some(old) = previous.iter().find(|old| old.id == node.id) {
            node.ping_ms = old.ping_ms.clone();
        }
    }
    FetchOutcome {
        imported_count: nodes.len() as u32,
        skipped_count: fetched.skipped_count,
        nodes,
    }
}

pub fn retain_selected_id(selected: Option<String>, nodes: &[ProfileNode]) -> Option<String> {
    selected.filter(|id| nodes.iter().any(|node| node.id == *id))
}

pub fn fetch_is_due(interval: FetchInterval, last_fetched_at: Option<i64>, now: i64) -> bool {
    let Some(last) = last_fetched_at else {
        return interval != FetchInterval::Off;
    };
    let elapsed = now.saturating_sub(last);
    match interval {
        FetchInterval::Off => false,
        FetchInterval::ThreeHours => elapsed >= 3 * 3600,
        FetchInterval::OneDay => elapsed >= 24 * 3600,
        FetchInterval::SevenDays => elapsed >= 7 * 24 * 3600,
    }
}

pub fn latest_latency(samples: &[Option<u32>]) -> Option<u32> {
    samples.iter().rev().find_map(|sample| *sample)
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

pub fn load_subscription_from(app_data_dir: &Path) -> Result<SubscriptionSettings, EngineError> {
    let path = app_data_dir.join(SETTINGS_RELATIVE);
    if !path.is_file() {
        return Ok(SubscriptionSettings::default());
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

pub fn save_subscription_to(
    app_data_dir: &Path,
    settings: &SubscriptionSettings,
) -> Result<(), EngineError> {
    write_json(&app_data_dir.join(SETTINGS_RELATIVE), settings)
}

pub fn load_nodes_from(app_data_dir: &Path) -> Result<Vec<ProfileNode>, EngineError> {
    let path = app_data_dir.join(NODES_RELATIVE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

pub fn save_nodes_to(app_data_dir: &Path, nodes: &[ProfileNode]) -> Result<(), EngineError> {
    write_json(&app_data_dir.join(NODES_RELATIVE), nodes)
}

pub fn load_subscription(app: &AppHandle) -> Result<SubscriptionSettings, EngineError> {
    load_subscription_from(&app_data_dir(app)?)
}

pub fn save_subscription(
    app: &AppHandle,
    settings: &SubscriptionSettings,
) -> Result<(), EngineError> {
    save_subscription_to(&app_data_dir(app)?, settings)
}

pub fn load_nodes(app: &AppHandle) -> Result<Vec<ProfileNode>, EngineError> {
    load_nodes_from(&app_data_dir(app)?)
}

pub fn save_nodes(app: &AppHandle, nodes: &[ProfileNode]) -> Result<(), EngineError> {
    save_nodes_to(&app_data_dir(app)?, nodes)
}

pub fn fetch_subscription_text(url: &str) -> Result<String, EngineError> {
    Ok(fetch_subscription_document(url)?.body)
}

pub fn fetch_subscription_document(url: &str) -> Result<SubscriptionDocument, EngineError> {
    let url = validate_subscription_url(url)?;
    let response = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .get(&url)
        .set("User-Agent", SUBSCRIPTION_USER_AGENT)
        .set("Accept", "application/json, text/plain;q=0.9, */*;q=0.8")
        .call()
        .map_err(|err| EngineError::Config(format!("subscription fetch failed: {err}")))?;
    let text = response
        .into_string()
        .map_err(|err| EngineError::Config(format!("subscription body read failed: {err}")))?;
    Ok(parse_subscription_document(&text))
}

pub fn apply_fetched_body(
    app_data_dir: &Path,
    body: &str,
    now: i64,
) -> Result<(SubscriptionSettings, Vec<ProfileNode>), EngineError> {
    let mut settings = load_subscription_from(app_data_dir)?;
    let previous = load_nodes_from(app_data_dir)?;
    let fetched = merge_fetched_nodes(&previous, parse_subscription_body(body));
    settings.skipped_count = fetched.skipped_count;
    settings.last_fetched_at = Some(now);
    settings.selected_profile_id = retain_selected_id(settings.selected_profile_id, &fetched.nodes)
        .or_else(|| fetched.nodes.first().map(|node| node.id.clone()));
    save_nodes_to(app_data_dir, &fetched.nodes)?;
    save_subscription_to(app_data_dir, &settings)?;
    Ok((settings, fetched.nodes))
}

pub fn clear_profiles_from(
    app_data_dir: &Path,
) -> Result<(SubscriptionSettings, Vec<ProfileNode>), EngineError> {
    let mut settings = load_subscription_from(app_data_dir)?;
    settings.selected_profile_id = None;
    settings.skipped_count = 0;
    save_nodes_to(app_data_dir, &[])?;
    save_subscription_to(app_data_dir, &settings)?;
    Ok((settings, Vec::new()))
}

pub fn save_url_to(app_data_dir: &Path, url: &str) -> Result<SubscriptionSettings, EngineError> {
    let url = validate_subscription_url(url)?;
    let mut settings = load_subscription_from(app_data_dir)?;
    settings.url = url;
    save_subscription_to(app_data_dir, &settings)?;
    Ok(settings)
}

fn write_json<T: Serialize + ?Sized>(path: &PathBuf, value: &T) -> Result<(), EngineError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, EngineError> {
    app.path()
        .app_data_dir()
        .map_err(|err| EngineError::Config(err.to_string()))
}

fn decode_subscription_text(body: &str) -> String {
    let trimmed = body.trim();
    if looks_like_share_list(trimmed) {
        return trimmed.to_string();
    }
    if let Some(decoded) = decode_base64(trimmed) {
        if looks_like_share_list(&decoded) || decoded.contains("://") {
            return decoded;
        }
    }
    trimmed.to_string()
}

fn looks_like_share_list(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        line.contains("://")
    })
}

fn decode_base64(input: &str) -> Option<String> {
    let compact: String = input.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.is_empty() {
        return None;
    }
    let mut bytes = Vec::new();
    let chars: Vec<u8> = compact.bytes().collect();
    let mut index = 0;
    while index < chars.len() {
        let c0 = chars.get(index).copied().unwrap_or(b'=');
        let c1 = chars.get(index + 1).copied().unwrap_or(b'=');
        let c2 = chars.get(index + 2).copied().unwrap_or(b'=');
        let c3 = chars.get(index + 3).copied().unwrap_or(b'=');
        let v0 = b64_value(c0)?;
        let v1 = b64_value(c1)?;
        bytes.push((v0 << 2) | (v1 >> 4));
        if c2 != b'=' {
            let v2 = b64_value(c2)?;
            bytes.push(((v1 & 0x0f) << 4) | (v2 >> 2));
            if c3 != b'=' {
                let v3 = b64_value(c3)?;
                bytes.push(((v2 & 0x03) << 6) | v3);
            }
        }
        index += 4;
    }
    String::from_utf8(bytes).ok()
}

fn b64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        b'=' => Some(0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vless_line() -> &'static str {
        "vless://11111111-1111-1111-1111-111111111111@edge.example.com:443?security=reality&pbk=abc&sni=www.cloudflare.com&sid=dead#Japan%20Tokyo"
    }

    fn second_vless_line() -> &'static str {
        "vless://22222222-2222-2222-2222-222222222222@hk.example.com:443?security=reality&pbk=abc&sni=www.cloudflare.com#Hong%20Kong"
    }

    #[test]
    fn parses_plaintext_vless_and_skips_vmess() {
        let body = format!(
            "{}\nvmess://eyJ2IjoiMiJ9\n{}",
            vless_line(),
            second_vless_line()
        );
        let parsed = parse_subscription_body(&body);
        assert_eq!(parsed.imported_count, 2);
        assert_eq!(parsed.skipped_count, 1);
        assert_eq!(parsed.nodes[0].country_code, "jp");
        assert_eq!(parsed.nodes[1].country_code, "hk");
    }

    #[test]
    fn parses_base64_body() {
        let encoded = encode_base64(&format!("{}\n{}", vless_line(), second_vless_line()));
        let parsed = parse_subscription_body(&encoded);
        assert_eq!(parsed.imported_count, 2);
        assert_eq!(parsed.skipped_count, 0);
    }

    #[test]
    fn re_fetch_keeps_ping_and_selected_when_id_survives() {
        let dir = temp_dir("keep-ping");
        let first = parse_subscription_body(&format!("{}\n{}", vless_line(), second_vless_line()));
        let mut nodes = first.nodes.clone();
        nodes[0].ping_ms = vec![Some(40), Some(42)];
        save_nodes_to(&dir, &nodes).unwrap();
        save_subscription_to(
            &dir,
            &SubscriptionSettings {
                url: "https://example.com/sub".into(),
                selected_profile_id: Some(nodes[0].id.clone()),
                ..SubscriptionSettings::default()
            },
        )
        .unwrap();

        let (settings, next) = apply_fetched_body(
            &dir,
            &format!("{}\nvmess://x\n{}", vless_line(), second_vless_line()),
            1_700_000_000,
        )
        .unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(next[0].ping_ms, vec![Some(40), Some(42)]);
        assert_eq!(
            settings.selected_profile_id.as_deref(),
            Some(nodes[0].id.as_str())
        );
        assert_eq!(settings.skipped_count, 1);
        assert_eq!(settings.last_fetched_at, Some(1_700_000_000));
    }

    #[test]
    fn drops_selected_when_node_disappears() {
        let previous =
            parse_subscription_body(&format!("{}\n{}", vless_line(), second_vless_line())).nodes;
        let fetched = parse_subscription_body(second_vless_line());
        let merged = merge_fetched_nodes(&previous, fetched);
        let selected = retain_selected_id(Some(previous[0].id.clone()), &merged.nodes);
        assert_eq!(selected, None);
    }

    #[test]
    fn interval_due_matches_thresholds() {
        assert!(!fetch_is_due(FetchInterval::Off, None, 100));
        assert!(fetch_is_due(FetchInterval::OneDay, None, 100));
        assert!(!fetch_is_due(FetchInterval::OneDay, Some(100), 100 + 3600));
        assert!(fetch_is_due(
            FetchInterval::OneDay,
            Some(100),
            100 + 24 * 3600
        ));
        assert!(fetch_is_due(
            FetchInterval::ThreeHours,
            Some(100),
            100 + 3 * 3600
        ));
        assert!(!fetch_is_due(
            FetchInterval::SevenDays,
            Some(100),
            100 + 24 * 3600
        ));
    }

    #[test]
    fn rejects_non_http_url() {
        assert!(validate_subscription_url("vless://x").is_err());
        assert!(validate_subscription_url("https://panel.example/sub").is_ok());
    }

    #[test]
    fn clear_profiles_keeps_url_and_interval() {
        let dir = temp_dir("clear-profiles");
        let parsed = parse_subscription_body(vless_line());
        save_nodes_to(&dir, &parsed.nodes).unwrap();
        save_subscription_to(
            &dir,
            &SubscriptionSettings {
                url: "https://example.com/sub".into(),
                fetch_interval: FetchInterval::SevenDays,
                last_fetched_at: Some(99),
                selected_profile_id: Some(parsed.nodes[0].id.clone()),
                skipped_count: 3,
            },
        )
        .unwrap();

        let (settings, nodes) = clear_profiles_from(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert!(nodes.is_empty());
        assert_eq!(settings.url, "https://example.com/sub");
        assert_eq!(settings.fetch_interval, FetchInterval::SevenDays);
        assert_eq!(settings.last_fetched_at, Some(99));
        assert_eq!(settings.selected_profile_id, None);
        assert_eq!(settings.skipped_count, 0);
    }

    fn encode_base64(input: &str) -> String {
        const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes = input.as_bytes();
        let mut out = String::new();
        let mut index = 0;
        while index < bytes.len() {
            let b0 = bytes[index];
            let b1 = bytes.get(index + 1).copied();
            let b2 = bytes.get(index + 2).copied();
            out.push(TABLE[(b0 >> 2) as usize] as char);
            out.push(TABLE[(((b0 & 0x03) << 4) | (b1.unwrap_or(0) >> 4)) as usize] as char);
            if b1.is_some() {
                out.push(
                    TABLE[(((b1.unwrap_or(0) & 0x0f) << 2) | (b2.unwrap_or(0) >> 6)) as usize]
                        as char,
                );
            } else {
                out.push('=');
            }
            if b2.is_some() {
                out.push(TABLE[(b2.unwrap_or(0) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
            index += 3;
        }
        out
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xraybox-sub-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
