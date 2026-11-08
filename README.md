# xraybox

Desktop and mobile client for **VLESS Reality**. It imports a subscription or a manual endpoint, then runs [Xray-core](https://github.com/XTLS/Xray-core) in a tunnel on Windows, macOS, Android, and iOS.

The interface is English and Vietnamese. Appearance follows the system theme, or a fixed light or dark theme.

## Features

- Subscription URL, clipboard paste, and QR (camera or image)
- Manual VLESS Reality endpoint
- Ping and smart connect
- Scheduled subscription refresh (3 hours, 1 day, 7 days, or off)
- Per-service routing packs and a China direct policy, using `geosite.dat` / `geoip.dat`
- Remote DNS, LAN bypass, and an internet kill switch
- Auto-connect, start on boot, and a tray icon on desktop
- Session duration, traffic, and current IP

## Tech stack

- **UI:** Vue 3, TypeScript, Vite
- **Shell:** Tauri 2, Rust
- **Engine:** Xray-core sidecar, pinned to **v26.9.9** (fallback **v26.7.28**)
- **iOS tunnel:** Xray c-archive plus [hev-socks5-tunnel](https://github.com/heiher/hev-socks5-tunnel)

Xray binaries and the iOS static libraries are downloaded or built locally. They are not committed. See [THIRD_PARTY.md](THIRD_PARTY.md).

## Prerequisites

| Tool | Notes |
| --- | --- |
| Node.js 20+ | Frontend |
| pnpm | Package scripts |
| Rust (rustup) | Tauri shell |
| Windows | WebView2 and the MSVC C++ build tools |
| macOS | Xcode command line tools for the desktop app; full Xcode for iOS |
| Android | JDK 17–21, Android SDK, and NDK. JDK 22+ breaks the Gradle build |

## Getting started

```bash
git clone https://github.com/nshermione/xraybox.git
cd xraybox
pnpm install
```

Fetch the engine for the platform you are running, then start the app.

Windows:

```powershell
.\scripts\fetch-xray.ps1
pnpm tauri dev
```

macOS:

```bash
./scripts/fetch-xray.sh
pnpm tauri dev
```

Android (arm64):

```powershell
.\scripts\fetch-xray-android.ps1
pnpm android:dev
```

iOS 17 (on a Mac):

```bash
./scripts/build-xray-ios.sh
./scripts/build-hev-ios.sh
pnpm ios:dev
```

`pnpm tauri dev` serves the UI at `http://localhost:1420` and opens the Tauri window. The first macOS connect installs a root helper and opens a `utun` interface.

## Scripts

| Command | What it does |
| --- | --- |
| `pnpm dev` | Vite only, no native shell |
| `pnpm build` | Typecheck and build the frontend into `dist/` |
| `pnpm tauri dev` | Desktop dev (Windows or macOS) |
| `pnpm windows:release` | NSIS and MSI installers |
| `pnpm macos:build` | `.app` and `.dmg` |
| `pnpm android:dev` | Debug install on a device or emulator |
| `pnpm android:release` | Signed arm64 APK at `release/xraybox-<version>-arm64.apk` |
| `pnpm ios:dev` | Debug build on a connected iPhone |
| `pnpm ios:release` | Signed IPA, uploaded to TestFlight |
| `pnpm icon` | Regenerate app icons from `src-tauri/icons/icon.svg` |

Rust tests:

```bash
cd src-tauri
cargo test --lib
```

## Platform notes

**Windows.** `wintun.dll` has to sit beside `xray.exe`. Wintun does not search the working directory or `PATH`. `src-tauri/build.rs` copies it there for local builds. `scripts/fetch-xray.ps1` checks the release `.dgst` before extracting.

**macOS.** `scripts/fetch-xray.sh` writes `xray-aarch64-apple-darwin` and `xray-x86_64-apple-darwin` under `src-tauri/binaries/`, verifies the digest, and strips quarantine.

**Android.** Connect uses `VpnService` and passes the TUN fd to Xray. The WebView and API traffic go through the tunnel. Xray's own sockets to the VLESS server and remote DNS skip it via `excludeRoute`. Sideload only: there is no Play Store listing. The first `pnpm android:release` creates a keystore in `src-tauri/gen/android/` (gitignored). Back it up. Android installs an update over an existing copy only when the APK is signed with the same key. Bump `version` in `src-tauri/tauri.conf.json` for every release.

**iOS.** `scripts/build-xray-ios.sh` and `scripts/build-hev-ios.sh` write libraries under `src-tauri/ios/Vendor/` (gitignored). Connect uses a Packet Tunnel extension. `pnpm ios:release` needs an App Store Connect API key at `~/.appstoreconnect/private_keys/`. See [docs/testing/ios-device.md](docs/testing/ios-device.md).

Geo data is seeded under `src-tauri/resources/assets/` by the fetch scripts. Those files are gitignored.

## Layout

```
src/                 Vue UI, screens, and client state
src-tauri/src/       Rust engine: Xray config, tunnel, routing, subscriptions
src-tauri/ios/       Packet Tunnel provider and the iOS plugin
scripts/             Fetch Xray, build iOS libraries, release APK and IPA
docs/plans/          Design notes for the engine cutover
docs/testing/        Device and leak-check notes
```

## Third-party code

This repository's own code is MIT. Bundled and downloaded components stay under their own licenses:

| Component | License |
| --- | --- |
| Xray-core | MPL-2.0 |
| Wintun (`wintun.dll` inside the Windows Xray zip) | Prebuilt Binaries License |
| hev-socks5-tunnel | MIT |
| Loyalsoldier `geosite.dat` / `geoip.dat` | Upstream rule lists |

Details and what must ship beside an installer are in [THIRD_PARTY.md](THIRD_PARTY.md).

## License

[MIT](LICENSE). Copyright (c) 2026 xraybox contributors.
