#![cfg_attr(not(windows), allow(dead_code))]

//! Windows Firewall rules that close the two paths traffic can take around an
//! IPv4-only TUN.
//!
//! Xray has no `strict_route` equivalent, so the tunnel is a routing-table
//! arrangement rather than an enforced boundary. Two destinations are reachable
//! without ever crossing the TUN default route:
//!
//! 1. **Global IPv6.** The TUN carries `0.0.0.0/0` only, so every IPv6
//!    destination stays on the physical NIC. Windows prefers IPv6 over IPv4
//!    (RFC 6724), so on a dual-stack network the real address is what the
//!    remote host sees while the UI reports Connected.
//! 2. **Resolvers on the local network.** A router used as the DNS server is
//!    on-link for the physical NIC, and the on-link route wins over the TUN
//!    default route. Setting `dns` on the TUN adapter points the resolver at a
//!    public server instead, but Windows smart multi-homed name resolution can
//!    still fan a query out over every interface.
//!
//! These rules are the enforcement half; `engine::config` owns the routing and
//! resolver half. Both are needed: routing alone leaks, and firewall rules
//! alone would break name resolution.

use crate::engine::error::EngineError;
use crate::engine::firewall;

pub const RULE_IPV6: &str = "Xraybox Leak Guard IPv6";
pub const RULE_DNS_UDP: &str = "Xraybox Leak Guard Off-Tunnel DNS UDP";
pub const RULE_DNS_TCP: &str = "Xraybox Leak Guard Off-Tunnel DNS TCP";

/// Currently allocated global unicast IPv6, and nothing else. Blocking `::/0`
/// would swallow link-local (`fe80::/10`), unique-local (`fc00::/7`) and
/// multicast (`ff00::/8`) as well, and Windows Firewall gives block rules
/// precedence over allow rules — so an allow-back rule could not undo it.
/// Neighbour discovery, DHCPv6 and mDNS all live outside `2000::/3` and keep
/// working.
pub const IPV6_GLOBAL_UNICAST: &str = "2000::/3";

/// Ranges a resolver can sit in and still be reachable without crossing the TUN
/// default route. A public resolver is not listed: `0.0.0.0/0` sends it into the
/// tunnel already.
pub const OFF_TUNNEL_DNS_RANGES: [&str; 5] = [
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "100.64.0.0/10",
    "169.254.0.0/16",
];

pub const DNS_PORT: &str = "53";

const RULE_NAMES: [&str; 3] = [RULE_IPV6, RULE_DNS_UDP, RULE_DNS_TCP];

pub fn off_tunnel_dns_ranges() -> String {
    OFF_TUNNEL_DNS_RANGES.join(",")
}

pub fn block_ipv6_args() -> Vec<String> {
    firewall::add_rule_args(
        RULE_IPV6,
        &[
            ("dir", "out"),
            ("action", "block"),
            ("protocol", "any"),
            ("remoteip", IPV6_GLOBAL_UNICAST),
        ],
    )
}

pub fn block_dns_args(name: &str, protocol: &str) -> Vec<String> {
    firewall::add_rule_args(
        name,
        &[
            ("dir", "out"),
            ("action", "block"),
            ("protocol", protocol),
            ("remoteport", DNS_PORT),
            ("remoteip", &off_tunnel_dns_ranges()),
        ],
    )
}

/// Applied before the sidecar spawns, so there is no window in which the TUN is
/// up but the guard is not.
pub fn apply() -> Result<(), EngineError> {
    #[cfg(windows)]
    {
        release();
        firewall::run(&block_ipv6_args())?;
        firewall::run(&block_dns_args(RULE_DNS_UDP, "UDP"))?;
        if let Err(err) = firewall::run(&block_dns_args(RULE_DNS_TCP, "TCP")) {
            release();
            return Err(err);
        }
        Ok(())
    }
    #[cfg(not(windows))]
    Ok(())
}

pub fn release() {
    #[cfg(windows)]
    for name in RULE_NAMES {
        firewall::delete_rule_best_effort(name);
    }
}

/// A hard kill leaves the rules behind, and firewall rules survive a reboot.
/// Releasing at startup is safe because there is no tunnel yet, so nothing can
/// leak differently — unlike the kill switch, whose whole point is to stay
/// closed until the app reconnects.
pub fn release_leftover() {
    release();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv6_rule_blocks_global_unicast_outbound_only() {
        let args = block_ipv6_args();
        assert!(args.contains(&format!("name={RULE_IPV6}")));
        assert!(args.contains(&"dir=out".to_string()));
        assert!(args.contains(&"action=block".to_string()));
        assert!(args.contains(&"remoteip=2000::/3".to_string()));
        assert!(args.contains(&"enable=yes".to_string()));
    }

    #[test]
    fn ipv6_rule_spares_link_local_ula_and_multicast() {
        // fe80::/10, fc00::/7 and ff00::/8 must not fall inside the blocked
        // prefix, or NDP, DHCPv6 and mDNS break with it.
        assert_eq!(IPV6_GLOBAL_UNICAST, "2000::/3");
        for spared in ["fe80::", "fc00::", "ff02::1"] {
            let first_byte = u128::from(spared.parse::<std::net::Ipv6Addr>().unwrap()) >> 125;
            assert_ne!(first_byte, 1, "{spared} would be caught by 2000::/3");
        }
        let blocked = u128::from(
            "2606:4700:4700::1111"
                .parse::<std::net::Ipv6Addr>()
                .unwrap(),
        ) >> 125;
        assert_eq!(blocked, 1);
    }

    #[test]
    fn dns_rules_cover_both_protocols_on_port_53() {
        let udp = block_dns_args(RULE_DNS_UDP, "UDP");
        let tcp = block_dns_args(RULE_DNS_TCP, "TCP");
        assert!(udp.contains(&"protocol=UDP".to_string()));
        assert!(tcp.contains(&"protocol=TCP".to_string()));
        for args in [&udp, &tcp] {
            assert!(args.contains(&"dir=out".to_string()));
            assert!(args.contains(&"action=block".to_string()));
            assert!(args.contains(&"remoteport=53".to_string()));
        }
        assert_ne!(udp[4], tcp[4], "both protocols need distinct rule names");
    }

    #[test]
    fn dns_rule_covers_rfc1918_cgnat_and_link_local() {
        let ranges = off_tunnel_dns_ranges();
        for range in [
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "100.64.0.0/10",
            "169.254.0.0/16",
        ] {
            assert!(ranges.contains(range), "{range} missing from {ranges}");
        }
    }

    #[test]
    fn dns_rule_leaves_public_resolvers_alone() {
        // A public resolver follows 0.0.0.0/0 into the TUN, so blocking it would
        // only break the resolution the guard is meant to protect.
        let ranges = off_tunnel_dns_ranges();
        assert!(!ranges.contains("1.1.1.1"));
        assert!(!ranges.contains("8.8.8.8"));
    }

    #[test]
    fn every_rule_this_module_adds_is_also_released() {
        let added: Vec<String> = [
            block_ipv6_args(),
            block_dns_args(RULE_DNS_UDP, "UDP"),
            block_dns_args(RULE_DNS_TCP, "TCP"),
        ]
        .iter()
        .map(|args| args[4].clone())
        .collect();
        for name in RULE_NAMES {
            assert!(
                added.contains(&format!("name={name}")),
                "{name} never added"
            );
        }
        assert_eq!(added.len(), RULE_NAMES.len());
    }
}
