use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::error::EngineError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VlessReality {
    #[serde(default)]
    pub remark: String,
    pub server: String,
    pub server_port: u16,
    pub uuid: String,
    pub server_name: String,
    pub public_key: String,
    #[serde(default)]
    pub short_id: String,
    pub flow: Option<String>,
    #[serde(default = "default_fingerprint")]
    pub fingerprint: String,
    #[serde(default)]
    pub spider_x: Option<String>,
    #[serde(default)]
    pub mldsa65_verify: Option<String>,
}

fn default_fingerprint() -> String {
    "chrome".into()
}

impl VlessReality {
    pub fn validate(&self) -> Result<(), EngineError> {
        if self.server.trim().is_empty() {
            return Err(EngineError::Config("server is required".into()));
        }
        if self.server_port == 0 {
            return Err(EngineError::Config("port must be 1..=65535".into()));
        }
        if !is_parseable_uuid(&self.uuid) {
            return Err(EngineError::Config("uuid is invalid".into()));
        }
        if self.public_key.trim().is_empty() {
            return Err(EngineError::Config("public_key is required".into()));
        }
        if self.server_name.trim().is_empty() {
            return Err(EngineError::Config("server_name is required".into()));
        }
        if !is_valid_short_id(&self.short_id) {
            return Err(EngineError::Config(
                "short_id must be hex (0-9, a-f), even length, at most 16 digits".into(),
            ));
        }
        Ok(())
    }

    fn effective_fingerprint(&self) -> String {
        if self.fingerprint.is_empty() {
            default_fingerprint()
        } else {
            self.fingerprint.clone()
        }
    }

    pub fn to_outbound(&self, tag: &str) -> Value {
        let mut settings = serde_json::json!({
            "address": self.server,
            "port": self.server_port,
            "id": self.uuid,
            "encryption": "none",
        });
        if let Some(flow) = non_empty(self.flow.as_deref()) {
            settings["flow"] = Value::String(flow.to_string());
        }

        let mut reality = serde_json::json!({
            "serverName": self.server_name,
            "fingerprint": self.effective_fingerprint(),
            // password holds the share-link `pbk` / struct field `public_key` — same value.
            "password": self.public_key,
            "shortId": self.short_id,
        });
        if let Some(spider) = non_empty(self.spider_x.as_deref()) {
            reality["spiderX"] = Value::String(spider.to_string());
        }
        if let Some(verify) = non_empty(self.mldsa65_verify.as_deref()) {
            reality["mldsa65Verify"] = Value::String(verify.to_string());
        }

        serde_json::json!({
            "tag": tag,
            "protocol": "vless",
            "settings": settings,
            "streamSettings": {
                "method": "raw",
                "security": "reality",
                "realitySettings": reality
            }
        })
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn is_valid_short_id(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return true;
    }
    if value.len() > 16 || value.len() % 2 != 0 {
        return false;
    }
    value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_parseable_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        match index {
            8 | 13 | 18 | 23 => {
                if *byte != b'-' {
                    return false;
                }
            }
            _ => {
                if !byte.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_profile() -> VlessReality {
        VlessReality {
            remark: "edge".into(),
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
    fn vless_reality_outbound_json() {
        let profile = VlessReality {
            remark: String::new(),
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
        };
        let outbound = profile.to_outbound("proxy");
        assert_eq!(outbound["protocol"], "vless");
        assert_eq!(outbound["tag"], "proxy");
        assert_eq!(outbound["streamSettings"]["security"], "reality");
        assert_eq!(
            outbound["streamSettings"]["realitySettings"]["password"],
            "abc"
        );
        assert_eq!(outbound["settings"]["flow"], "xtls-rprx-vision");
    }

    #[test]
    fn omits_flow_when_none() {
        let mut profile = sample_profile();
        profile.flow = None;
        let outbound = profile.to_outbound("proxy");
        assert!(outbound["settings"].get("flow").is_none());
    }

    #[test]
    fn rejects_empty_uuid() {
        let mut profile = sample_profile();
        profile.uuid.clear();
        assert!(profile.validate().is_err());
    }

    #[test]
    fn accepts_hex_short_id() {
        let mut profile = sample_profile();
        profile.short_id = "dead".into();
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn accepts_empty_short_id() {
        let mut profile = sample_profile();
        profile.short_id.clear();
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn rejects_non_hex_short_id() {
        let mut profile = sample_profile();
        profile.short_id = "test".into();
        let err = profile.validate().expect_err("non-hex short_id");
        assert!(matches!(err, EngineError::Config(_)));
        assert!(!matches!(err, EngineError::NeedAdmin));

        profile.short_id = "not-hex".into();
        let err = profile.validate().expect_err("non-hex short_id");
        assert!(matches!(err, EngineError::Config(_)));
        assert!(!matches!(err, EngineError::NeedAdmin));
    }

    #[test]
    fn xray_outbound_uses_flat_settings_and_raw_method() {
        let profile = sample_profile();
        let outbound = profile.to_outbound("proxy");

        assert_eq!(outbound["tag"], "proxy");
        assert_eq!(outbound["protocol"], "vless");

        // Flat settings: no vnext, no users array.
        assert_eq!(outbound["settings"]["address"], "edge.example.com");
        assert_eq!(outbound["settings"]["port"], 443);
        assert_eq!(outbound["settings"]["id"], profile.uuid);
        assert_eq!(outbound["settings"]["encryption"], "none");
        assert!(outbound["settings"].get("vnext").is_none());

        let stream = &outbound["streamSettings"];
        assert_eq!(stream["method"], "raw");
        assert_eq!(stream["security"], "reality");
        assert!(stream.get("network").is_none());

        let reality = &stream["realitySettings"];
        assert_eq!(reality["serverName"], "www.cloudflare.com");
        assert_eq!(reality["fingerprint"], "chrome");
        assert_eq!(reality["password"], "abc");
        assert_eq!(reality["shortId"], "dead");
        assert!(reality.get("publicKey").is_none());
        assert!(reality.get("target").is_none());
    }

    #[test]
    fn accepts_short_id_shorter_than_16_hex_digits() {
        let mut profile = sample_profile();
        profile.short_id = "aa1234".into(); // core pads to aa12340000000000
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn xray_outbound_carries_spider_x_and_pq_verify() {
        let mut profile = sample_profile();
        profile.spider_x = Some("/26306e280ce9053".into());
        profile.mldsa65_verify = Some("BASE64KEY".into());
        let outbound = profile.to_outbound("proxy");
        let reality = &outbound["streamSettings"]["realitySettings"];
        assert_eq!(reality["spiderX"], "/26306e280ce9053");
        assert_eq!(reality["mldsa65Verify"], "BASE64KEY");
    }

    #[test]
    fn xray_outbound_omits_empty_optionals() {
        let mut profile = sample_profile();
        profile.spider_x = None;
        profile.mldsa65_verify = None;
        profile.flow = None;
        let outbound = profile.to_outbound("proxy");
        let reality = &outbound["streamSettings"]["realitySettings"];
        assert!(reality.get("spiderX").is_none());
        assert!(reality.get("mldsa65Verify").is_none());
        assert!(outbound["settings"].get("flow").is_none());
    }

    #[test]
    fn deserializes_without_spider_x_and_pq_verify() {
        let json = r#"{
            "remark": "edge",
            "server": "edge.example.com",
            "serverPort": 443,
            "uuid": "11111111-1111-1111-1111-111111111111",
            "serverName": "www.cloudflare.com",
            "publicKey": "abc",
            "shortId": "dead",
            "flow": "xtls-rprx-vision",
            "fingerprint": "chrome"
        }"#;
        let profile: VlessReality = serde_json::from_str(json).unwrap();
        assert_eq!(profile.spider_x, None);
        assert_eq!(profile.mldsa65_verify, None);
    }
}
