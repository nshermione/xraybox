use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};

use super::subscription::{latest_latency, ProfileNode};
use crate::engine::windows::{physical_ipv4, tun_interface_present};

pub const PING_HISTORY: usize = 5;
pub const PING_TIMEOUT: Duration = Duration::from_secs(3);
const PING_CONCURRENCY: usize = 8;
const IMPLAUSIBLE_TUN_RTT_MS: u32 = 2;

pub fn push_sample(samples: &mut Vec<Option<u32>>, sample: Option<u32>) {
    samples.push(sample);
    if samples.len() > PING_HISTORY {
        let overflow = samples.len() - PING_HISTORY;
        samples.drain(0..overflow);
    }
}

pub fn smart_score(samples: &[Option<u32>]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let successes: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.map(f64::from))
        .collect();
    if successes.is_empty() {
        return Some(f64::INFINITY);
    }
    let mean = successes.iter().sum::<f64>() / successes.len() as f64;
    let variance = successes
        .iter()
        .map(|value| {
            let delta = value - mean;
            delta * delta
        })
        .sum::<f64>()
        / successes.len() as f64;
    let stddev = variance.sqrt();
    let fail_rate = (samples.len() - successes.len()) as f64 / samples.len() as f64;
    Some(mean + 0.5 * stddev + 200.0 * fail_rate)
}

pub fn pick_best_node<'a>(nodes: &'a [ProfileNode]) -> Option<&'a ProfileNode> {
    let mut best: Option<(&ProfileNode, f64)> = None;
    for node in nodes {
        let Some(score) = smart_score(&node.ping_ms) else {
            continue;
        };
        match best {
            Some((_, best_score)) if score >= best_score => {}
            _ => best = Some((node, score)),
        }
    }
    best.map(|(node, _)| node)
}

/// Probes bind the physical address on purpose, so the RTT is the real path and
/// not the ~0 ms local TUN handshake. With the kill switch engaged and the
/// tunnel up they cannot leave at all: `BlockOutbound` allows only `xray.exe`
/// and the TUN subnet.
///
/// Skipping is not a privacy choice — probing directly is accepted, see
/// docs/testing/leak-checks.md. It is the only way to avoid recording a
/// firewall drop as a timeout, which fills every history with `None` and makes
/// `smart_score` return infinity for every node at the next connect.
pub fn probes_are_deliverable(tunnel_running: bool, kill_switch: bool) -> bool {
    !(tunnel_running && kill_switch)
}

pub fn ping_host(host: &str, port: u16) -> Option<u32> {
    ping_host_with(host, port, physical_ipv4(), tun_interface_present())
}

fn ping_host_with(host: &str, port: u16, source: Option<Ipv4Addr>, tun_up: bool) -> Option<u32> {
    let target = format!("{host}:{port}");
    let mut addrs = match target.to_socket_addrs() {
        Ok(addrs) => addrs.collect::<Vec<SocketAddr>>(),
        Err(_) => return None,
    };
    if addrs.is_empty() {
        return None;
    }
    addrs.sort_by_key(|addr| addr.is_ipv6());
    let started = Instant::now();
    for addr in addrs {
        if connect_direct(addr, source).is_err() {
            continue;
        }
        let ms = elapsed_ms(started);
        if should_discard_tun_rtt(ms, source.is_some(), tun_up) {
            continue;
        }
        return Some(ms);
    }
    None
}

fn connect_direct(dest: SocketAddr, source: Option<Ipv4Addr>) -> std::io::Result<()> {
    let socket = Socket::new(Domain::for_address(dest), Type::STREAM, Some(Protocol::TCP))?;
    if let (Some(ip), SocketAddr::V4(_)) = (source, dest) {
        socket.bind(&SocketAddr::from((ip, 0)).into())?;
    }
    socket.connect_timeout(&dest.into(), PING_TIMEOUT)?;
    Ok(())
}

fn elapsed_ms(started: Instant) -> u32 {
    started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32
}

fn should_discard_tun_rtt(ms: u32, bound_to_physical: bool, tun_up: bool) -> bool {
    !bound_to_physical && tun_up && ms < IMPLAUSIBLE_TUN_RTT_MS
}

pub fn ping_nodes(nodes: &mut [ProfileNode]) {
    if nodes.is_empty() {
        return;
    }
    let (tx, rx) = mpsc::channel();
    let mut pending = 0usize;
    let mut next = 0usize;

    while pending < PING_CONCURRENCY && next < nodes.len() {
        spawn_ping(next, &nodes[next], tx.clone());
        pending += 1;
        next += 1;
    }

    while pending > 0 {
        let (index, sample) = rx.recv().expect("ping worker");
        push_sample(&mut nodes[index].ping_ms, sample);
        pending -= 1;
        if next < nodes.len() {
            spawn_ping(next, &nodes[next], tx.clone());
            pending += 1;
            next += 1;
        }
    }
}

fn spawn_ping(index: usize, node: &ProfileNode, tx: mpsc::Sender<(usize, Option<u32>)>) {
    let host = node.profile.server.clone();
    let port = node.profile.server_port;
    thread::spawn(move || {
        let _ = tx.send((index, ping_host(&host, port)));
    });
}

#[allow(dead_code)]
pub fn displayed_latency(node: &ProfileNode) -> Option<u32> {
    latest_latency(&node.ping_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::vless::VlessReality;

    fn node_with(id: &str, samples: Vec<Option<u32>>) -> ProfileNode {
        ProfileNode {
            id: id.into(),
            profile: VlessReality {
                remark: id.into(),
                server: "127.0.0.1".into(),
                server_port: 9,
                uuid: "11111111-1111-1111-1111-111111111111".into(),
                server_name: "www.cloudflare.com".into(),
                public_key: "abc".into(),
                short_id: "dead".into(),
                flow: None,
                fingerprint: "chrome".into(),
                spider_x: None,
                mldsa65_verify: None,
            },
            country_code: "xx".into(),
            ping_ms: samples,
        }
    }

    #[test]
    fn keeps_only_last_five_samples() {
        let mut samples = vec![Some(10), Some(20), Some(30), Some(40), Some(50)];
        push_sample(&mut samples, Some(60));
        assert_eq!(
            samples,
            vec![Some(20), Some(30), Some(40), Some(50), Some(60)]
        );
    }

    #[test]
    fn prefers_low_mean_and_stable_history() {
        let stable = smart_score(&[Some(40), Some(42), Some(41), Some(43), Some(40)]).unwrap();
        let jittery = smart_score(&[Some(20), Some(120), Some(30), Some(140), Some(25)]).unwrap();
        let flaky = smart_score(&[Some(30), None, Some(32), None, Some(31)]).unwrap();
        assert!(stable < jittery);
        assert!(stable < flaky);
    }

    #[test]
    fn pick_best_skips_unpinged_and_prefers_lowest_score() {
        let nodes = vec![
            node_with("empty", Vec::new()),
            node_with("slow", vec![Some(200), Some(210)]),
            node_with("fast", vec![Some(40), Some(42)]),
        ];
        assert_eq!(
            pick_best_node(&nodes).map(|node| node.id.as_str()),
            Some("fast")
        );
    }

    #[test]
    fn all_timeouts_score_infinite() {
        let score = smart_score(&[None, None]).unwrap();
        assert!(score.is_infinite());
    }

    #[test]
    fn discards_local_tun_handshake_when_unbound() {
        assert!(should_discard_tun_rtt(0, false, true));
        assert!(should_discard_tun_rtt(1, false, true));
        assert!(!should_discard_tun_rtt(40, false, true));
        assert!(!should_discard_tun_rtt(0, true, true));
        assert!(!should_discard_tun_rtt(0, false, false));
    }

    #[test]
    fn probes_are_deliverable_while_disconnected() {
        assert!(probes_are_deliverable(false, false));
        assert!(probes_are_deliverable(false, true));
    }

    #[test]
    fn probes_are_deliverable_when_the_kill_switch_is_off() {
        assert!(probes_are_deliverable(true, false));
    }

    #[test]
    fn probes_are_blocked_by_the_kill_switch_while_connected() {
        assert!(!probes_are_deliverable(true, true));
    }

    #[test]
    fn a_skipped_refresh_keeps_every_sample() {
        let original = vec![
            node_with("a", vec![Some(40), Some(42)]),
            node_with("b", vec![Some(80), None, Some(90)]),
        ];
        let mut nodes = original.clone();
        if probes_are_deliverable(true, true) {
            ping_nodes(&mut nodes);
        }
        assert_eq!(nodes[0].ping_ms, original[0].ping_ms);
        assert_eq!(nodes[1].ping_ms, original[1].ping_ms);
        assert_eq!(latest_latency(&nodes[0].ping_ms), Some(42));
        assert_eq!(latest_latency(&nodes[1].ping_ms), Some(90));
    }
}
