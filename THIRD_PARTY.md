# Third-party notices

## Xray-core

Windows sidecar binaries are downloaded by `scripts/fetch-xray.ps1` from
[XTLS/Xray-core](https://github.com/XTLS/Xray-core) releases (currently pinned
to **v26.9.9**; fallback pin **v26.7.28**). They are not committed to this
repository. Xray-core is MPL-2.0.

## WinTun

The Xray-core Windows zip redistributes `wintun.dll` under Wintun's Prebuilt
Binaries License. `scripts/fetch-xray.ps1` copies that DLL next to the sidecar
in `src-tauri/binaries/` (gitignored). `tauri.conf.json` bundles
`binaries/wintun.dll` so the TUN inbound can load it at runtime. Ship
`LICENSE-wintun.txt` alongside the installer. WinTUN is copyright WireGuard LLC
/ the WireGuard project.

## hev-socks5-tunnel

iPhone builds link [hev-socks5-tunnel](https://github.com/heiher/hev-socks5-tunnel)
(MIT) from `scripts/build-hev-ios.sh`. The static library is not committed.

## Xray-core iOS archive

`scripts/build-xray-ios.sh` compiles the same **v26.9.9** tag as a c-archive for
`GOOS=ios`. Output `src-tauri/ios/Vendor/libxray.a` is gitignored. MPL-2.0.

Runtime geo assets are `geosite.dat` / `geoip.dat` from
[Loyalsoldier/v2ray-rules-dat](https://github.com/Loyalsoldier/v2ray-rules-dat),
seeded under `src-tauri/resources/assets/`. They replace the Sing-box `.srs`
rule-sets that `00`–`06` shipped.
