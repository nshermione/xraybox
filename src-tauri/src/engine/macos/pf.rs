//! Packet filter rules for the macOS leak guard and kill switch.
//!
//! `/etc/pf.conf` already contains `anchor "com.apple/*"`, so rules loaded into
//! the sub-anchor `com.apple/xraybox` are evaluated without editing that file
//! and without replacing Apple's main ruleset. Enable and disable go through
//! `pfctl -E` / `pfctl -X <token>` so we only drop our own reference count.

use std::net::Ipv4Addr;

pub const PF_ANCHOR: &str = "com.apple/xraybox";
pub const TUN_NAME: &str = "utun10";

/// Routes Xray installs for `0.0.0.0/0` on Darwin (more-specific prefixes that
/// beat the existing default route, leaving `0.0.0.0/8` alone). A SIGKILL skips
/// Xray's own cleanup, so the helper deletes these on teardown.
pub const PROTECTED_IPV4_ROUTES: [&str; 8] = [
    "1.0.0.0/8",
    "2.0.0.0/7",
    "4.0.0.0/6",
    "8.0.0.0/5",
    "16.0.0.0/4",
    "32.0.0.0/3",
    "64.0.0.0/2",
    "128.0.0.0/1",
];

pub const OFF_TUNNEL_DNS: &str =
    "{ 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 100.64.0.0/10, 169.254.0.0/16 }";

/// `quick` makes the first match win, so passes sit above the blocks.
pub fn pf_rules(kill_switch: bool, server_ips: &[String], server_port: u16) -> String {
    let mut lines = Vec::new();
    lines.push("pass out quick on lo0 all".to_string());
    // 127.0.0.1 is not always classified as lo0. Without this, kill switch's
    // final `block drop out all` drops `xray api` dials to 127.0.0.1:9090.
    lines.push("pass out quick inet from any to 127.0.0.0/8".to_string());
    lines.push("pass in quick inet from 127.0.0.0/8 to any".to_string());
    lines.push(format!("pass out quick on {TUN_NAME} all"));
    lines.push("pass out quick inet proto udp from any port 68 to any port 67".to_string());
    lines.push("pass out quick inet proto udp from any to any port 67".to_string());
    if kill_switch {
        for ip in server_ips {
            if is_plain_ipv4(ip) && server_port > 0 {
                lines.push(format!(
                    "pass out quick inet proto tcp from any to {ip} port {server_port}"
                ));
                lines.push(format!(
                    "pass out quick inet proto udp from any to {ip} port {server_port}"
                ));
            }
        }
    }
    // `return`, not `drop`: a silent drop makes the stack wait out the full
    // connect timeout on IPv6 before it tries the IPv4 address that actually
    // enters the TUN. RST / ICMP unreachable fails that attempt immediately.
    lines.push("block return out quick inet6 from any to 2000::/3".to_string());
    lines.push(format!(
        "block drop out quick proto {{ tcp udp }} from any to {OFF_TUNNEL_DNS} port 53"
    ));
    if kill_switch {
        lines.push("block drop out quick all".to_string());
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn is_plain_ipv4(value: &str) -> bool {
    let Ok(addr) = value.parse::<Ipv4Addr>() else {
        return false;
    };
    value == addr.to_string()
}

pub fn parse_pf_token(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        let Some(rest) = line
            .strip_prefix("Token :")
            .or_else(|| line.strip_prefix("Token:"))
        else {
            continue;
        };
        let token = rest.trim();
        if !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()) {
            return Some(token.to_string());
        }
    }
    None
}

pub fn route_delete_args(prefix: &str, gateway: &str) -> Vec<String> {
    vec![
        "-n".into(),
        "delete".into(),
        "-net".into(),
        prefix.into(),
        gateway.into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leak_guard_blocks_global_ipv6_and_lan_dns_without_a_blanket_block() {
        let rules = pf_rules(false, &["1.2.3.4".into()], 443);
        assert!(rules.contains("block return out quick inet6 from any to 2000::/3"));
        assert!(rules.contains("port 53"));
        assert!(rules.contains("10.0.0.0/8"));
        assert!(rules.contains("192.168.0.0/16"));
        assert!(!rules.contains("block drop out quick all"));
        assert!(!rules.contains("1.2.3.4"));
        assert!(rules.contains("pass out quick on lo0 all"));
        assert!(rules.contains("pass out quick inet from any to 127.0.0.0/8"));
        assert!(rules.contains("pass out quick on utun10 all"));
    }

    #[test]
    fn kill_switch_allows_the_server_then_blocks_everything_else() {
        let rules = pf_rules(true, &["203.0.113.10".into()], 443);
        let server = rules.find("203.0.113.10").unwrap();
        let block = rules.find("block drop out quick all").unwrap();
        assert!(server < block);
        assert!(rules.contains("pass out quick inet proto tcp from any to 203.0.113.10 port 443"));
    }

    #[test]
    fn kill_switch_rejects_injected_addresses() {
        let rules = pf_rules(true, &["1.2.3.4; pass all".into(), "not-an-ip".into()], 443);
        assert!(!rules.contains("pass all"));
        assert!(!rules.contains("not-an-ip"));
        assert!(rules.contains("block drop out quick all"));
    }

    #[test]
    fn plain_ipv4_rejects_leading_zeros_and_extra_text() {
        assert!(is_plain_ipv4("1.2.3.4"));
        assert!(!is_plain_ipv4("01.2.3.4"));
        assert!(!is_plain_ipv4("1.2.3.4/32"));
        assert!(!is_plain_ipv4("::1"));
    }

    #[test]
    fn token_parser_reads_pfctl_stderr() {
        let output = "No ALTQ support in kernel\npf enabled\nToken : 1234567890\n";
        assert_eq!(parse_pf_token(output).as_deref(), Some("1234567890"));
        assert_eq!(parse_pf_token("pf enabled\n"), None);
    }

    #[test]
    fn protected_routes_cover_the_darwin_default_split() {
        assert_eq!(PROTECTED_IPV4_ROUTES[0], "1.0.0.0/8");
        assert_eq!(PROTECTED_IPV4_ROUTES[7], "128.0.0.0/1");
        assert_eq!(PROTECTED_IPV4_ROUTES.len(), 8);
        let args = route_delete_args(PROTECTED_IPV4_ROUTES[0], "172.19.0.1");
        assert_eq!(args[0], "-n");
        assert!(args.contains(&"delete".to_string()));
        assert!(args.contains(&"1.0.0.0/8".to_string()));
    }
}
