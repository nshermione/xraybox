use serde::{Deserialize, Serialize};

use super::action::RuleAction;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceOverride {
    pub id: String,
    pub action: RuleAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackState {
    pub id: String,
    #[serde(default)]
    pub enabled: bool,
    pub action: RuleAction,
    #[serde(default)]
    pub services: Vec<ServiceOverride>,
}

#[derive(Debug, Clone, Copy)]
pub struct ServiceDef {
    pub id: &'static str,
    pub tag: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct PackDef {
    pub id: &'static str,
    pub umbrella_tags: &'static [&'static str],
    pub services: &'static [ServiceDef],
    pub default_enabled: bool,
    pub default_action: RuleAction,
}

pub const PACK_ORDER: [&str; 13] = [
    "google",
    "apple",
    "meta",
    "tiktok",
    "x",
    "microsoft",
    "amazon",
    "netflix",
    "telegram",
    "discord",
    "openai",
    "github",
    "china",
];

/// Packs removed from the catalog. Saved state for them is discarded on load
/// instead of failing validation (`ads`: 192k domains exceed the iOS
/// extension memory limit).
pub const RETIRED_PACK_IDS: [&str; 1] = ["ads"];

pub const CN_CDN_TAGS: [&str; 2] = ["geosite:apple-cn", "geosite:google-cn"];
pub const OVERSEAS_TAG: &str = "geosite:geolocation-!cn";

pub const CATALOG: &[PackDef] = &[
    PackDef {
        id: "google",
        umbrella_tags: &["geosite:google"],
        services: &[
            ServiceDef {
                id: "youtube",
                tag: "geosite:youtube",
            },
            ServiceDef {
                id: "google-play",
                tag: "geosite:google-play",
            },
        ],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "apple",
        umbrella_tags: &["geosite:apple"],
        services: &[ServiceDef {
            id: "icloud",
            tag: "geosite:icloud",
        }],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "meta",
        umbrella_tags: &["geosite:facebook", "geosite:instagram", "geosite:whatsapp"],
        services: &[
            ServiceDef {
                id: "facebook",
                tag: "geosite:facebook",
            },
            ServiceDef {
                id: "instagram",
                tag: "geosite:instagram",
            },
            ServiceDef {
                id: "whatsapp",
                tag: "geosite:whatsapp",
            },
            ServiceDef {
                id: "messenger",
                tag: "geosite:messenger",
            },
        ],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "tiktok",
        umbrella_tags: &["geosite:tiktok", "geosite:bytedance"],
        services: &[
            ServiceDef {
                id: "tiktok",
                tag: "geosite:tiktok",
            },
            ServiceDef {
                id: "bytedance",
                tag: "geosite:bytedance",
            },
        ],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "x",
        umbrella_tags: &["geosite:twitter"],
        services: &[],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "microsoft",
        umbrella_tags: &["geosite:microsoft"],
        services: &[ServiceDef {
            id: "onedrive",
            tag: "geosite:onedrive",
        }],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "amazon",
        umbrella_tags: &["geosite:amazon"],
        services: &[],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "netflix",
        umbrella_tags: &["geosite:netflix", "geoip:netflix"],
        services: &[],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "telegram",
        umbrella_tags: &["geosite:telegram", "geoip:telegram"],
        services: &[],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "discord",
        umbrella_tags: &["geosite:discord"],
        services: &[],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "openai",
        umbrella_tags: &["geosite:openai"],
        services: &[],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "github",
        umbrella_tags: &["geosite:github"],
        services: &[],
        default_enabled: false,
        default_action: RuleAction::Proxy,
    },
    PackDef {
        id: "china",
        umbrella_tags: &["geosite:cn", "geoip:cn"],
        services: &[
            ServiceDef {
                id: "google-cn",
                tag: "geosite:google-cn",
            },
            ServiceDef {
                id: "apple-cn",
                tag: "geosite:apple-cn",
            },
        ],
        default_enabled: true,
        default_action: RuleAction::Direct,
    },
];

pub fn pack_def(id: &str) -> Option<&'static PackDef> {
    CATALOG.iter().find(|pack| pack.id == id)
}

pub fn catalog_defaults() -> Vec<PackState> {
    CATALOG
        .iter()
        .map(|pack| PackState {
            id: pack.id.into(),
            enabled: pack.default_enabled,
            action: pack.default_action,
            services: Vec::new(),
        })
        .collect()
}

pub fn validate_packs(packs: &[PackState]) -> Result<(), crate::engine::error::EngineError> {
    for pack in packs {
        let def = pack_def(&pack.id).ok_or_else(|| {
            crate::engine::error::EngineError::Config(format!("unknown pack id: {}", pack.id))
        })?;
        for service in &pack.services {
            if !def.services.iter().any(|known| known.id == service.id) {
                return Err(crate::engine::error::EngineError::Config(format!(
                    "unknown service {} in pack {}",
                    service.id, pack.id
                )));
            }
        }
    }
    Ok(())
}

fn push_unique(tags: &mut Vec<String>, tag: &str) {
    if !tags.iter().any(|existing| existing == tag) {
        tags.push(tag.to_string());
    }
}

/// Umbrella tags referenced by **enabled** packs only. Disabled packs contribute nothing.
pub fn needed_umbrella_tags(packs: &[PackState]) -> Vec<String> {
    let mut tags = Vec::new();
    for pack in ordered_packs(packs) {
        if !pack.enabled {
            continue;
        }
        if let Some(def) = pack_def(&pack.id) {
            for tag in def.umbrella_tags {
                push_unique(&mut tags, tag);
            }
        }
    }
    tags
}

/// Files that must exist or Connect fails: China umbrellas when that pack is on.
pub fn required_umbrella_tags(packs: &[PackState]) -> Vec<String> {
    needed_umbrella_tags(packs)
        .into_iter()
        .filter(|tag| tag == "geosite:cn" || tag == "geoip:cn")
        .collect()
}

/// Enabled brand umbrellas. Missing files skip the pack; they must not fail Connect.
pub fn optional_umbrella_tags(packs: &[PackState]) -> Vec<String> {
    needed_umbrella_tags(packs)
        .into_iter()
        .filter(|tag| tag != "geosite:cn" && tag != "geoip:cn")
        .collect()
}

#[allow(dead_code)]
pub fn optional_service_tags(packs: &[PackState]) -> Vec<String> {
    let mut tags = Vec::new();
    for pack in ordered_packs(packs) {
        if !pack.enabled {
            continue;
        }
        let Some(def) = pack_def(&pack.id) else {
            continue;
        };
        for service in &pack.services {
            if let Some(known) = def.services.iter().find(|item| item.id == service.id) {
                tags.push(known.tag.to_string());
            }
        }
    }
    tags
}

pub fn ordered_packs(packs: &[PackState]) -> Vec<PackState> {
    let mut ordered = Vec::new();
    for id in PACK_ORDER {
        if let Some(pack) = packs.iter().find(|pack| pack.id == id) {
            ordered.push(pack.clone());
        }
    }
    ordered
}

pub fn service_tag(pack_id: &str, service_id: &str) -> Option<&'static str> {
    pack_def(pack_id)?
        .services
        .iter()
        .find(|service| service.id == service_id)
        .map(|service| service.tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_xray_selectors() {
        let china = pack_def("china").unwrap();
        assert_eq!(china.umbrella_tags, ["geosite:cn", "geoip:cn"]);
        assert_eq!(service_tag("google", "youtube"), Some("geosite:youtube"));
        assert_eq!(service_tag("china", "google-cn"), Some("geosite:google-cn"));
        assert_eq!(service_tag("china", "apple-cn"), Some("geosite:apple-cn"));
        assert!(service_tag("google", "google-cn").is_none());
        assert!(service_tag("apple", "apple-cn").is_none());
        assert!(service_tag("china", "baidu").is_none());
        assert_eq!(
            pack_def("telegram").unwrap().umbrella_tags,
            ["geosite:telegram", "geoip:telegram"]
        );
        assert_eq!(
            pack_def("netflix").unwrap().umbrella_tags,
            ["geosite:netflix", "geoip:netflix"]
        );
        assert!(pack_def("ads").is_none());
        for pack in CATALOG {
            for tag in pack.umbrella_tags {
                assert!(
                    tag.starts_with("geosite:") || tag.starts_with("geoip:"),
                    "{tag}"
                );
            }
        }
    }

    #[test]
    fn catalog_follows_pack_order() {
        let ids: Vec<_> = CATALOG.iter().map(|pack| pack.id).collect();
        assert_eq!(ids, PACK_ORDER);
    }

    #[test]
    fn defaults_needed_tags_are_cn_only() {
        let needed = needed_umbrella_tags(&catalog_defaults());
        assert_eq!(
            needed,
            vec!["geosite:cn".to_string(), "geoip:cn".to_string()]
        );
        assert_eq!(
            required_umbrella_tags(&catalog_defaults()),
            vec!["geosite:cn".to_string(), "geoip:cn".to_string()]
        );
        assert!(optional_umbrella_tags(&catalog_defaults()).is_empty());
        assert!(!needed.iter().any(|tag| tag == "geosite:google"));
    }

    #[test]
    fn google_off_is_not_needed() {
        let packs = catalog_defaults();
        assert!(packs
            .iter()
            .any(|pack| pack.id == "google" && !pack.enabled));
        assert!(!needed_umbrella_tags(&packs)
            .iter()
            .any(|tag| tag == "geosite:google"));
    }

    #[test]
    fn google_on_is_needed_but_not_hard_required() {
        let mut packs = catalog_defaults();
        packs
            .iter_mut()
            .find(|pack| pack.id == "google")
            .unwrap()
            .enabled = true;
        let needed = needed_umbrella_tags(&packs);
        assert!(needed.iter().any(|tag| tag == "geosite:google"));
        assert_eq!(
            required_umbrella_tags(&packs),
            vec!["geosite:cn".to_string(), "geoip:cn".to_string()]
        );
        let optional = optional_umbrella_tags(&packs);
        assert!(optional.iter().any(|tag| tag == "geosite:google"));
    }

    #[test]
    #[ignore = "sidecar integration; needs pinned geo files"]
    fn xray_test_accepts_every_catalog_category() {
        use std::path::PathBuf;
        use std::process::Command;

        use crate::dns::DnsSettings;
        use crate::engine::config::ConfigBuilder;
        use crate::engine::process::{resolve_sidecar_path, validate_config_args};
        use crate::profile::VlessReality;
        use crate::routing::brands::bind_route;
        use crate::routing::compile::RouteSettings;

        let sidecar = resolve_sidecar_path().expect("sidecar missing; run scripts/fetch-xray.ps1");
        let resources = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("assets");
        let binaries = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
        let asset_dir = if resources.join("geosite.dat").is_file() {
            resources
        } else {
            binaries
        };

        let mut packs = catalog_defaults();
        for pack in &mut packs {
            pack.enabled = true;
            if let Some(def) = pack_def(&pack.id) {
                pack.services = def
                    .services
                    .iter()
                    .map(|service| ServiceOverride {
                        id: service.id.into(),
                        action: pack.action,
                    })
                    .collect();
            }
        }

        let bound = bind_route(packs, &asset_dir, &[asset_dir.clone()]).unwrap();
        let mut route = RouteSettings::default();
        route.extra_rules = bound.extra_rules;

        let profile = VlessReality {
            remark: "test".into(),
            server: "edge.example.com".into(),
            server_port: 443,
            uuid: "11111111-1111-1111-1111-111111111111".into(),
            server_name: "www.cloudflare.com".into(),
            public_key: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            short_id: "dead".into(),
            flow: Some("xtls-rprx-vision".into()),
            fingerprint: "chrome".into(),
            spider_x: None,
            mldsa65_verify: None,
        };

        let mut value = ConfigBuilder::default()
            .with_proxy(profile)
            .with_tun()
            .with_dns(DnsSettings::default())
            .with_local_dns_domains(bound.local_dns_domains)
            .with_route(route)
            .build();
        // TUN inbound needs admin for Wintun; strip it so run -test can
        // schema-check DNS + every catalog selector unelevated.
        value["inbounds"] = serde_json::json!([]);

        let workdir =
            std::env::temp_dir().join(format!("xraybox-catalog-test-{}", std::process::id()));
        std::fs::create_dir_all(&workdir).unwrap();
        let config_path = workdir.join("config.json");
        std::fs::write(&config_path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        let output = Command::new(&sidecar)
            .args(validate_config_args(config_path.to_str().unwrap()))
            .env("XRAY_LOCATION_ASSET", &asset_dir)
            .output()
            .expect("failed to run xray run -test");

        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let _ = std::fs::remove_dir_all(&workdir);
        assert!(
            output.status.success(),
            "xray run -test failed (likely a missing geosite/geoip category): {stderr}{stdout}"
        );
    }
}
