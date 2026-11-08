//! `networksetup` argument builders and parsers. The helper runs the commands;
//! these functions stay pure so tests do not need root.

pub fn set_dns_args(service: &str, servers: &[String]) -> Vec<String> {
    let mut args = vec!["-setdnsservers".to_string(), service.to_string()];
    if servers.is_empty() {
        args.push("empty".to_string());
    } else {
        args.extend(servers.iter().cloned());
    }
    args
}

pub fn get_dns_args(service: &str) -> Vec<String> {
    vec!["-getdnsservers".to_string(), service.to_string()]
}

pub fn get_info_args(service: &str) -> Vec<String> {
    vec!["-getinfo".to_string(), service.to_string()]
}

/// How IPv6 was configured on a service before we turned it off for the tunnel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ipv6Config {
    pub mode: String,
    pub address: String,
    pub prefix: String,
    pub router: String,
}

impl Ipv6Config {
    pub fn automatic() -> Self {
        Self {
            mode: "automatic".into(),
            address: String::new(),
            prefix: String::new(),
            router: String::new(),
        }
    }
}

pub fn set_ipv6_off_args(service: &str) -> Vec<String> {
    vec!["-setv6off".to_string(), service.to_string()]
}

pub fn restore_ipv6_args(service: &str, config: &Ipv6Config) -> Vec<String> {
    match config.mode.as_str() {
        "off" => set_ipv6_off_args(service),
        "linklocal" => vec!["-setv6LinkLocal".to_string(), service.to_string()],
        "manual" if !config.address.is_empty() && !config.prefix.is_empty() => vec![
            "-setv6manual".to_string(),
            service.to_string(),
            config.address.clone(),
            config.prefix.clone(),
            config.router.clone(),
        ],
        _ => vec!["-setv6automatic".to_string(), service.to_string()],
    }
}

/// `networksetup -getinfo` reports the mode on the `IPv6:` line.
pub fn parse_ipv6_config(getinfo: &str) -> Ipv6Config {
    let mut mode = String::new();
    let mut address = String::new();
    let mut prefix = String::new();
    let mut router = String::new();
    for line in getinfo.lines() {
        let line = line.trim();
        if let Some(value) = labeled(line, "IPv6:") {
            mode = match value.to_ascii_lowercase().as_str() {
                "off" => "off".into(),
                "manual" => "manual".into(),
                "link-local only" | "linklocal" => "linklocal".into(),
                _ => "automatic".into(),
            };
        } else if let Some(value) = labeled(line, "IPv6 IP address:") {
            if value != "none" {
                address = value.to_string();
            }
        } else if let Some(value) = labeled(line, "IPv6 Prefix Length:") {
            if value != "none" {
                prefix = value.to_string();
            }
        } else if let Some(value) = labeled(line, "IPv6 Router:") {
            if value != "none" {
                router = value.to_string();
            }
        }
    }
    if mode.is_empty() {
        mode = "automatic".into();
    }
    Ipv6Config {
        mode,
        address,
        prefix,
        router,
    }
}

fn labeled<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    line.strip_prefix(label).map(str::trim)
}

/// Empty means "no servers configured" (DHCP). `networksetup` prints a sentence
/// in that case rather than an empty list.
pub fn parse_dns_servers(output: &str) -> Vec<String> {
    if output.to_ascii_lowercase().contains("aren't any dns") {
        return Vec::new();
    }
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("There "))
        .map(str::to_string)
        .collect()
}

pub fn default_route_device(route_output: &str) -> Option<String> {
    for line in route_output.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("interface:") else {
            continue;
        };
        let device = rest.trim();
        if !device.is_empty() {
            return Some(device.to_string());
        }
    }
    None
}

/// `networksetup -listnetworkserviceorder` pairs a service name with a device.
/// Disabled services are marked `(*)` and are skipped.
pub fn service_for_device(order: &str, device: &str) -> Option<String> {
    let mut pending: Option<String> = None;
    for line in order.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("(*)") {
            pending = None;
            continue;
        }
        if line.contains("Device:") {
            if device_token(line) == Some(device) {
                return pending;
            }
            pending = None;
            continue;
        }
        if let Some(name) = service_name(line) {
            pending = Some(name);
        }
    }
    None
}

fn service_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix('(')?;
    let (_, name) = rest.split_once(')')?;
    let name = name.trim();
    if name.is_empty() || name.contains("Hardware Port") {
        return None;
    }
    Some(name.to_string())
}

fn device_token(line: &str) -> Option<&str> {
    let rest = line.split("Device:").nth(1)?.trim();
    let token = rest.trim_end_matches(')').trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_dns_uses_empty_to_restore_dhcp() {
        assert_eq!(
            set_dns_args("Wi-Fi", &[]),
            vec!["-setdnsservers", "Wi-Fi", "empty"]
        );
        assert_eq!(
            set_dns_args("Wi-Fi", &["1.1.1.1".into(), "8.8.8.8".into()]),
            vec!["-setdnsservers", "Wi-Fi", "1.1.1.1", "8.8.8.8"]
        );
    }

    #[test]
    fn parse_dns_treats_the_dhcp_sentence_as_empty() {
        let dhcp = "There aren't any DNS Servers set on Wi-Fi.\n";
        assert!(parse_dns_servers(dhcp).is_empty());
        assert_eq!(
            parse_dns_servers("1.1.1.1\n8.8.8.8\n"),
            vec!["1.1.1.1", "8.8.8.8"]
        );
    }

    #[test]
    fn default_route_device_reads_the_interface_line() {
        let output = "\
   route to: default
destination: default
    gateway: 192.168.1.1
  interface: en0
      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING>
";
        assert_eq!(default_route_device(output).as_deref(), Some("en0"));
    }

    #[test]
    fn service_for_device_skips_disabled_and_matches_the_token() {
        let order = "\
An asterisk (*) denotes that a network service is disabled.
(1) Wi-Fi
(Hardware Port: Wi-Fi, Device: en0)

(*) Thunderbolt Bridge
(Hardware Port: Thunderbolt Bridge, Device: bridge0)

(2) USB 10/100/1000 LAN
(Hardware Port: USB 10/100/1000 LAN, Device: en7)
";
        assert_eq!(service_for_device(order, "en0").as_deref(), Some("Wi-Fi"));
        assert_eq!(
            service_for_device(order, "en7").as_deref(),
            Some("USB 10/100/1000 LAN")
        );
        assert_eq!(service_for_device(order, "bridge0"), None);
        assert_eq!(service_for_device(order, "en"), None);
    }

    #[test]
    fn ipv6_off_then_restores_the_previous_mode() {
        let automatic =
            parse_ipv6_config("IPv6: Automatic\nIPv6 IP address: none\nIPv6 Router: none\n");
        assert_eq!(automatic.mode, "automatic");
        assert_eq!(set_ipv6_off_args("Wi-Fi"), vec!["-setv6off", "Wi-Fi"]);
        assert_eq!(
            restore_ipv6_args("Wi-Fi", &automatic),
            vec!["-setv6automatic", "Wi-Fi"]
        );

        let manual = parse_ipv6_config(
            "IPv6: Manual\nIPv6 IP address: 2001:db8::1\nIPv6 Prefix Length: 64\nIPv6 Router: 2001:db8::ffff\n",
        );
        assert_eq!(
            restore_ipv6_args("Wi-Fi", &manual),
            vec![
                "-setv6manual",
                "Wi-Fi",
                "2001:db8::1",
                "64",
                "2001:db8::ffff"
            ]
        );
        assert_eq!(parse_ipv6_config("IPv6: Off\n").mode, "off");
    }
}
