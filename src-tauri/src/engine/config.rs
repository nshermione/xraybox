use serde_json::{json, Value};

use crate::dns::DnsSettings;
use crate::profile::VlessReality;
use crate::routing::compile::{compile_route_object, host_direct_route_rule};
use crate::routing::RouteSettings;

pub const PROXY_OUTBOUND_TAG: &str = "proxy";
pub const API_LISTEN: &str = "127.0.0.1:9090";

pub struct ConfigBuilder {
    log_level: String,
    // 09/11 extend build() to emit these; signatures stay so later files can compose.
    proxy: Option<VlessReality>,
    mixed_inbound: Option<(String, u16)>,
    tun: bool,
    dns: Option<DnsSettings>,
    local_dns_domains: Vec<String>,
    route: Option<RouteSettings>,
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self {
            log_level: "warning".into(),
            proxy: None,
            mixed_inbound: None,
            tun: false,
            dns: None,
            local_dns_domains: Vec::new(),
            route: None,
        }
    }
}

impl ConfigBuilder {
    pub fn with_log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = level.into();
        self
    }

    pub fn with_proxy(mut self, profile: VlessReality) -> Self {
        self.proxy = Some(profile);
        self
    }

    pub fn with_mixed_inbound(mut self, listen: impl Into<String>, port: u16) -> Self {
        self.mixed_inbound = Some((listen.into(), port));
        self
    }

    pub fn with_smoke_mixed(self, enabled: bool) -> Self {
        if enabled {
            self.with_mixed_inbound("127.0.0.1", 7890)
        } else {
            self
        }
    }

    pub fn with_mixed_inbound_if_enabled(self) -> Self {
        self.with_smoke_mixed(smoke_mixed_enabled())
    }

    pub fn with_tun(mut self) -> Self {
        self.tun = true;
        self
    }

    pub fn with_dns(mut self, settings: DnsSettings) -> Self {
        self.dns = Some(settings);
        self
    }

    pub fn with_local_dns_domains(mut self, domains: Vec<String>) -> Self {
        self.local_dns_domains = domains;
        self
    }

    pub fn with_route(mut self, settings: RouteSettings) -> Self {
        self.route = Some(settings);
        self
    }

    pub fn build(&self) -> Value {
        let mut config = json!({
            "log": { "loglevel": self.log_level },
            "inbounds": self.inbounds(),
            "outbounds": self.outbounds(),
            "stats": {},
            "policy": policy(cfg!(target_os = "ios")),
            "api": {
                "tag": "api",
                "listen": API_LISTEN,
                "services": ["HandlerService", "RoutingService", "StatsService"]
            }
        });
        if let Some(routing) = self.routing() {
            config["routing"] = routing;
        }
        if let Some(dns) = &self.dns {
            let host = self.proxy.as_ref().map(|profile| profile.server.as_str());
            config["dns"] = crate::dns::build_dns(dns, &self.local_dns_domains, host);
        }
        config
    }

    fn routing(&self) -> Option<Value> {
        if !self.tun {
            return None;
        }
        let mut bootstrap = Vec::new();
        if let Some(profile) = &self.proxy {
            bootstrap.push(host_direct_route_rule(&profile.server));
        }
        bootstrap.extend(ios_bootstrap_rules(cfg!(target_os = "ios")));
        let settings = self.route.clone().unwrap_or_default();
        Some(compile_route_object(&settings, &bootstrap))
    }

    fn inbounds(&self) -> Vec<Value> {
        let mut inbounds = Vec::new();
        if self.tun {
            inbounds.push(self.tun_inbound());
        }
        if let Some((listen, port)) = &self.mixed_inbound {
            inbounds.push(json!({
                "tag": "mixed-in",
                "protocol": "socks",
                "listen": listen,
                "port": port,
                "settings": { "udp": true }
            }));
        }
        inbounds
    }

    fn tun_inbound(&self) -> Value {
        if cfg!(target_os = "ios") {
            ios_socks_inbound()
        } else if cfg!(target_os = "android") {
            android_tun_inbound()
        } else if cfg!(target_os = "macos") {
            macos_tun_inbound()
        } else {
            windows_tun_inbound(&self.tun_dns_servers())
        }
    }

    fn tun_dns_servers(&self) -> Vec<String> {
        self.dns
            .as_ref()
            .map(crate::dns::tun_dns_servers)
            .unwrap_or_default()
    }

    fn outbounds(&self) -> Vec<Value> {
        if let Some(profile) = &self.proxy {
            // Xray sends unmatched traffic, and traffic whose rule names a missing tag,
            // to the first outbound. `10` hot-swaps `proxy` by remove+add, so `block`
            // must sit first: a missing proxy fails closed instead of leaking to `direct`.
            // The catch-all in Task 5 is what carries normal traffic to `proxy`.
            vec![
                json!({ "tag": "block", "protocol": "blackhole" }),
                profile.to_outbound(PROXY_OUTBOUND_TAG),
                json!({ "tag": "direct", "protocol": "freedom" }),
            ]
        } else {
            vec![
                json!({ "tag": "direct", "protocol": "freedom" }),
                json!({ "tag": "block", "protocol": "blackhole" }),
            ]
        }
    }
}

/// Seconds an idle iOS connection is kept. Each one holds TLS state and pipe
/// buffers inside the 50MB extension budget; Xray's default is 300.
const IOS_CONN_IDLE_SECONDS: u32 = 120;

/// Rules that replace the iOS packet filter. hev reads the utun directly, so
/// Swift never sees a packet. These sit after the server bypass and before
/// `geoip:private`, or DNS to a LAN resolver would go `direct`.
fn ios_bootstrap_rules(ios: bool) -> Vec<Value> {
    if !ios {
        return Vec::new();
    }
    vec![
        json!({
            "type": "field",
            "port": "53",
            "ip": ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "100.64.0.0/10", "169.254.0.0/16"],
            "outboundTag": "block"
        }),
        json!({
            "type": "field",
            "ip": ["::/0"],
            "outboundTag": "block"
        }),
    ]
}

fn policy(ios: bool) -> Value {
    let mut policy = json!({
        "system": {
            "statsOutboundUplink": true,
            "statsOutboundDownlink": true
        }
    });
    if ios {
        policy["levels"] = json!({ "0": { "connIdle": IOS_CONN_IDLE_SECONDS } });
    }
    policy
}

pub fn smoke_mixed_enabled() -> bool {
    std::env::var("XRAYBOX_SMOKE_MIXED").ok().as_deref() == Some("1")
}

/// iOS has no TUN fd. The packet tunnel feeds this loopback SOCKS inbound.
/// Sniffing stays on so domain rules still see HTTP, TLS, and QUIC.
pub fn ios_socks_inbound() -> Value {
    json!({
        "tag": "socks-in",
        "protocol": "socks",
        "listen": "127.0.0.1",
        "port": crate::engine::ios::SOCKS_PORT,
        "settings": { "udp": true },
        "sniffing": {
            "enabled": true,
            "destOverride": ["http", "tls", "quic"]
        }
    })
}

fn android_tun_inbound() -> Value {
    // `name` must be set. Empty name calls net.Interfaces() → netlinkrib
    // permission denied on Android. The name is unused: VpnService already
    // created the iface and Xray reads it from XRAY_TUN_FD.
    json!({
        "tag": "tun-in",
        "protocol": "tun",
        "settings": {
            "name": "xray0",
            "mtu": 1500
        },
        "sniffing": {
            "enabled": true,
            "destOverride": ["http", "tls", "quic"]
        }
    })
}

/// macOS utun. `dns` is omitted: Xray does not apply it on Darwin (system DNS
/// stays with the OS). `desc` is a Wintun label and must not be set here.
/// `::/0` is omitted on purpose; leak guard blocks global IPv6 instead.
fn macos_tun_inbound() -> Value {
    json!({
        "tag": "tun-in",
        "protocol": "tun",
        "settings": {
            "name": crate::engine::macos::TUN_NAME,
            "mtu": 1500,
            "gateway": [crate::engine::windows::TUN_IPV4_GATEWAY],
            "autoSystemRoutingTable": ["0.0.0.0/0"],
            "autoOutboundsInterface": "auto"
        },
        "sniffing": {
            "enabled": true,
            "destOverride": ["http", "tls", "quic"]
        }
    })
}

fn windows_tun_inbound(dns_servers: &[String]) -> Value {
    let mut settings = json!({
        "name": crate::engine::windows::TUN_INTERFACE_NAME,
        "desc": "Wintun",
        "mtu": 9000,
        "gateway": [crate::engine::windows::TUN_IPV4_GATEWAY],
        // CIDRs, not table names: v26.9.9 parses each item with netip.ParsePrefix
        // and calls SetRoutesForFamily / SetIPAddressesForFamily / SetDNS.
        //
        // IPv4 only, and `::/0` is deliberately absent: the gateway above gives
        // the adapter no routable IPv6 address, so claiming the IPv6 default
        // route would black-hole it. `engine::leak_guard` blocks global IPv6
        // instead, which is what keeps the real address off the wire.
        "autoSystemRoutingTable": ["0.0.0.0/0"],
        "autoOutboundsInterface": "auto"
    });
    // Windows-only field, added alongside `gateway` in Xray PR #5887. Omitting
    // it leaves the adapter with no resolver, and Windows then falls back to the
    // physical NIC's servers — a DNS leak whenever those are a LAN address.
    if !dns_servers.is_empty() {
        settings["dns"] = json!(dns_servers);
    }
    json!({
        "tag": "tun-in",
        "protocol": "tun",
        "settings": settings,
        "sniffing": {
            "enabled": true,
            "destOverride": ["http", "tls", "quic"]
        }
    })
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
    fn proxy_tag_matches_rule_action() {
        assert_eq!(
            PROXY_OUTBOUND_TAG,
            crate::routing::action::RuleAction::Proxy.outbound_tag()
        );
    }

    #[test]
    fn config_has_no_secret_anywhere() {
        let text = ConfigBuilder::default().build().to_string();
        assert!(!text.contains("secret"));
        assert_eq!(
            ConfigBuilder::default().build()["api"]["listen"],
            API_LISTEN
        );
    }

    #[test]
    fn minimal_config_has_log_freedom_and_api() {
        let json = ConfigBuilder::default().build();
        assert_eq!(json["log"]["loglevel"], "warning");

        let outbounds = json["outbounds"].as_array().unwrap();
        assert_eq!(outbounds[0]["tag"], "direct");
        assert_eq!(outbounds[0]["protocol"], "freedom");
        assert_eq!(outbounds[1]["tag"], "block");
        assert_eq!(outbounds[1]["protocol"], "blackhole");

        assert_eq!(json["api"]["listen"], "127.0.0.1:9090");
        assert!(json["api"]["services"]
            .as_array()
            .unwrap()
            .iter()
            .any(|service| service == "HandlerService"));
        assert!(json["api"]["services"]
            .as_array()
            .unwrap()
            .iter()
            .any(|service| service == "StatsService"));
        assert_eq!(json["policy"]["system"]["statsOutboundUplink"], true);
        assert_eq!(json["policy"]["system"]["statsOutboundDownlink"], true);
        assert!(json.get("stats").is_some());
        assert!(json.get("experimental").is_none());
    }

    #[test]
    fn ios_policy_shortens_idle_connections_only_on_ios() {
        let ios = policy(true);
        assert_eq!(ios["levels"]["0"]["connIdle"], IOS_CONN_IDLE_SECONDS);
        assert_eq!(ios["system"]["statsOutboundUplink"], true);
        assert!(policy(false).get("levels").is_none());
    }

    #[test]
    fn with_log_level_overrides_default_warning() {
        let json = ConfigBuilder::default().with_log_level("debug").build();
        assert_eq!(json["log"]["loglevel"], "debug");
    }

    #[test]
    fn writes_pretty_json_roundtrip() {
        let value = ConfigBuilder::default().build();
        let text = serde_json::to_string_pretty(&value).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, value);
    }

    #[test]
    fn block_is_first_so_a_missing_proxy_fails_closed() {
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .build();
        let tags: Vec<&str> = json["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|outbound| outbound["tag"].as_str().unwrap())
            .collect();
        assert_eq!(tags, ["block", "proxy", "direct"]);
        assert_eq!(json["outbounds"][0]["protocol"], "blackhole");
    }

    #[test]
    fn mixed_inbound_included_when_enabled() {
        let json = ConfigBuilder::default().with_smoke_mixed(true).build();
        let inbound = &json["inbounds"][0];
        assert_eq!(inbound["tag"], "mixed-in");
        assert_eq!(inbound["protocol"], "socks");
        assert_eq!(inbound["listen"], "127.0.0.1");
        assert_eq!(inbound["port"], 7890);
        assert_eq!(inbound["settings"]["udp"], true);
    }

    #[test]
    fn mixed_inbound_omitted_when_disabled() {
        let json = ConfigBuilder::default().with_smoke_mixed(false).build();
        assert!(json["inbounds"].as_array().unwrap().is_empty());
    }

    #[test]
    fn android_tun_inbound_has_no_wintun() {
        let tun = android_tun_inbound();
        assert_eq!(tun["tag"], "tun-in");
        assert_eq!(tun["protocol"], "tun");
        assert_eq!(tun["settings"]["name"], "xray0");
        assert_eq!(tun["settings"]["mtu"], 1500);
        assert!(tun["settings"].get("desc").is_none());
        assert!(tun["settings"].get("autoSystemRoutingTable").is_none());
        assert!(tun["settings"].get("autoOutboundsInterface").is_none());
        assert_eq!(tun["sniffing"]["enabled"], true);
    }

    #[test]
    fn windows_tun_inbound_sets_wintun_dns_and_ipv4_route() {
        let tun = windows_tun_inbound(&["1.1.1.1".into(), "8.8.8.8".into()]);
        assert_eq!(tun["settings"]["name"], "xraybox-tun");
        assert_eq!(tun["settings"]["desc"], "Wintun");
        assert_eq!(tun["settings"]["mtu"], 9000);
        assert_eq!(tun["settings"]["dns"][0], "1.1.1.1");
        assert_eq!(tun["settings"]["dns"][1], "8.8.8.8");
        assert_eq!(tun["settings"]["autoSystemRoutingTable"][0], "0.0.0.0/0");
        assert!(tun["settings"]["autoSystemRoutingTable"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry != "::/0"));
    }

    #[test]
    fn macos_tun_inbound_is_utun_without_wintun_or_ipv6_default() {
        let tun = macos_tun_inbound();
        assert_eq!(tun["tag"], "tun-in");
        assert_eq!(tun["protocol"], "tun");
        assert_eq!(tun["settings"]["name"], "utun10");
        assert_eq!(tun["settings"]["mtu"], 1500);
        assert!(tun["settings"].get("desc").is_none());
        assert!(tun["settings"].get("dns").is_none());
        assert_eq!(tun["settings"]["gateway"][0], "172.19.0.1/30");
        assert_eq!(tun["settings"]["autoOutboundsInterface"], "auto");
        let table = tun["settings"]["autoSystemRoutingTable"]
            .as_array()
            .unwrap();
        assert_eq!(table, &vec![json!("0.0.0.0/0")]);
        assert_eq!(tun["sniffing"]["enabled"], true);
    }

    #[test]
    fn ios_socks_inbound_is_loopback_with_sniffing() {
        let inbound = ios_socks_inbound();
        assert_eq!(inbound["tag"], "socks-in");
        assert_eq!(inbound["protocol"], "socks");
        assert_eq!(inbound["listen"], "127.0.0.1");
        assert_eq!(inbound["port"], 10808);
        assert_eq!(inbound["settings"]["udp"], true);
        assert_eq!(inbound["sniffing"]["enabled"], true);
        let overrides = inbound["sniffing"]["destOverride"].as_array().unwrap();
        assert_eq!(overrides, &vec![json!("http"), json!("tls"), json!("quic")]);
        assert!(inbound["settings"].get("dns").is_none());
    }

    #[test]
    #[cfg(not(target_os = "ios"))]
    fn tun_inbound_assigns_the_remote_resolvers_to_the_adapter() {
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .with_dns(DnsSettings::default())
            .build();
        let dns = json["inbounds"][0]["settings"].get("dns");
        #[cfg(target_os = "macos")]
        assert!(dns.is_none());
        #[cfg(not(target_os = "macos"))]
        {
            let dns = dns.unwrap().as_array().unwrap();
            assert_eq!(dns, &vec![json!("1.1.1.1"), json!("8.8.8.8")]);
        }
    }

    #[test]
    fn tun_inbound_omits_dns_when_no_ipv4_remote_is_configured() {
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .with_dns(DnsSettings {
                remote_servers: vec!["dns.example.com".into(), "2606:4700:4700::1111".into()],
                fallback_to_system: false,
            })
            .build();
        assert!(json["inbounds"][0]["settings"].get("dns").is_none());
    }

    #[test]
    fn tun_inbound_without_dns_settings_has_no_dns_field() {
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .build();
        assert!(json["inbounds"][0]["settings"].get("dns").is_none());
    }

    #[test]
    #[cfg(not(target_os = "ios"))]
    fn tun_claims_the_ipv4_default_route_only() {
        // `leak_guard` blocks global IPv6 because the adapter has no routable
        // IPv6 address to carry it. If an IPv6 gateway is ever added, `::/0`
        // belongs here and that block has to be revisited with it.
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .build();
        let settings = &json["inbounds"][0]["settings"];
        let table = settings["autoSystemRoutingTable"].as_array().unwrap();
        assert_eq!(table, &vec![json!("0.0.0.0/0")]);
        let gateway = settings["gateway"].as_array().unwrap();
        assert!(gateway
            .iter()
            .all(|entry| !entry.as_str().unwrap().contains(':')));
    }

    #[test]
    #[cfg(not(target_os = "ios"))]
    fn tun_inbound_matches_xray_schema() {
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .build();
        let tun = &json["inbounds"][0];
        assert_eq!(tun["tag"], "tun-in");
        assert_eq!(tun["protocol"], "tun");
        assert_eq!(tun["settings"]["autoOutboundsInterface"], "auto");
        assert!(tun["settings"]["autoSystemRoutingTable"].is_array());
        assert_eq!(tun["sniffing"]["enabled"], true);
        #[cfg(target_os = "macos")]
        {
            assert_eq!(tun["settings"]["name"], "utun10");
            assert_eq!(tun["settings"]["mtu"], 1500);
            assert!(tun["settings"].get("desc").is_none());
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(tun["settings"]["name"], "xraybox-tun");
            assert_eq!(tun["settings"]["mtu"], 9000);
        }
    }

    fn profile_b() -> VlessReality {
        let mut profile = sample_profile();
        profile.server = "other.example.com".into();
        profile.uuid = "22222222-2222-2222-2222-222222222222".into();
        profile
    }

    #[test]
    fn tun_inbound_unchanged_when_only_vless_changes() {
        let first = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .build();
        let second = ConfigBuilder::default()
            .with_proxy(profile_b())
            .with_tun()
            .build();
        assert_eq!(first["inbounds"], second["inbounds"]);
        assert_ne!(first["outbounds"], second["outbounds"]);
        assert_ne!(first["routing"]["rules"], second["routing"]["rules"]);
    }

    #[test]
    fn bootstrap_rules_put_server_before_lan() {
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .with_dns(DnsSettings::default())
            .with_route(RouteSettings::default())
            .build();
        let text = json.to_string();
        assert!(!text.contains("fakeip"));
        assert!(!text.contains("198.18"));
        let rules = json["routing"]["rules"].as_array().unwrap();
        assert_eq!(rules[0]["domain"][0], "full:edge.example.com");
        assert_eq!(rules[0]["outboundTag"], "direct");
        let lan_index = if cfg!(target_os = "ios") { 3 } else { 1 };
        assert_eq!(rules[lan_index]["ip"][0], "geoip:private");
        assert_eq!(rules[lan_index]["outboundTag"], "direct");
        assert_eq!(rules.last().unwrap()["outboundTag"], "proxy");
        assert_eq!(json["routing"]["domainStrategy"], "IPIfNonMatch");
        assert!(json["routing"].get("final").is_none());
        assert_eq!(json["dns"]["queryStrategy"], "UseIPv4");
    }

    #[test]
    fn ios_bootstrap_blocks_lan_dns_and_ipv6_before_lan_bypass() {
        assert!(ios_bootstrap_rules(false).is_empty());
        let mut bootstrap = vec![host_direct_route_rule("edge.example.com")];
        bootstrap.extend(ios_bootstrap_rules(true));
        let routing = compile_route_object(&RouteSettings::default(), &bootstrap);
        let rules = routing["rules"].as_array().unwrap();
        assert_eq!(rules[0]["domain"][0], "full:edge.example.com");
        assert_eq!(rules[0]["outboundTag"], "direct");
        assert_eq!(rules[1]["port"], "53");
        assert_eq!(rules[1]["outboundTag"], "block");
        let dns_ranges = rules[1]["ip"].as_array().unwrap();
        for range in [
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "100.64.0.0/10",
            "169.254.0.0/16",
        ] {
            assert!(dns_ranges.iter().any(|ip| ip == range), "{range}");
        }
        assert_eq!(rules[2]["ip"][0], "::/0");
        assert_eq!(rules[2]["outboundTag"], "block");
        assert_eq!(rules[3]["ip"][0], "geoip:private");
        assert_eq!(rules[3]["outboundTag"], "direct");
    }

    #[test]
    fn lan_dns_and_ipv6_blocks_are_ios_only() {
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .build();
        let rules = json["routing"]["rules"].as_array().unwrap();
        let blocks_ipv6 = rules.iter().any(|rule| {
            rule["ip"]
                .as_array()
                .is_some_and(|ips| ips.iter().any(|ip| ip == "::/0"))
        });
        let blocks_lan_dns = rules.iter().any(|rule| rule.get("port").is_some());
        assert_eq!(blocks_ipv6, cfg!(target_os = "ios"));
        assert_eq!(blocks_lan_dns, cfg!(target_os = "ios"));
    }

    #[test]
    fn server_ip_bypasses_the_tunnel() {
        let mut profile = sample_profile();
        profile.server = "203.0.113.10".into();
        let json = ConfigBuilder::default()
            .with_proxy(profile)
            .with_tun()
            .build();
        let rules = json["routing"]["rules"].as_array().unwrap();
        let bypass = rules
            .iter()
            .find(|rule| rule["outboundTag"] == "direct")
            .expect("server bypass rule");
        assert_eq!(bypass["ip"][0], "203.0.113.10/32");
        assert!(bypass.get("ip_cidr").is_none());
    }

    #[test]
    fn last_rule_sends_everything_to_proxy() {
        let json = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .build();
        let rules = json["routing"]["rules"].as_array().unwrap();
        let last = rules.last().unwrap();
        assert_eq!(last["network"], "tcp,udp");
        assert_eq!(last["outboundTag"], "proxy");
    }

    #[test]
    fn tun_inbound_is_bit_identical_across_builds() {
        let first = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .build();
        let second = ConfigBuilder::default()
            .with_proxy(sample_profile())
            .with_tun()
            .build();
        assert_eq!(first["inbounds"], second["inbounds"]);
    }
}
