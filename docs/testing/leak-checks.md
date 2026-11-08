# Leak checks — Windows

What "connected" has to mean, how it is enforced, and how to prove it on a real
machine. Read this before touching `engine::config`, `engine::leak_guard`,
`engine::kill_switch`, or anything that decides which outbound a packet takes.

## The property under test

While the engine reports `Running`, no packet may reach a destination outside the
local network carrying the machine's real source address, and no name lookup may
reach a resolver outside the tunnel.

Xray has no `strict_route` equivalent, so this is **not** enforced by the tunnel
itself. The tunnel is a routing-table arrangement, and three things can step
around a routing table:

| Escape route | Why it exists | What closes it |
| --- | --- | --- |
| Global IPv6 | The TUN adapter has an IPv4 address and claims `0.0.0.0/0` only, so every IPv6 destination stays on the physical NIC. Windows prefers IPv6 (RFC 6724), so on a dual-stack network it is the *default* path, not an edge case. | `leak_guard` blocks `2000::/3` outbound |
| A resolver on the local network | A router used as the DNS server is on-link for the physical NIC, and an on-link route beats the TUN default route. | `settings.dns` on the TUN inbound moves the resolver to a public address; `leak_guard` blocks port 53 to private ranges as the backstop |
| A process that binds an interface or hardcodes a resolver | Nothing in a routing table can stop it. | Only the kill switch (`BlockOutbound` + an allow rule for `xray.exe` and the TUN subnet) |

## Layers, and why both halves are needed

`engine::config` owns routing and resolvers. `engine::leak_guard` owns
enforcement. Neither is sufficient alone:

- Routing alone leaks. That was the state before this document existed: the
  config was correct about everything it controlled, and IPv6 plus LAN DNS went
  around it untouched.
- Firewall rules alone break the product. Blocking port 53 to the LAN with no
  resolver on the TUN adapter means no name resolution at all.

So a change to one half is a change to both. The unit test
`config::tests::tun_claims_the_ipv4_default_route_only` exists to make that
coupling fail loudly: if someone adds an IPv6 gateway and `::/0`, that test
breaks and points at `leak_guard`.

## What the unit tests can and cannot prove

`cargo test -p xraybox --lib` covers:

- `dns::tests::tun_dns_servers_*` — only IPv4 literals reach Windows `SetDNS`;
  hostnames and IPv6 entries are dropped rather than passed through.
- `engine::config::tests::tun_inbound_*` — the TUN inbound carries the remote
  resolvers, omits `dns` when no IPv4 literal is configured, and claims the IPv4
  default route only.
- `engine::leak_guard::tests::*` — the blocked IPv6 prefix is `2000::/3` and not
  `::/0` (Windows Firewall gives block rules precedence, so a `::/0` block could
  not be undone by an allow-back rule for link-local); the DNS rules cover both
  protocols on port 53 across RFC1918, CGNAT and link-local; every rule added is
  also released.
- `engine::firewall::tests::*` — verbatim path prefixes are stripped, since
  `netsh` rejects `\\?\`.

None of that proves Windows honours the config. Every claim about runtime
behaviour comes from `scripts/leak-test.ps1`.

### Two cheap probes that need no admin and no server

**Field names: hand the field a wrong type.** `xray run -test` exiting 0 proves
nothing about a field name, because Go's decoder drops unknown keys — verified,
not assumed:

```
{"name":"xraybox-tun","thisFieldDoesNotExist":["x"]}   ->  Configuration OK.
{"name":"xraybox-tun","dns":12345}                     ->  json: cannot unmarshal
    number into Go struct field TunConfig.dns of type []string
```

The type error names the struct field, which is what confirms `dns` exists on
`TunConfig` and takes a string list. Both cases are pinned as ignored tests:

```powershell
cargo test -p xraybox --lib -- --ignored tun_dns_field_is_named_dns unknown_tun_setting
```

Run them after every sidecar version bump. A rename upstream would otherwise
reintroduce the DNS leak with no other symptom — the config would still validate,
the app would still connect, and the adapter would silently have no resolver.

The same probe showed Xray does **not** validate the address values in `dns`
(`"dns":["not an ip"]` is accepted and only fails later inside `SetDNS`). That is
why `dns::tun_dns_servers` filters to IPv4 literals in Rust instead of trusting
the sidecar to complain.

**Firewall rule shapes: `netsh` validates syntax before elevation.** An
unelevated `netsh advfirewall firewall add rule ...` that answers
`The requested operation requires elevation` has accepted the arguments; one that
answers `'x' is not a valid argument` or prints the usage block has not. Use it to
check a new rule shape before wiring it up.

## Runtime checks

```powershell
# 1. Disconnected, to capture the baseline egress address.
.\scripts\leak-test.ps1

# 2. Connect from the app, then rerun with the baseline it printed.
.\scripts\leak-test.ps1 -BaselineIp 203.0.113.7

# 3. Optional, destructive: kills xray.exe to exercise the fail-closed path.
.\scripts\leak-test.ps1 -BaselineIp 203.0.113.7 -IncludeCrashTest
```

The script is read-only except for `-IncludeCrashTest`. Exit code 0 means no
check reported `FAIL`; `SKIP` means the check needs the other connection state.

What each check is really asking:

| Check | Fails when |
| --- | --- |
| TUN adapter has DNS servers | `settings.dns` was dropped from the TUN inbound, so the OS resolves through the physical NIC |
| Off-tunnel resolvers are blocked | a LAN resolver still answers a direct query while connected |
| Global IPv6 cannot leave | a native IPv6 connection to `2606:4700:4700::1111` succeeds while connected |
| Leak guard firewall rules installed / released | the rules are missing while connected, or left behind while disconnected |
| Egress differs from the disconnected baseline | the tunnel is up but traffic is not using it |
| TUN owns an IPv4 default route | `autoSystemRoutingTable` did not take effect |
| Leak guard survives a sidecar crash | the guard was released on crash, reopening IPv6 and LAN DNS |
| Egress after crash | traffic fell back to the physical NIC — expected unless the kill switch is on |

### Test on Ethernet, not only Wi-Fi

The DNS behaviour depends on what the physical adapter's resolver is:

- resolver is a **public** address (for example Wi-Fi set to `8.8.8.8`) — the
  query follows `0.0.0.0/0` into the tunnel and nothing leaks, guard or no guard.
- resolver is a **LAN** address (a router, the common default) — the query takes
  the on-link route and leaks.

Verifying on a Wi-Fi network with public DNS and concluding "no leak" is the
trap. Reproduce on the adapter whose resolver is private, or temporarily point
the test adapter at its gateway.

### IPv6 needs a dual-stack network

If `leak-test.ps1` reports *no global IPv6 address and no ::/0 route*, the IPv6
check has proved nothing about the guard — the network simply has no IPv6. To
exercise it, use a mobile hotspot or an ISP link with IPv6 enabled; most
Vietnamese FTTH providers enable it by default.

## Known trade-offs, deliberately accepted

- **LAN resolvers stop working while connected.** Pi-hole, a corporate AD
  resolver, or `\\hostname` lookups that depend on the router will fail. That is
  the guard doing its job; there is no `netsh` filter for "port 53 unless the
  source is the TUN". Disable by deleting
  `Xraybox Leak Guard Off-Tunnel DNS UDP` / `... TCP`.
- **IPv6 is blocked, not tunnelled.** Tunnelling it would need a routable IPv6
  address in `gateway`, `::/0` in `autoSystemRoutingTable`, and
  `queryStrategy: "UseIP"` — and it only helps if the VLESS server has IPv6
  egress. Without that, IPv6 would enter the tunnel and time out, which is worse
  than blocking it.
- **Firewall rules survive a reboot.** A hard kill leaves them behind, so
  `lib.rs` releases the leak guard unconditionally at startup. The kill switch is
  released only when `autoConnect` is off, because staying closed until the app
  reconnects is the point of it.
- **Node latency is probed on the physical path.** `profile::ping::ping_host`
  binds the physical address on purpose, so the RTT is the real path and not the
  ~0 ms local TUN handshake. Measuring through the proxy would report
  client→server→node, which is useless for choosing a node. The probe talks
  only to servers the user already configured, so it does not expose the real
  address to a third party — only the **set of nodes** to the ISP. That set is
  one-time information and is already disclosed before connect (the first
  smart-connect pick, or any visit to the Servers screen while disconnected),
  so scoping probes while connected would recover nothing. An IPv6-only node
  cannot be measured while connected because `leak_guard` blocks `2000::/3`.
  Residual, accepted: nodes that first appear from a subscription refresh
  *while connected* have never been disclosed, and opening Servers then
  discloses them all at once. Tracking a per-node "already seen" bit is more
  clever than it is worth.
- **Kill switch on + connected means no ping, and that is the correct
  outcome.** `BlockOutbound` allows only `xray.exe` and the TUN subnet. Opening
  `xraybox.exe` through the kill switch just to make ping work would reopen
  the hole the kill switch exists to close. Binding the probe to the TUN
  address is also unusable: the packet would enter Xray and report a wrong
  number. `ping_profiles` therefore skips the probe and does not write the
  file — recording a firewall drop as `None` would fill every history and
  make `smart_score` return infinity for every node at the next connect.
- **Kill switch no longer breaks name resolution.** The TUN adapter now has
  `settings.dns`, so a query to `1.1.1.1` leaves with a TUN source address and
  matches `localip=172.19.0.0/30`. The older warning that the kill switch
  could take name resolution down only applied when the adapter had no
  resolver of its own.

## Still open

Recorded so nobody rediscovers them as new findings:

- **Connected-state leak checks on a private resolver.** The disconnected
  baseline was taken on Wi-Fi with public DNS (`8.8.8.8` / `8.8.4.4`). On
  that adapter both `TUN adapter has DNS servers` and `Off-tunnel resolvers
  are blocked` are uninformative: a public resolver already follows
  `0.0.0.0/0` into the tunnel, guard or no guard. Re-run
  `scripts/leak-test.ps1 -BaselineIp …` while connected on Ethernet (or any
  adapter whose resolver is a LAN address such as `192.168.254.254`).
- **Smart multi-homed name resolution.** Windows can fan a query out over every
  interface in parallel. The port-53 block covers the case where the other
  interface's resolver is private, which is the common one, but a public resolver
  configured on the physical NIC would still be queried with the physical source
  address. The complete fix is
  `HKLM\SOFTWARE\Policies\Microsoft\Windows NT\DNSClient\DisableSmartNameResolution = 1`
  applied while connected and restored on disconnect. Upstream's own answer to
  the same question was WFP-level interception.
- **The gRPC API has no authentication, and the hole is local-only.**
  `api.listen` is a fixed `127.0.0.1:9090` with `HandlerService` and
  `RoutingService` exposed. It does not listen on the LAN or the internet, so
  only a process on this machine can call it — and that process can remove
  the `proxy` outbound or add a `direct` rule. Xray has no auth or TLS for
  the API. The v26.9.9 binary has no `domainsocket` / `dsock` strings, and
  `xray api` only accepts `IP:port`, so there is no file-socket ACL path.
  The remaining option is an ephemeral port chosen at start and threaded
  through `wait_api`, `confirm_api`, `add_rules`, `add_outbound`,
  `remove_outbound` and `query_proxy_traffic`. That is obscurity, not
  authentication; it also happens to avoid colliding with whatever else
  wants 9090. Deliberately not bundled with the leak fixes.

## When adding a route or resolver

1. Add the unit test first — the config shape is cheap to pin.
2. Ask which of the three escape routes above the change opens or closes. A new
   `direct` rule means real-address traffic for those destinations by design;
   make sure that is intended and visible in the UI.
3. Run `scripts/leak-test.ps1` disconnected, then connected, on an adapter with a
   **private** resolver.
4. If the change touches IPv6 or `autoSystemRoutingTable`, rerun on a network
   with working IPv6.
5. Update the tables in this file rather than leaving the reasoning in the commit
   message.
