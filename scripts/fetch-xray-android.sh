#!/bin/bash
# Fetch a pinned Xray-core Android arm64 build into jniLibs as libxray.so.
#
# Pin: v26.9.9 (XTLS/Xray-core). Override with the first argument or
# XRAY_VERSION (e.g. 26.9.9). Same pin as scripts/fetch-xray-android.ps1.
#
# Android 10+ refuses exec from the app files dir. The binary is disguised
# as libxray.so and extracted to nativeLibraryDir via useLegacyPackaging.
#
# The zip is checked against its sibling .dgst (SHA2-256).

set -euo pipefail

version="${1:-${XRAY_VERSION:-26.9.9}}"
version="${version#v}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; then
  echo "XRAY_VERSION must look like 26.9.9, got '$version'" >&2
  exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
dest_dir="$root/src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a"
dest_path="$dest_dir/libxray.so"
asset="Xray-android-arm64-v8a.zip"
url="https://github.com/XTLS/Xray-core/releases/download/v${version}/${asset}"

mkdir -p "$dest_dir"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/xraybox-xray-android.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

echo "Downloading $url"
curl -fsSL "$url" -o "$tmp/$asset"
curl -fsSL "${url}.dgst" -o "$tmp/${asset}.dgst"

line="$(grep -m1 'SHA2-256=' "$tmp/${asset}.dgst" || true)"
[[ -n "$line" ]] || { echo "SHA2-256= line not found in ${asset}.dgst" >&2; exit 1; }
expected="$(printf '%s' "${line#*SHA2-256=}" | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')"
actual="$(sha256_of "$tmp/$asset" | tr '[:upper:]' '[:lower:]')"
if [[ "$actual" != "$expected" ]]; then
  echo "SHA256 mismatch for $asset: expected $expected, got $actual" >&2
  exit 1
fi
echo "SHA2-256 verified"

unzip -q -o "$tmp/$asset" -d "$tmp/extract"
found="$(find "$tmp/extract" -type f -name xray | head -n 1)"
[[ -n "$found" ]] || { echo "xray binary not found inside $asset" >&2; exit 1; }
cp -f "$found" "$dest_path"
echo "Wrote $dest_path"
