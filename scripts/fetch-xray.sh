#!/bin/bash
# Fetch pinned Xray-core macOS builds into the Tauri sidecar paths.
#
# Pin: v26.9.9 (XTLS/Xray-core). Override with the first argument or
# XRAY_VERSION (digits only, e.g. 26.9.9). Same pin as scripts/fetch-xray.ps1.
#
# Writes:
#   src-tauri/binaries/xray-aarch64-apple-darwin
#   src-tauri/binaries/xray-x86_64-apple-darwin
# and seeds Loyalsoldier geosite.dat / geoip.dat under src-tauri/resources/assets/.
#
# Each zip is checked against its sibling .dgst (SHA2-256). Quarantine is
# stripped and the execute bit is set, or Gatekeeper refuses to run the binary.

set -euo pipefail

version="${1:-${XRAY_VERSION:-26.9.9}}"
version="${version#v}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; then
  echo "XRAY_VERSION must look like 26.9.9, got '$version'" >&2
  exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
dest_dir="$root/src-tauri/binaries"
assets_dir="$root/src-tauri/resources/assets"
mkdir -p "$dest_dir" "$assets_dir"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/xraybox-xray.XXXXXX")"
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT

verify_and_extract() {
  local asset="$1"
  local dest="$2"
  local url="https://github.com/XTLS/Xray-core/releases/download/v${version}/${asset}"
  local zip="$tmp/$asset"
  local dgst="$tmp/${asset}.dgst"

  echo "Downloading $url"
  curl -fsSL "$url" -o "$zip"
  curl -fsSL "${url}.dgst" -o "$dgst"

  local line expected actual
  line="$(grep -m1 'SHA2-256=' "$dgst" || true)"
  if [[ -z "$line" ]]; then
    echo "SHA2-256= line not found in ${asset}.dgst" >&2
    exit 1
  fi
  expected="$(printf '%s' "${line#*SHA2-256=}" | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')"
  actual="$(shasum -a 256 "$zip" | awk '{print $1}' | tr '[:upper:]' '[:lower:]')"
  if [[ "$actual" != "$expected" ]]; then
    echo "SHA256 mismatch for $asset: expected $expected, got $actual" >&2
    exit 1
  fi
  echo "SHA2-256 verified for $asset"

  local extract="$tmp/extract-${asset}"
  mkdir -p "$extract"
  unzip -q -o "$zip" -d "$extract"
  local found
  found="$(find "$extract" -type f -name xray | head -n 1)"
  if [[ -z "$found" ]]; then
    echo "xray not found inside $asset" >&2
    exit 1
  fi
  cp -f "$found" "$dest"
  chmod +x "$dest"
  xattr -dr com.apple.quarantine "$dest" 2>/dev/null || true
  echo "Wrote $dest"
}

verify_and_extract "Xray-macos-arm64-v8a.zip" "$dest_dir/xray-aarch64-apple-darwin"
verify_and_extract "Xray-macos-64.zip" "$dest_dir/xray-x86_64-apple-darwin"

download_geo() {
  local name="$1"
  local url="$2"
  local dest="$assets_dir/$name"
  echo "Downloading $url"
  curl -fsSL "$url" -o "$dest"
  if [[ ! -s "$dest" ]]; then
    echo "Loyalsoldier $name download produced an empty file" >&2
    exit 1
  fi
  echo "Wrote $dest"
}

download_geo "geosite.dat" "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geosite.dat"
download_geo "geoip.dat" "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geoip.dat"
