use super::VlessReality;
use crate::engine::error::EngineError;

#[derive(Debug, PartialEq, Eq)]
pub enum ShareError {
    UnsupportedProtocol,
    Invalid,
}

impl ShareError {
    fn into_engine(self) -> EngineError {
        match self {
            ShareError::UnsupportedProtocol => {
                EngineError::Config("unsupported share link protocol".into())
            }
            ShareError::Invalid => EngineError::Config("invalid share link".into()),
        }
    }
}

pub fn parse_vless_share_link(uri: &str) -> Result<VlessReality, ShareError> {
    let uri = uri.trim();
    let Some((scheme, rest)) = uri.split_once("://") else {
        return Err(ShareError::Invalid);
    };
    if !scheme.eq_ignore_ascii_case("vless") {
        return Err(ShareError::UnsupportedProtocol);
    }

    let (body, fragment) = match rest.split_once('#') {
        Some((body, fragment)) => (body, Some(fragment)),
        None => (rest, None),
    };
    let (authority, query) = match body.split_once('?') {
        Some((authority, query)) => (authority, query),
        None => (body, ""),
    };
    let (user, host_port) = authority.split_once('@').ok_or(ShareError::Invalid)?;
    let uuid = percent_decode(user);
    let (server, port) = split_host_port(host_port).ok_or(ShareError::Invalid)?;
    let params = parse_query(query);

    let security = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("security"))
        .map(|(_, value)| value.as_str())
        .unwrap_or("");
    if !security.eq_ignore_ascii_case("reality") {
        return Err(ShareError::Invalid);
    }

    let network = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("type") || key.eq_ignore_ascii_case("network"))
        .map(|(_, value)| value.as_str())
        .unwrap_or("tcp");
    if !matches!(
        network.to_ascii_lowercase().as_str(),
        "tcp" | "raw" | "none" | ""
    ) {
        return Err(ShareError::Invalid);
    }

    let public_key = required_param(&params, &["pbk", "publickey"])?;
    let server_name = optional_param(&params, &["sni", "servername"])
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| server.clone());
    let short_id = optional_param(&params, &["sid", "shortid"]).unwrap_or_default();
    let flow = optional_param(&params, &["flow"]).filter(|value| !value.is_empty());
    let fingerprint =
        optional_param(&params, &["fp", "fingerprint"]).unwrap_or_else(|| "chrome".into());
    let spider_x = optional_param(&params, &["spx", "spiderx"]).filter(|value| !value.is_empty());
    let mldsa65_verify =
        optional_param(&params, &["pqv", "mldsa65verify"]).filter(|value| !value.is_empty());
    let remark = fragment
        .map(percent_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| server.clone());

    let profile = VlessReality {
        remark,
        server,
        server_port: port,
        uuid,
        server_name,
        public_key,
        short_id,
        flow,
        fingerprint,
        spider_x,
        mldsa65_verify,
    };
    profile.validate().map_err(|_| ShareError::Invalid)?;
    Ok(profile)
}

pub fn parse_share_link(uri: &str) -> Result<VlessReality, ShareError> {
    parse_vless_share_link(uri)
}

pub fn profile_id(profile: &VlessReality) -> String {
    format!(
        "{}@{}:{}",
        profile.uuid.to_ascii_lowercase(),
        profile.server.to_ascii_lowercase(),
        profile.server_port
    )
}

#[allow(dead_code)]
pub fn parse_vless_share_link_engine(uri: &str) -> Result<VlessReality, EngineError> {
    parse_vless_share_link(uri).map_err(ShareError::into_engine)
}

fn required_param(params: &[(String, String)], keys: &[&str]) -> Result<String, ShareError> {
    optional_param(params, keys)
        .filter(|value| !value.is_empty())
        .ok_or(ShareError::Invalid)
}

fn optional_param(params: &[(String, String)], keys: &[&str]) -> Option<String> {
    params.iter().find_map(|(key, value)| {
        if keys.iter().any(|wanted| key.eq_ignore_ascii_case(wanted)) {
            Some(value.clone())
        } else {
            None
        }
    })
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    if query.is_empty() {
        return Vec::new();
    }
    query
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

fn split_host_port(value: &str) -> Option<(String, u16)> {
    if let Some(rest) = value.strip_prefix('[') {
        let (host, port_part) = rest.split_once("]:")?;
        let port = port_part.parse().ok()?;
        return Some((host.to_string(), port));
    }
    if let Some((host, port_part)) = value.rsplit_once(':') {
        if !host.is_empty() {
            let port = port_part.parse().ok()?;
            return Some((host.to_string(), port));
        }
    }
    if value.is_empty() {
        return None;
    }
    Some((value.to_string(), 443))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_nibble(bytes[index + 1]), hex_nibble(bytes[index + 2]))
            {
                out.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        if bytes[index] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[index]);
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_uri() -> String {
        format!(
            "vless://11111111-1111-1111-1111-111111111111@edge.example.com:443?encryption=none&flow=xtls-rprx-vision&security=reality&sni=www.cloudflare.com&fp=chrome&pbk=abc&sid=dead&spx=%2F26306e280ce9053&pqv=BASE64KEY#Japan%20Tokyo"
        )
    }

    #[test]
    fn parses_vless_reality_share_link() {
        let profile = parse_vless_share_link(&sample_uri()).unwrap();
        assert_eq!(profile.server, "edge.example.com");
        assert_eq!(profile.server_port, 443);
        assert_eq!(profile.uuid, "11111111-1111-1111-1111-111111111111");
        assert_eq!(profile.server_name, "www.cloudflare.com");
        assert_eq!(profile.public_key, "abc");
        assert_eq!(profile.short_id, "dead");
        assert_eq!(profile.flow.as_deref(), Some("xtls-rprx-vision"));
        assert_eq!(profile.fingerprint, "chrome");
        assert_eq!(profile.spider_x.as_deref(), Some("/26306e280ce9053"));
        assert_eq!(profile.mldsa65_verify.as_deref(), Some("BASE64KEY"));
        assert_eq!(profile.remark, "Japan Tokyo");
    }

    #[test]
    fn rejects_non_reality_vless() {
        let uri = "vless://11111111-1111-1111-1111-111111111111@edge.example.com:443?security=tls&pbk=abc&sni=www.cloudflare.com";
        assert_eq!(parse_vless_share_link(uri), Err(ShareError::Invalid));
    }

    #[test]
    fn rejects_vmess_as_unsupported() {
        assert_eq!(
            parse_share_link("vmess://eyJ2IjoiMiJ9"),
            Err(ShareError::UnsupportedProtocol)
        );
    }

    #[test]
    fn rejects_ws_transport() {
        let uri = "vless://11111111-1111-1111-1111-111111111111@edge.example.com:443?security=reality&pbk=abc&sni=www.cloudflare.com&type=ws";
        assert_eq!(parse_vless_share_link(uri), Err(ShareError::Invalid));
    }

    #[test]
    fn parses_ipv6_host() {
        let uri = "vless://11111111-1111-1111-1111-111111111111@[2001:db8::1]:8443?security=reality&pbk=abc&sni=www.cloudflare.com";
        let profile = parse_vless_share_link(uri).unwrap();
        assert_eq!(profile.server, "2001:db8::1");
        assert_eq!(profile.server_port, 8443);
    }

    #[test]
    fn defaults_port_and_sni_to_host() {
        let uri = "vless://11111111-1111-1111-1111-111111111111@edge.example.com?security=reality&pbk=abc";
        let profile = parse_vless_share_link(uri).unwrap();
        assert_eq!(profile.server_port, 443);
        assert_eq!(profile.server_name, "edge.example.com");
    }

    #[test]
    fn profile_id_is_stable_and_lowercased() {
        let profile = parse_vless_share_link(&sample_uri()).unwrap();
        assert_eq!(
            profile_id(&profile),
            "11111111-1111-1111-1111-111111111111@edge.example.com:443"
        );
    }
}
