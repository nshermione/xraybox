#!/bin/bash
# Build a signed release APK (arm64) for sideloading. No Play Store involved.
#
# Usage:
#   pnpm android:release
#
# Output:
#   release/xraybox-<version>-arm64.apk
#
# Install on a phone: copy the APK over (or host it) and open it, allowing
# "Install unknown apps" for the app used to open it. Or: $ANDROID_HOME/platform-tools/adb install -r <apk>
#
# Requirements: JDK 17-21 (22+ breaks Gradle), Android SDK + NDK, Rust (rustup), pnpm.
#   ANDROID_HOME  SDK path. Auto-detected from the usual install locations.
#   NDK_HOME      NDK path. Default: newest NDK under $ANDROID_HOME/ndk.
#   JAVA_HOME     JDK 17-21 path. Default: first supported JDK found.
#
# Signing: the first run creates a keystore at
#   src-tauri/gen/android/xraybox-release.jks
# plus keystore.properties beside it (both gitignored). Android only accepts an
# update when the APK is signed with the SAME key as the installed app, so back
# the keystore up and reuse it for every release. Losing it means every user
# must uninstall before they can install the next version.

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
android_dir="$root/src-tauri/gen/android"
keystore_file="$android_dir/xraybox-release.jks"
keystore_properties="$android_dir/keystore.properties"
jni_libs_dir="$android_dir/app/src/main/jniLibs/arm64-v8a"
apk_output_dir="$android_dir/app/build/outputs/apk"
release_dir="$root/release"
rust_target="aarch64-linux-android"
key_alias="xraybox"
min_jdk_major=17
max_jdk_major=21

fail() {
  echo "Error: $1" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "'$1' not found. $2"
}

find_android_sdk() {
  local candidate
  for candidate in \
    "${ANDROID_HOME:-}" \
    "${ANDROID_SDK_ROOT:-}" \
    "$HOME/Library/Android/sdk" \
    "$HOME/Android/Sdk"; do
    if [[ -n "$candidate" && -d "$candidate" ]]; then
      echo "$candidate"
      return
    fi
  done
  fail "Android SDK not found. Install Android Studio or set ANDROID_HOME."
}

find_newest_child() {
  # Newest version-sorted subdirectory name of $1, empty when there is none.
  [[ -d "$1" ]] || return 0
  ls -1 "$1" | sort -V | tail -n 1
}

java_major_version() {
  # Major version of the JDK at $1 (e.g. 17), empty when it is not a JDK.
  local java_bin="$1/bin/java"
  [[ -x "$java_bin" ]] || return 0
  "$java_bin" -version 2>&1 |
    sed -n 's/.*version "\([0-9]*\).*/\1/p' | head -n 1
}

is_supported_jdk() {
  # The Gradle/Kotlin/AGP versions in gen/android break on JDK 22+.
  local major
  major="$(java_major_version "$1")"
  [[ -n "$major" && "$major" -ge "$min_jdk_major" && "$major" -le "$max_jdk_major" ]]
}

find_java_home() {
  local candidate
  local candidates=("${JAVA_HOME:-}")
  if [[ -x /usr/libexec/java_home ]]; then
    local version
    for version in 21 17; do
      candidates+=("$(/usr/libexec/java_home -v "$version" 2>/dev/null || true)")
    done
  fi
  candidates+=(
    /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home
    /opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home
    /usr/lib/jvm/java-21-openjdk-amd64
    /usr/lib/jvm/java-17-openjdk-amd64
    "/Applications/Android Studio.app/Contents/jbr/Contents/Home"
  )
  for candidate in "${candidates[@]}"; do
    if [[ -n "$candidate" ]] && is_supported_jdk "$candidate"; then
      echo "$candidate"
      return
    fi
  done
  fail "No JDK $min_jdk_major-$max_jdk_major found (JDK 22+ is not supported by this Gradle setup). Install one, e.g. 'brew install openjdk@17', or point JAVA_HOME at it."
}

setup_environment() {
  export ANDROID_HOME
  ANDROID_HOME="$(find_android_sdk)"
  export ANDROID_SDK_ROOT="$ANDROID_HOME"

  if [[ -z "${NDK_HOME:-}" ]]; then
    local ndk_version
    ndk_version="$(find_newest_child "$ANDROID_HOME/ndk")"
    [[ -n "$ndk_version" ]] ||
      fail "Android NDK not found. Install it via Android Studio > SDK Manager > SDK Tools, or set NDK_HOME."
    NDK_HOME="$ANDROID_HOME/ndk/$ndk_version"
  fi
  [[ -d "$NDK_HOME" ]] || fail "NDK_HOME does not exist: $NDK_HOME"
  export NDK_HOME

  export JAVA_HOME
  JAVA_HOME="$(find_java_home)"
  export PATH="$JAVA_HOME/bin:$PATH"

  require_command keytool "Install a full JDK (not a JRE)."
  require_command openssl "Install OpenSSL."
  require_command rustup "Install Rust from https://rustup.rs."
  require_command pnpm "Install pnpm."
}

ensure_rust_target() {
  if ! rustup target list --installed | grep -qx "$rust_target"; then
    echo "Installing Rust target $rust_target ..."
    rustup target add "$rust_target"
  fi
}

ensure_xray_sidecar() {
  if [[ -s "$jni_libs_dir/libxray.so" ]]; then
    return
  fi
  echo "libxray.so missing, fetching Xray-core ..."
  bash "$root/scripts/fetch-xray-android.sh"
}

random_secret() {
  # Hex only, so it is safe in a properties file and on a command line.
  openssl rand -hex 16
}

ensure_signing_key() {
  if [[ -f "$keystore_properties" ]]; then
    [[ -f "$keystore_file" ]] ||
      fail "$keystore_properties exists but $keystore_file is missing."
    return
  fi
  [[ ! -f "$keystore_file" ]] ||
    fail "$keystore_file exists without $keystore_properties. Restore the properties file, or delete the keystore to create a new one (users of the old key will have to uninstall)."

  echo "No signing key found, creating $keystore_file ..."
  local password
  password="$(random_secret)"
  keytool -genkeypair \
    -keystore "$keystore_file" \
    -storetype PKCS12 \
    -alias "$key_alias" \
    -keyalg RSA -keysize 4096 -validity 36500 \
    -storepass "$password" -keypass "$password" \
    -dname "CN=xraybox" >/dev/null

  (
    umask 077
    cat >"$keystore_properties" <<EOF
storeFile=$(basename "$keystore_file")
storePassword=$password
keyAlias=$key_alias
keyPassword=$password
EOF
  )
  echo
  echo "!! BACK UP these two files now (password manager / private storage):"
  echo "     $keystore_file"
  echo "     $keystore_properties"
  echo "!! Without them you cannot ship updates that install over existing copies."
  echo
}

app_version() {
  node -p "require('$root/src-tauri/tauri.conf.json').version"
}

build_apk() {
  # A stale APK from a previous run must not be mistaken for this build.
  rm -rf "$apk_output_dir"

  echo "Building release APK ($rust_target) ..."
  (
    cd "$root"
    pnpm tauri android build --apk --target aarch64 --ci
  )
}

locate_signed_apk() {
  local apk_path
  apk_path="$(find "$apk_output_dir" -name '*-release.apk' -type f | head -n 1)"
  if [[ -z "$apk_path" ]]; then
    local unsigned
    unsigned="$(find "$apk_output_dir" -name '*-release-unsigned.apk' -type f | head -n 1)"
    [[ -z "$unsigned" ]] ||
      fail "Build produced an unsigned APK ($unsigned). Check $keystore_properties."
    fail "No release APK found under $apk_output_dir."
  fi
  echo "$apk_path"
}

verify_signature() {
  local apk_path="$1"
  local build_tools_version apksigner
  build_tools_version="$(find_newest_child "$ANDROID_HOME/build-tools")"
  apksigner="$ANDROID_HOME/build-tools/$build_tools_version/apksigner"
  if [[ -z "$build_tools_version" || ! -x "$apksigner" ]]; then
    echo "apksigner not found, skipping signature check."
    return
  fi
  "$apksigner" verify "$apk_path" || fail "APK signature verification failed."
}

publish_apk() {
  local built_apk="$1"
  local final_apk="$release_dir/xraybox-$(app_version)-arm64.apk"
  mkdir -p "$release_dir"
  cp -f "$built_apk" "$final_apk"
  verify_signature "$final_apk"
  echo
  echo "APK: $final_apk"
  echo "Install: open it on the phone, or with USB debugging on, run:"
  echo "  \"$ANDROID_HOME/platform-tools/adb\" install -r \"$final_apk\""
}

setup_environment
ensure_rust_target
ensure_xray_sidecar
ensure_signing_key
build_apk
publish_apk "$(locate_signed_apk)"
