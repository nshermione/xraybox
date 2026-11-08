# iPhone device checklist

Run on a physical iPhone, iOS 17 or newer, with a paid Apple Developer team. The Simulator does not establish a packet tunnel. Do not treat a green `cargo test` as this list.

Build first:

```bash
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
./scripts/build-xray-ios.sh
./scripts/build-hev-ios.sh
pnpm tauri ios init --ci --skip-targets-install   # once
./scripts/ios-integrate.sh
pnpm tauri ios dev
```

Set the development team in Xcode or `APPLE_DEVELOPMENT_TEAM`. Do not commit the team id.

## RAM gate (before calling connect done)

1. Connect once with the bundled `geosite.dat` and `geoip.dat`.
2. If the app shows `ios_memory_budget`, stop. Xray plus full geo did not fit the extension. Do not delete routing packs to hide it. Decide with the maintainer before changing assets.
3. A successful Running means the footprint stayed under 45MB. Record the footprint line from the extension log in the App Group (`xray.log` / `ready.json`).

## Connect

- [ ] First Connect shows the system VPN consent once.
- [ ] Status becomes Running only after the HTTP 204 probe.
- [ ] Second Connect does not ask again.
- [ ] Cancelling the consent shows the iPhone permission error, not the macOS administrator string.
- [ ] Disconnect removes the VPN icon.
- [ ] A failed connect does not leave the profile stuck on Connecting.

## Traffic

- [ ] Safari and another app exit via the VLESS server (public IP on the home screen matches the server, not the carrier).
- [ ] The xraybox UI still loads. With `tauri ios dev`, the page keeps updating after Connect (dev host is excluded).
- [ ] Switching servers while connected does not ask for VPN permission again. The VPN icon stays. There is a short stall, then the new server’s IP.
- [ ] IPv6 does not leave the phone (no global AAAA answer on the wire, or the lookup fails closed). On iOS this is an Xray route (`::/0` → `block`), not a packet drop in the extension.
- [ ] A LAN device is still reachable for non-DNS traffic. DNS to the router (`:53`) does not leave. On iOS that block is an Xray route (port 53 to private, CGNAT, and link-local ranges → `block`), placed before the LAN bypass.

## Kill switch and boot

- [ ] Kill switch on, Xray dead inside the extension: packets drop, the VPN icon stays, Xray is restarted. Traffic does not use the carrier IP during the gap.
- [ ] Kill switch off, Xray dead: the tunnel comes down and the phone has a network again.
- [ ] Disconnect with the kill switch on does not immediately reconnect.
- [ ] Start on boot on, tunnel left connected, reboot, unlock: the VPN comes back without opening the app.
- [ ] After an explicit Disconnect, reboot does not bring the VPN back until the app is opened.

## Edges

- [ ] Lock the screen for one minute. The VPN icon is still there and a page load works.
- [ ] Switch Wi-Fi and cellular. The tunnel stays up and the public IP is still the server.
- [ ] Turn the VPN off in Settings or Control Center. The app shows Disconnected and does not fight the toggle unless On Demand is still armed for an active session.
- [ ] On a captive portal, Connect surfaces the hijack error telling you to sign in to Wi-Fi. The app does not exclude the whole network.
- [ ] Landscape still shows the bottom tabs, not the desktop sidebar.
- [ ] QR import can open the camera.
- [ ] Notch and the home indicator do not cover the header or the tab bar.

## What this build cannot claim

Menu bar, single instance, a root helper, and a firewall that stays after the VPN is off are not iPhone features.
