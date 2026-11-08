#!/bin/bash
# Build a signed App Store IPA and upload it to TestFlight.
#
# Usage:
#   pnpm ios:release              build + upload
#   pnpm ios:release --no-upload  build the IPA only
#
# Credentials (App Store Connect API key, Users and Access > Integrations):
#   APP_STORE_CONNECT_API_KEY_ID   key id, e.g. ABC123DEFG
#   APP_STORE_CONNECT_ISSUER_ID    issuer UUID
#   The key file must be at ~/.appstoreconnect/private_keys/AuthKey_<KEY_ID>.p8
#
# Optional:
#   BUILD_NUMBER   CFBundleVersion, integers only. Default: UTC yyyymmddHHMM.
#                  TestFlight rejects a build number it has already seen.

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
apple="$root/src-tauri/gen/apple"
vendor="$root/src-tauri/ios/Vendor"
tunnel_plist="$root/src-tauri/ios/PacketTunnel-Info.plist"
ipa_dir="$apple/build/arm64"
upload=true

for arg in "$@"; do
  case "$arg" in
    --no-upload) upload=false ;;
    --) ;; # pnpm forwards the separator
    *)
      echo "Unknown option: $arg" >&2
      exit 1
      ;;
  esac
done

export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
build_number="${BUILD_NUMBER:-$(date -u +%Y%m%d%H%M)}"

fail() {
  echo "$1" >&2
  exit 1
}

require_native_libraries() {
  for lib in libxray.a libhev-socks5-tunnel.a; do
    [[ -f "$vendor/$lib" ]] ||
      fail "Missing $vendor/$lib. Run scripts/build-xray-ios.sh and scripts/build-hev-ios.sh."
  done
}

require_upload_credentials() {
  [[ -n "${APP_STORE_CONNECT_API_KEY_ID:-}" ]] ||
    fail "APP_STORE_CONNECT_API_KEY_ID is not set."
  [[ -n "${APP_STORE_CONNECT_ISSUER_ID:-}" ]] ||
    fail "APP_STORE_CONNECT_ISSUER_ID is not set."
  local key_file="$HOME/.appstoreconnect/private_keys/AuthKey_${APP_STORE_CONNECT_API_KEY_ID}.p8"
  [[ -f "$key_file" ]] || fail "API key file not found: $key_file"
}

require_native_libraries
if [[ "$upload" == true ]]; then
  require_upload_credentials
fi

# `tauri ios build` rewrites CFBundleVersion in this tracked file. Put it
# back afterwards so a release does not leave the working tree dirty.
tunnel_plist_backup="$(mktemp)"
cp "$tunnel_plist" "$tunnel_plist_backup"
restore_tunnel_plist() {
  cp "$tunnel_plist_backup" "$tunnel_plist"
  rm -f "$tunnel_plist_backup"
}
trap restore_tunnel_plist EXIT

# Rebuilding into the same directory must not pick up a stale IPA.
rm -f "$ipa_dir"/*.ipa

echo "Building build $build_number ..."
(
  cd "$root"
  pnpm tauri ios build \
    --ci \
    --export-method app-store-connect \
    --config "{\"bundle\":{\"iOS\":{\"bundleVersion\":\"$build_number\"}}}"
)

ipa_path="$(find "$ipa_dir" -maxdepth 1 -name '*.ipa' | head -n 1)"
[[ -n "$ipa_path" ]] || fail "No IPA produced in $ipa_dir."
echo "IPA: $ipa_path"

if [[ "$upload" == false ]]; then
  echo "Skipping upload (--no-upload)."
  exit 0
fi

echo "Uploading to App Store Connect ..."
xcrun altool --upload-app \
  --type ios \
  --file "$ipa_path" \
  --apiKey "$APP_STORE_CONNECT_API_KEY_ID" \
  --apiIssuer "$APP_STORE_CONNECT_ISSUER_ID"

echo "Uploaded build $build_number. It appears in TestFlight after Apple finishes processing."
