#!/bin/bash
# Build Xray-core v26.9.9 as an iOS arm64 c-archive for the packet tunnel.
#
# Requires Xcode (not only Command Line Tools) and Go 1.22+.
# Writes src-tauri/ios/Vendor/libxray.a and libxray.h. Both are gitignored.

set -euo pipefail

version="${1:-${XRAY_VERSION:-26.9.9}}"
version="${version#v}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; then
  echo "XRAY_VERSION must look like 26.9.9, got '$version'" >&2
  exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
src="$root/src-tauri/ios/xray"
vendor="$root/src-tauri/ios/Vendor"
mkdir -p "$vendor"

if [[ -d /Applications/Xcode.app/Contents/Developer ]]; then
  export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
fi
if ! xcrun --sdk iphoneos --show-sdk-path >/dev/null 2>&1; then
  echo "The iOS SDK is missing. Install Xcode and point DEVELOPER_DIR at it." >&2
  exit 1
fi

if ! command -v go >/dev/null 2>&1 || ! go version | grep -q 'go1\.2[7-9]\|go1\.[3-9]'; then
  toolchain="$vendor/go"
  need=1
  if [[ -x "$toolchain/bin/go" ]] && "$toolchain/bin/go" version | grep -q 'go1\.2[7-9]\|go1\.[3-9]'; then
    need=0
  fi
  if [[ "$need" == 1 ]]; then
    echo "Downloading Go 1.27.1 into src-tauri/ios/Vendor/go (Xray v26 needs Go 1.27)."
    archive="$vendor/go.tgz"
    curl -fsSL "https://go.dev/dl/go1.27.1.darwin-arm64.tar.gz" -o "$archive"
    rm -rf "$toolchain"
    tar -C "$vendor" -xzf "$archive"
    rm -f "$archive"
  fi
  export PATH="$toolchain/bin:$PATH"
fi

sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
clang="$(xcrun --sdk iphoneos --find clang)"
export CGO_ENABLED=1
export GOOS=ios
export GOARCH=arm64
export CC="$clang"
export CGO_CFLAGS="-arch arm64 -isysroot ${sdk} -miphoneos-version-min=17.0"
export CGO_LDFLAGS="-arch arm64 -isysroot ${sdk} -miphoneos-version-min=17.0"

xray_src="$vendor/src/Xray-core"
if [[ ! -d "$xray_src/.git" ]]; then
  git clone --branch "v${version}" --depth 1 https://github.com/XTLS/Xray-core "$xray_src"
else
  git -C "$xray_src" fetch --depth 1 origin "refs/tags/v${version}:refs/tags/v${version}"
  git -C "$xray_src" checkout "v${version}"
fi

cd "$src"
# v26 tags are not valid Go module versions (module path has no /v26).
go mod edit -require="github.com/xtls/xray-core@v0.0.0"
go mod edit -replace="github.com/xtls/xray-core=../Vendor/src/Xray-core"
go mod tidy
go build -trimpath -buildmode=c-archive -o "$vendor/libxray.a" .

echo "Wrote $vendor/libxray.a"
ls -lh "$vendor/libxray.a" "$vendor/libxray.h"
