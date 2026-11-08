#!/bin/bash
# After `pnpm tauri ios init`, wire the packet tunnel into the generated
# XcodeGen project. Safe to run again. Re-run it if `tauri ios init`
# regenerates project.yml.

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
apple="$root/src-tauri/gen/apple"
yml="$apple/project.yml"
vendor="$root/src-tauri/ios/Vendor"
hev_src="$vendor/src/hev-socks5-tunnel"
if [[ -f "$hev_src/third-part/hev-task-system/bin/libhev-task-system.a" ]]; then
  cp "$hev_src/third-part/hev-task-system/bin/libhev-task-system.a" "$vendor/libhev-task-system.a"
  cp "$hev_src/third-part/lwip/bin/liblwip.a" "$vendor/liblwip.a"
  cp "$hev_src/third-part/yaml/bin/libyaml.a" "$vendor/libyaml.a"
fi
if [[ ! -f "$yml" ]]; then
  echo "Missing $yml. Run: pnpm tauri ios init --ci --skip-targets-install" >&2
  exit 1
fi

export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"

python3 - "$root" "$apple" "$yml" <<'PY'
import pathlib, sys
root, apple, yml_path = map(pathlib.Path, sys.argv[1:])
ios = root / "src-tauri" / "ios"
tunnel = apple / "PacketTunnel"
tunnel.mkdir(parents=True, exist_ok=True)
app_sources = apple / "Sources" / "xraybox"
app_sources.mkdir(parents=True, exist_ok=True)

def copy(src, dest):
    dest.write_bytes(src.read_bytes())

for name in [
    "PacketTunnelProvider.swift",
    "HevTunnel.swift",
    "utun_fd.c",
    "utun_fd.h",
    "PacketTunnel-Bridging-Header.h",
    "PacketTunnel-Info.plist",
    "PacketTunnel.entitlements",
]:
    copy(ios / name, tunnel / name)
copy(ios / "plugin" / "Sources" / "AppGroup.swift", tunnel / "AppGroup.swift")
for stale in (app_sources / "VpnPlugin.swift", app_sources / "AppGroup.swift"):
    if stale.exists():
        stale.unlink()

text = yml_path.read_text()
if "NSCameraUsageDescription:" not in text:
    text = text.replace(
        "UIRequiredDeviceCapabilities: [arm64, metal]\n",
        "UIRequiredDeviceCapabilities: [arm64, metal]\n"
        "        NSCameraUsageDescription: xraybox uses the camera to import a subscription QR code.\n",
        1,
    )
    text = text.replace(
        "        UISupportedInterfaceOrientations:\n"
        "          - UIInterfaceOrientationPortrait\n"
        "          - UIInterfaceOrientationLandscapeLeft\n"
        "          - UIInterfaceOrientationLandscapeRight\n",
        "        UISupportedInterfaceOrientations:\n"
        "          - UIInterfaceOrientationPortrait\n",
        1,
    )
if "- sdk: NetworkExtension.framework" not in text:
    text = text.replace(
        "    - sdk: WebKit.framework\n",
        "    - sdk: WebKit.framework\n    - sdk: NetworkExtension.framework\n    - target: PacketTunnel\n      embed: true\n",
        1,
    )
if "IPHONEOS_DEPLOYMENT_TARGET" not in text:
    text = text.replace(
        "      ENABLE_BITCODE: false\n",
        "      ENABLE_BITCODE: false\n      IPHONEOS_DEPLOYMENT_TARGET: \"17.0\"\n",
        1,
    )
marker = "  PacketTunnel:\n"
if marker not in text:
    text += """
  PacketTunnel:
    type: app-extension
    platform: iOS
    sources:
      - path: PacketTunnel
        excludes:
          - "*-Bridging-Header.h"
          - "*-Info.plist"
          - "*.entitlements"
    info:
      path: PacketTunnel/PacketTunnel-Info.plist
      properties:
        CFBundleDisplayName: xraybox VPN
        CFBundleShortVersionString: "0.1.0"
        CFBundleVersion: "1"
        NSExtension:
          NSExtensionPointIdentifier: com.apple.networkextension.packet-tunnel
          NSExtensionPrincipalClass: "$(PRODUCT_MODULE_NAME).PacketTunnelProvider"
    entitlements:
      path: PacketTunnel/PacketTunnel.entitlements
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: io.github.xraybox.PacketTunnel
        IPHONEOS_DEPLOYMENT_TARGET: "17.0"
        SWIFT_OBJC_BRIDGING_HEADER: PacketTunnel/PacketTunnel-Bridging-Header.h
        CODE_SIGN_ENTITLEMENTS: PacketTunnel/PacketTunnel.entitlements
        LIBRARY_SEARCH_PATHS: $(inherited) $(PROJECT_DIR)/../../ios/Vendor
        OTHER_LDFLAGS: $(inherited) -framework NetworkExtension -framework Security -framework CoreFoundation -lresolv -force_load $(PROJECT_DIR)/../../ios/Vendor/libxray.a -force_load $(PROJECT_DIR)/../../ios/Vendor/libhev-socks5-tunnel.a $(PROJECT_DIR)/../../ios/Vendor/src/hev-socks5-tunnel/third-part/hev-task-system/bin/libhev-task-system.a $(PROJECT_DIR)/../../ios/Vendor/src/hev-socks5-tunnel/third-part/lwip/bin/liblwip.a $(PROJECT_DIR)/../../ios/Vendor/src/hev-socks5-tunnel/third-part/yaml/bin/libyaml.a
    dependencies:
      - sdk: NetworkExtension.framework
      - sdk: libresolv.tbd
"""
yml_path.write_text(text)

entitlements = next(apple.glob("*_iOS/*.entitlements"), None)
if entitlements is None:
    raise SystemExit(f"app entitlements not found under {apple}")
body = entitlements.read_text()
if "group.io.github.xraybox" not in body:
    insertion = """  <key>com.apple.security.application-groups</key>
  <array>
    <string>group.io.github.xraybox</string>
  </array>
  <key>com.apple.developer.networking.networkextension</key>
  <array>
    <string>packet-tunnel-provider</string>
  </array>
"""
    if "</dict>" not in body:
        raise SystemExit(f"unexpected entitlements file: {entitlements}")
    body = body.replace("</dict>", insertion + "</dict>", 1)
    entitlements.write_text(body)
print(f"updated {yml_path}")
print(f"updated {entitlements}")
PY

if ! command -v xcodegen >/dev/null 2>&1; then
  echo "xcodegen is not on PATH. Install it, then re-run this script." >&2
  exit 1
fi
(cd "$apple" && xcodegen generate)
echo "Xcode project updated. Build the native libraries if Vendor is empty:"
echo "  ./scripts/build-xray-ios.sh"
echo "  ./scripts/build-hev-ios.sh"
