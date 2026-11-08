pub mod store;

use serde::{Deserialize, Serialize};

use crate::engine::error::EngineError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsSettings {
    pub remote_servers: Vec<String>,
    #[serde(default)]
    pub fallback_to_system: bool,
}

impl Default for DnsSettings {
    fn default() -> Self {
        Self {
            remote_servers: vec!["1.1.1.1".into(), "8.8.8.8".into()],
            fallback_to_system: false,
        }
    }
}

impl DnsSettings {
    pub fn validate(&self) -> Result<(), EngineError> {
        if self.remote_servers.is_empty() {
            return Err(EngineError::Config(
                "at least one remote DNS server is required".into(),
            ));
        }
        for server in &self.remote_servers {
            if !valid_dns_host(server) {
                return Err(EngineError::Config(format!("invalid DNS server: {server}")));
            }
        }
        Ok(())
    }
}

/// Resolvers to assign to the Windows TUN adapter.
///
/// Without this the adapter has no DNS server, so Windows keeps resolving
/// through the physical NIC's servers. When those are a LAN address the query
/// takes the on-link route, which beats the TUN default route, and every lookup
/// reaches the ISP outside the tunnel. The `dns` block this module also builds
/// does not help: it is Xray's own resolver for routing and `freedom` dialling,
/// not the one applications use.
///
/// Windows `SetDNS` takes literal addresses, and the tunnel carries IPv4 only,
/// so hostname and IPv6 entries are dropped rather than passed through.
pub fn tun_dns_servers(settings: &DnsSettings) -> Vec<String> {
    settings
        .remote_servers
        .iter()
        .map(|server| server.trim())
        .filter(|server| server.parse::<std::net::Ipv4Addr>().is_ok())
        .map(str::to_string)
        .collect()
}

pub fn dns_geosite_to_local(rule_set_tag: &str) -> String {
    rule_set_tag.to_string()
}

pub fn dns_domain_to_local(domain: &str) -> String {
    format!("full:{domain}")
}

pub fn build_dns(
    settings: &DnsSettings,
    local_domains: &[String],
    proxy_host: Option<&str>,
) -> serde_json::Value {
    let mut domains = local_domains.to_vec();
    if let Some(host) = proxy_host.map(str::trim).filter(|h| !h.is_empty()) {
        if !crate::routing::compile::is_literal_ip(host) {
            domains.push(format!("full:{host}"));
        }
    }

    let mut servers = Vec::new();
    if !domains.is_empty() {
        servers.push(serde_json::json!({ "address": "localhost", "domains": domains }));
    }
    servers.extend(
        settings
            .remote_servers
            .iter()
            .map(|host| serde_json::json!(host)),
    );
    if settings.fallback_to_system {
        // Unrestricted and last: 09 already puts a domain-scoped localhost
        // first for China and the proxy host. This one only answers after
        // the remotes have failed.
        servers.push(serde_json::json!("localhost"));
    }

    let mut dns = serde_json::json!({ "servers": servers, "queryStrategy": "UseIPv4" });
    if settings.fallback_to_system {
        dns["disableFallback"] = serde_json::json!(false);
    }
    dns
}

fn valid_dns_host(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.contains(' ') || value.contains("://") {
        return false;
    }
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':' | '[' | ']'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::config::ConfigBuilder;
    use crate::profile::VlessReality;

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
    fn dns_puts_local_first_with_domains_then_remotes() {
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .with_dns(DnsSettings::default())
            .with_local_dns_domains(vec![dns_geosite_to_local("geosite:cn")])
            .build();

        let servers = json["dns"]["servers"].as_array().unwrap();
        assert_eq!(servers[0]["address"], "localhost");
        let domains = servers[0]["domains"].as_array().unwrap();
        assert!(domains.iter().any(|d| d == "geosite:cn"));
        assert!(domains.iter().any(|d| d == "full:edge.example.com"));

        assert_eq!(servers[1], "1.1.1.1");
        assert_eq!(servers[2], "8.8.8.8");

        assert_eq!(json["dns"]["queryStrategy"], "UseIPv4");
        assert!(json["dns"].get("final").is_none());
        assert!(json["dns"].get("rules").is_none());
        assert!(json["dns"].get("strategy").is_none());
    }

    #[test]
    fn fallback_appends_localhost_last_when_enabled() {
        let mut dns = DnsSettings::default();
        assert!(!dns.fallback_to_system); // privacy-safe default

        dns.fallback_to_system = true;
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .with_dns(dns)
            .build();
        let servers = json["dns"]["servers"].as_array().unwrap();
        assert_eq!(servers.last().unwrap(), "localhost");
        assert_eq!(json["dns"]["disableFallback"], false);
    }

    #[test]
    fn default_remote_servers() {
        let dns = DnsSettings::default();
        assert_eq!(dns.remote_servers, ["1.1.1.1", "8.8.8.8"]);
    }

    #[test]
    fn rejects_empty_remote_list() {
        let mut dns = DnsSettings::default();
        dns.remote_servers.clear();
        assert!(dns.validate().is_err());
    }

    #[test]
    fn rejects_invalid_host() {
        let mut dns = DnsSettings::default();
        dns.remote_servers = vec!["not a host!!!".into()];
        assert!(dns.validate().is_err());
    }

    #[test]
    fn json_contains_local_and_both_remotes() {
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .with_dns(DnsSettings::default())
            .build();
        let text = json.to_string();
        assert!(text.contains("1.1.1.1"));
        assert!(text.contains("8.8.8.8"));
        let servers = json["dns"]["servers"].as_array().unwrap();
        assert_eq!(servers[0]["address"], "localhost");
        let domains = servers[0]["domains"].as_array().unwrap();
        assert!(domains.iter().any(|d| d == "full:edge.example.com"));
        assert!(servers.iter().any(|server| server == "1.1.1.1"));
        assert!(servers.iter().any(|server| server == "8.8.8.8"));
        assert!(json["dns"].get("fakeip").is_none());
        assert!(json["dns"].get("rules").is_none());
        assert!(!text.contains("fakeip"));
        assert!(!text.contains("198.18"));
        assert_eq!(json["dns"]["queryStrategy"], "UseIPv4");
    }

    #[test]
    fn china_local_rule_stays_before_remote() {
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .with_dns(DnsSettings::default())
            .with_local_dns_domains(vec![dns_geosite_to_local("geosite:cn")])
            .build();
        let servers = json["dns"]["servers"].as_array().unwrap();
        assert_eq!(servers[0]["address"], "localhost");
        assert!(servers[0]["domains"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d == "geosite:cn"));
        let remote = servers
            .iter()
            .position(|server| server == "1.1.1.1")
            .unwrap();
        assert!(0 < remote);
    }

    #[test]
    fn tun_dns_servers_keeps_ipv4_literals_in_order() {
        let settings = DnsSettings {
            remote_servers: vec!["1.1.1.1".into(), " 9.9.9.9 ".into()],
            fallback_to_system: false,
        };
        assert_eq!(tun_dns_servers(&settings), ["1.1.1.1", "9.9.9.9"]);
    }

    #[test]
    fn tun_dns_servers_drops_hostnames_and_ipv6() {
        // Windows SetDNS takes literal addresses, and the TUN carries IPv4 only.
        let settings = DnsSettings {
            remote_servers: vec![
                "dns.example.com".into(),
                "2606:4700:4700::1111".into(),
                "8.8.8.8".into(),
            ],
            fallback_to_system: false,
        };
        assert_eq!(tun_dns_servers(&settings), ["8.8.8.8"]);
    }

    #[test]
    fn tun_dns_servers_defaults_match_the_remote_list() {
        assert_eq!(
            tun_dns_servers(&DnsSettings::default()),
            DnsSettings::default().remote_servers
        );
    }

    #[test]
    fn dns_geosite_to_local_is_selector() {
        assert_eq!(dns_geosite_to_local("geosite:cn"), "geosite:cn");
    }

    #[test]
    fn dns_domain_to_local_uses_full_prefix() {
        assert_eq!(
            dns_domain_to_local("edge.example.com"),
            "full:edge.example.com"
        );
    }

    #[test]
    fn ip_proxy_host_skips_domain_local_rule() {
        let mut profile = sample_profile();
        profile.server = "203.0.113.10".into();
        let json = ConfigBuilder::default()
            .with_proxy(profile)
            .with_tun()
            .with_dns(DnsSettings::default())
            .build();
        let servers = json["dns"]["servers"].as_array().unwrap();
        assert!(servers.iter().all(|server| server.get("domains").is_none()));
        assert_eq!(servers[0], "1.1.1.1");
        assert_eq!(json["routing"]["rules"][0]["ip"][0], "203.0.113.10/32");
        assert_eq!(json["routing"]["rules"][0]["outboundTag"], "direct");
    }
}
