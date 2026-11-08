#!/bin/bash
# Build hev-socks5-tunnel for iOS arm64.
#
# Writes src-tauri/ios/Vendor/libhev-socks5-tunnel.a. Gitignored.
# Override the tag with HEV_REF.

set -euo pipefail

ref="${HEV_REF:-2.18.0}"
root="$(cd "$(dirname "$0")/.." && pwd)"
vendor="$root/src-tauri/ios/Vendor"
src="$vendor/src/hev-socks5-tunnel"
mkdir -p "$vendor/src"

if [[ -d /Applications/Xcode.app/Contents/Developer ]]; then
  export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
fi
if ! xcrun --sdk iphoneos --show-sdk-path >/dev/null 2>&1; then
  echo "The iOS SDK is missing. Install Xcode and point DEVELOPER_DIR at it." >&2
  exit 1
fi

if [[ ! -d "$src/.git" ]]; then
  git clone --recursive --branch "$ref" --depth 1 \
    https://github.com/heiher/hev-socks5-tunnel "$src"
else
  git -C "$src" fetch --depth 1 origin "refs/tags/${ref}:refs/tags/${ref}" || true
  git -C "$src" checkout "$ref"
  git -C "$src" submodule update --init --recursive
fi

sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
clang="$(xcrun --sdk iphoneos --find clang)"
export CC="$clang"
export CFLAGS="-arch arm64 -isysroot ${sdk} -miphoneos-version-min=17.0 -fembed-bitcode=off"
export LDFLAGS="-arch arm64 -isysroot ${sdk} -miphoneos-version-min=17.0"

make -C "$src" clean || true
make -C "$src" static \
  CC="$clang" \
  CFLAGS="$CFLAGS"

cp "$src/bin/libhev-socks5-tunnel.a" "$vendor/libhev-socks5-tunnel.a"
cp "$src/third-part/hev-task-system/bin/libhev-task-system.a" "$vendor/libhev-task-system.a"
cp "$src/third-part/lwip/bin/liblwip.a" "$vendor/liblwip.a"
cp "$src/third-part/yaml/bin/libyaml.a" "$vendor/libyaml.a"
echo "Wrote hev static libraries under $vendor"
ls -lh "$vendor"/libhev-socks5-tunnel.a "$vendor"/libhev-task-system.a "$vendor"/liblwip.a "$vendor"/libyaml.a
