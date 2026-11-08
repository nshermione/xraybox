use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::action::RuleAction;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSettings {
    pub bypass_lan: bool,
    #[serde(default)]
    pub extra_rules: Vec<Value>,
}

impl Default for RouteSettings {
    fn default() -> Self {
        Self {
            bypass_lan: true,
            extra_rules: Vec::new(),
        }
    }
}

pub fn lan_direct_rule() -> Value {
    json!({
        "type": "field",
        "ip": ["geoip:private"],
        "outboundTag": RuleAction::Direct.outbound_tag()
    })
}

pub fn is_literal_ip(host: &str) -> bool {
    parse_host_ip(host).is_some()
}

pub fn host_direct_route_rule(server: &str) -> Value {
    let host = server.trim();
    if let Some(ip) = parse_host_ip(host) {
        let cidr = match ip {
            IpAddr::V4(_) => format!("{ip}/32"),
            IpAddr::V6(_) => format!("{ip}/128"),
        };
        json!({ "type": "field", "ip": [cidr], "outboundTag": RuleAction::Direct.outbound_tag() })
    } else {
        json!({ "type": "field", "domain": [format!("full:{host}")], "outboundTag": RuleAction::Direct.outbound_tag() })
    }
}

pub fn proxy_catch_all_rule() -> Value {
    json!({
        "type": "field",
        "network": "tcp,udp",
        "outboundTag": RuleAction::Proxy.outbound_tag()
    })
}

fn parse_host_ip(host: &str) -> Option<IpAddr> {
    let host = host.trim();
    let host = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    host.parse().ok()
}

pub fn compile_route_rules(settings: &RouteSettings) -> Vec<Value> {
    let mut rules = Vec::new();
    if settings.bypass_lan {
        rules.push(lan_direct_rule());
    }
    rules.extend(settings.extra_rules.clone());
    rules
}

pub fn compile_route_object(settings: &RouteSettings, bootstrap_rules: &[Value]) -> Value {
    let mut rules = bootstrap_rules.to_vec();
    rules.extend(compile_route_rules(settings));
    rules.push(proxy_catch_all_rule());
    json!({
        "domainStrategy": "IPIfNonMatch",
        "rules": rules
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_with_route(settings: RouteSettings) -> Value {
        compile_route_object(&settings, &[host_direct_route_rule("edge.example.com")])
    }

    #[test]
    fn lan_rule_uses_geoip_private() {
        let rule = lan_direct_rule();
        assert_eq!(rule["type"], "field");
        assert_eq!(rule["ip"][0], "geoip:private");
        assert_eq!(rule["outboundTag"], "direct");
        assert!(rule.get("ip_is_private").is_none());
    }

    #[test]
    fn route_object_sets_domain_strategy_and_has_no_rule_set() {
        let routing = compile_route_object(&RouteSettings::default(), &[]);
        assert_eq!(routing["domainStrategy"], "IPIfNonMatch");
        assert!(routing.get("domainMatcher").is_none());
        assert!(routing.get("rule_set").is_none());
        assert!(routing.get("final").is_none());
        assert!(routing.get("default_domain_resolver").is_none());
    }

    #[test]
    fn catch_all_stays_last_even_with_pack_rules() {
        let settings = RouteSettings {
            bypass_lan: true,
            extra_rules: vec![
                json!({ "type": "field", "domain": ["geosite:cn"], "outboundTag": "direct" }),
            ],
        };
        let routing = compile_route_object(&settings, &[]);
        let rules = routing["rules"].as_array().unwrap();
        assert_eq!(rules.last().unwrap()["outboundTag"], "proxy");
        assert_eq!(rules.last().unwrap()["network"], "tcp,udp");
    }

    #[test]
    fn bypass_lan_on_inserts_direct_rule_first() {
        let json = build_with_route(RouteSettings {
            bypass_lan: true,
            extra_rules: vec![],
        });
        let rules = json["rules"].as_array().unwrap();
        assert_eq!(rules[0]["domain"][0], "full:edge.example.com");
        assert_eq!(rules[0]["outboundTag"], "direct");
        assert_eq!(rules[1]["ip"][0], "geoip:private");
        assert_eq!(rules[1]["outboundTag"], "direct");
        assert_eq!(rules.last().unwrap()["outboundTag"], "proxy");
    }

    #[test]
    fn is_literal_ip_detects_addresses() {
        assert!(is_literal_ip("1.1.1.1"));
        assert!(is_literal_ip("2001:db8::1"));
        assert!(!is_literal_ip("edge.example.com"));
    }

    #[test]
    fn host_direct_rule_uses_cidr_for_ip() {
        let ipv4 = host_direct_route_rule("203.0.113.10");
        assert_eq!(ipv4["ip"][0], "203.0.113.10/32");
        assert_eq!(ipv4["outboundTag"], "direct");
        let ipv6 = host_direct_route_rule("[2001:db8::1]");
        assert_eq!(ipv6["ip"][0], "2001:db8::1/128");
    }

    #[test]
    fn bypass_lan_off_has_no_lan_rule() {
        let json = build_with_route(RouteSettings {
            bypass_lan: false,
            extra_rules: vec![],
        });
        let rules = json["rules"].as_array().unwrap();
        assert!(rules.iter().all(|rule| rule.get("ip_is_private").is_none()));
        assert!(rules.iter().all(|rule| rule["ip"]
            .as_array()
            .map(|ips| ips.iter().all(|ip| ip != "geoip:private"))
            .unwrap_or(true)));
    }

    #[test]
    fn rule_order_is_bypass_then_lan_then_packs_then_catch_all() {
        let settings = RouteSettings {
            bypass_lan: true,
            extra_rules: vec![
                json!({ "type": "field", "domain": ["geosite:youtube"], "outboundTag": "direct" }),
                json!({ "type": "field", "domain": ["geosite:google"], "outboundTag": "proxy" }),
                json!({ "type": "field", "domain": ["geosite:cn"], "outboundTag": "direct" }),
            ],
        };
        let routing =
            compile_route_object(&settings, &[host_direct_route_rule("edge.example.com")]);
        let rules = routing["rules"].as_array().unwrap();
        assert_eq!(rules[0]["domain"][0], "full:edge.example.com");
        assert_eq!(rules[0]["outboundTag"], "direct");
        assert_eq!(rules[1]["ip"][0], "geoip:private");
        assert_eq!(rules[2]["domain"][0], "geosite:youtube");
        assert_eq!(rules[3]["domain"][0], "geosite:google");
        assert_eq!(rules[4]["domain"][0], "geosite:cn");
        assert_eq!(rules[5]["outboundTag"], "proxy");
        assert_eq!(rules[5]["network"], "tcp,udp");
        assert_eq!(rules.len(), 6);
    }

    #[test]
    fn existing_route_json_with_rule_sets_still_loads() {
        let loaded: RouteSettings = serde_json::from_str(
            r#"{"bypassLan":true,"extraRules":[],"ruleSets":[{"tag":"geosite-cn"}]}"#,
        )
        .unwrap();
        assert!(loaded.bypass_lan);
        assert!(loaded.extra_rules.is_empty());
    }
}
