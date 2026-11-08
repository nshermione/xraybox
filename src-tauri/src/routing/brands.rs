use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::action::RuleAction;
use super::catalog::{ordered_packs, pack_def, service_tag, PackState, CN_CDN_TAGS, OVERSEAS_TAG};
use crate::dns::dns_geosite_to_local;
use crate::engine::error::EngineError;

pub fn compile_packs(packs: &[PackState]) -> Vec<Value> {
    let ordered = ordered_packs(packs);
    let mut rules = Vec::new();

    for pack in &ordered {
        if !pack.enabled {
            continue;
        }
        for service in &pack.services {
            if let Some(tag) = service_tag(&pack.id, &service.id) {
                rules.push(geo_outbound(tag, service.action));
            }
        }
    }

    if let Some(china) = enabled_pack(&ordered, "china") {
        for tag in CN_CDN_TAGS {
            rules.push(geo_outbound(tag, china.action));
        }
    }

    for pack in &ordered {
        if !pack.enabled || pack.id == "china" {
            continue;
        }
        let Some(def) = pack_def(&pack.id) else {
            continue;
        };
        for tag in def.umbrella_tags {
            rules.push(geo_outbound(tag, pack.action));
        }
    }

    rules.push(geo_outbound(OVERSEAS_TAG, RuleAction::Proxy));

    if let Some(china) = enabled_pack(&ordered, "china") {
        if let Some(def) = pack_def("china") {
            for tag in def.umbrella_tags {
                rules.push(geo_outbound(tag, china.action));
            }
        }
    }

    rules
}

pub fn compile_dns_rules(packs: &[PackState]) -> Vec<String> {
    let ordered = ordered_packs(packs);
    let mut rules = Vec::new();
    for pack in &ordered {
        if !pack.enabled {
            continue;
        }
        let Some(def) = pack_def(&pack.id) else {
            continue;
        };
        if pack.id == "china" || pack.action == RuleAction::Direct {
            for tag in def.umbrella_tags {
                if tag.starts_with("geosite:") {
                    rules.push(dns_geosite_to_local(tag));
                }
            }
        }
        for service in &pack.services {
            if service.action == RuleAction::Direct {
                if let Some(tag) = service_tag(&pack.id, &service.id) {
                    if tag.starts_with("geosite:") {
                        rules.push(dns_geosite_to_local(tag));
                    }
                }
            }
        }
    }
    if let Some(china) = enabled_pack(&ordered, "china") {
        for tag in CN_CDN_TAGS {
            let proxied = china.services.iter().any(|service| {
                service_tag("china", &service.id) == Some(tag)
                    && service.action != RuleAction::Direct
            });
            if !proxied {
                rules.push(dns_geosite_to_local(tag));
            }
        }
    }
    rules
}

fn enabled_pack<'a>(packs: &'a [PackState], id: &str) -> Option<&'a PackState> {
    packs.iter().find(|pack| pack.id == id && pack.enabled)
}

fn geo_outbound(tag: &str, action: RuleAction) -> Value {
    let mut rule = json!({ "type": "field", "outboundTag": action.outbound_tag() });
    if tag.starts_with("geoip:") {
        rule["ip"] = json!([tag]);
    } else {
        rule["domain"] = json!([tag]);
    }
    rule
}

pub struct BoundRoute {
    pub extra_rules: Vec<Value>,
    pub local_dns_domains: Vec<String>,
    pub warnings: Vec<String>,
}

/// Two geo files are already guaranteed by `ensure_geo_assets`. Pack rules
/// are compiled as-is; `warnings` stays plumbed for later diagnostics.
pub fn bind_route(
    packs: Vec<PackState>,
    _cache_dir: &Path,
    _bundle_dirs: &[PathBuf],
) -> Result<BoundRoute, EngineError> {
    Ok(BoundRoute {
        extra_rules: compile_packs(&packs),
        local_dns_domains: compile_dns_rules(&packs),
        warnings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::catalog::{catalog_defaults, ServiceOverride};
    use super::*;

    fn off(id: &str) -> PackState {
        PackState {
            id: id.into(),
            enabled: false,
            action: RuleAction::Proxy,
            services: vec![],
        }
    }

    fn on(id: &str, action: RuleAction) -> PackState {
        PackState {
            id: id.into(),
            enabled: true,
            action,
            services: vec![],
        }
    }

    fn r_has_selector(rule: &Value, tag: &str) -> bool {
        let in_domain = rule["domain"]
            .as_array()
            .map(|tags| tags.iter().any(|value| value == tag))
            .unwrap_or(false);
        let in_ip = rule["ip"]
            .as_array()
            .map(|tags| tags.iter().any(|value| value == tag))
            .unwrap_or(false);
        in_domain || in_ip
    }

    fn position(rules: &[Value], tag: &str) -> usize {
        rules
            .iter()
            .position(|rule| r_has_selector(rule, tag))
            .expect(tag)
    }

    #[test]
    fn pack_rule_splits_domain_and_ip_selectors() {
        let domain_rule = geo_outbound("geosite:cn", RuleAction::Direct);
        assert_eq!(domain_rule["domain"][0], "geosite:cn");
        assert!(domain_rule.get("ip").is_none());

        let ip_rule = geo_outbound("geoip:cn", RuleAction::Direct);
        assert_eq!(ip_rule["ip"][0], "geoip:cn");
        assert!(ip_rule.get("domain").is_none());
    }

    #[test]
    fn defaults_china_on_and_brands_off() {
        let packs = catalog_defaults();
        let china = packs.iter().find(|pack| pack.id == "china").unwrap();
        assert!(china.enabled);
        assert!(matches!(china.action, RuleAction::Direct));
        assert!(packs
            .iter()
            .filter(|pack| pack.id != "china")
            .all(|pack| !pack.enabled));
    }

    #[test]
    fn disabled_pack_emits_no_brand_selector() {
        let rules = compile_packs(&[off("google")]);
        assert!(!rules
            .iter()
            .any(|rule| r_has_selector(rule, "geosite:google")));
        assert!(rules
            .iter()
            .any(|rule| r_has_selector(rule, OVERSEAS_TAG) && rule["outboundTag"] == "proxy"));
    }

    #[test]
    fn google_on_proxy_emits_geosite_google() {
        let rules = compile_packs(&[on("google", RuleAction::Proxy)]);
        assert!(rules
            .iter()
            .any(|rule| r_has_selector(rule, "geosite:google") && rule["outboundTag"] == "proxy"));
    }

    #[test]
    fn youtube_override_appears_before_google_umbrella() {
        let mut google = on("google", RuleAction::Proxy);
        google.services = vec![ServiceOverride {
            id: "youtube".into(),
            action: RuleAction::Direct,
        }];
        let rules = compile_packs(&[google]);
        let youtube = position(&rules, "geosite:youtube");
        let umbrella = position(&rules, "geosite:google");
        assert!(youtube < umbrella);
    }

    #[test]
    fn china_emits_geosite_and_geoip_direct() {
        let rules = compile_packs(&[on("china", RuleAction::Direct)]);
        assert!(rules
            .iter()
            .any(|rule| r_has_selector(rule, "geosite:cn") && rule["outboundTag"] == "direct"));
        assert!(rules
            .iter()
            .any(|rule| r_has_selector(rule, "geoip:cn") && rule["outboundTag"] == "direct"));
        assert!(rules.iter().any(
            |rule| r_has_selector(rule, "geosite:apple-cn") && rule["outboundTag"] == "direct"
        ));
        assert!(rules.iter().any(
            |rule| r_has_selector(rule, "geosite:google-cn") && rule["outboundTag"] == "direct"
        ));
    }

    #[test]
    fn brand_before_china_in_output() {
        let rules = compile_packs(&[
            on("google", RuleAction::Proxy),
            on("china", RuleAction::Direct),
        ]);
        assert!(position(&rules, "geosite:google") < position(&rules, "geosite:cn"));
    }

    #[test]
    fn google_cn_policy_before_google_umbrella() {
        let rules = compile_packs(&[
            on("google", RuleAction::Proxy),
            on("china", RuleAction::Direct),
        ]);
        assert!(position(&rules, "geosite:google-cn") < position(&rules, "geosite:google"));
        assert!(position(&rules, "geosite:google") < position(&rules, OVERSEAS_TAG));
        assert!(position(&rules, OVERSEAS_TAG) < position(&rules, "geosite:cn"));
    }

    #[test]
    fn china_cn_cdn_override_beats_policy() {
        let mut china = on("china", RuleAction::Direct);
        china.services = vec![ServiceOverride {
            id: "google-cn".into(),
            action: RuleAction::Proxy,
        }];
        let rules = compile_packs(&[on("google", RuleAction::Proxy), china]);
        let override_pos = position(&rules, "geosite:google-cn");
        assert_eq!(rules[override_pos]["outboundTag"], "proxy");
        assert!(override_pos < position(&rules, "geosite:google"));
    }

    #[test]
    fn china_off_skips_cn_cdn_tags() {
        let rules = compile_packs(&[on("google", RuleAction::Proxy)]);
        assert!(!rules
            .iter()
            .any(|rule| r_has_selector(rule, "geosite:apple-cn")));
        assert!(!rules
            .iter()
            .any(|rule| r_has_selector(rule, "geosite:google-cn")));
        assert!(!rules.iter().any(|rule| r_has_selector(rule, "geosite:cn")));
    }

    #[test]
    fn telegram_emits_domain_and_ip() {
        let rules = compile_packs(&[on("telegram", RuleAction::Proxy)]);
        assert!(
            rules
                .iter()
                .any(|rule| r_has_selector(rule, "geosite:telegram")
                    && rule["outboundTag"] == "proxy")
        );
        assert!(rules
            .iter()
            .any(|rule| r_has_selector(rule, "geoip:telegram") && rule["outboundTag"] == "proxy"));
    }

    #[test]
    fn china_adds_dns_local_rule() {
        let dns_rules = compile_dns_rules(&[on("china", RuleAction::Direct)]);
        assert!(dns_rules.iter().any(|tag| tag == "geosite:cn"));
        assert!(dns_rules.iter().any(|tag| tag == "geosite:apple-cn"));
        assert!(dns_rules.iter().any(|tag| tag == "geosite:google-cn"));
    }

    #[test]
    fn china_cn_cdn_proxy_skips_local_dns() {
        let mut china = on("china", RuleAction::Direct);
        china.services = vec![ServiceOverride {
            id: "google-cn".into(),
            action: RuleAction::Proxy,
        }];
        let dns_rules = compile_dns_rules(&[china]);
        assert!(!dns_rules.iter().any(|tag| tag == "geosite:google-cn"));
        assert!(dns_rules.iter().any(|tag| tag == "geosite:apple-cn"));
    }

    fn unique_temp(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "xraybox-bind-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn defaults_bind_cn_only_no_google_rule() {
        let cache = unique_temp("defaults");
        std::fs::create_dir_all(&cache).unwrap();
        let bound = bind_route(catalog_defaults(), &cache, &[]).unwrap();
        assert!(bound
            .extra_rules
            .iter()
            .any(|rule| r_has_selector(rule, "geosite:cn")));
        assert!(bound
            .extra_rules
            .iter()
            .any(|rule| r_has_selector(rule, "geoip:cn")));
        assert!(!bound
            .extra_rules
            .iter()
            .any(|rule| r_has_selector(rule, "geosite:google")));
        assert!(bound.warnings.is_empty());
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn google_on_emits_google_rule_without_per_tag_files() {
        let cache = unique_temp("google-on");
        std::fs::create_dir_all(&cache).unwrap();
        let mut packs = catalog_defaults();
        packs
            .iter_mut()
            .find(|pack| pack.id == "google")
            .unwrap()
            .enabled = true;
        let bound = bind_route(packs, &cache, &[]).unwrap();
        assert!(bound
            .extra_rules
            .iter()
            .any(|rule| r_has_selector(rule, "geosite:google")));
        assert!(bound.warnings.is_empty());
        let _ = std::fs::remove_dir_all(&cache);
    }
}
