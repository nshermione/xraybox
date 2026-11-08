use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() {
    link_ios_vpn_plugin();
    copy_wintun_beside_sidecar();
    tauri_build::build();
}

/// VpnPlugin.swift imports Tauri, so it cannot be compiled by the Xcode app
/// target. Build it as a static Swift package, the same way tauri-plugin does.
fn link_ios_vpn_plugin() {
    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("ios") {
        return;
    }
    link_ios_vpn_plugin_on_macos();
}

#[cfg(target_os = "macos")]
fn link_ios_vpn_plugin_on_macos() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let package = manifest.join("ios").join("plugin");
    println!(
        "cargo:rerun-if-changed={}",
        package.join("Package.swift").display()
    );
    let sources = package.join("Sources");
    if let Ok(entries) = fs::read_dir(&sources) {
        for entry in entries.flatten() {
            println!("cargo:rerun-if-changed={}", entry.path().display());
        }
    }

    let tauri_library = env::var("DEP_TAURI_IOS_LIBRARY_PATH").unwrap_or_else(|_| {
        panic!("missing DEP_TAURI_IOS_LIBRARY_PATH; tauri must be a direct dependency")
    });
    println!("cargo:rerun-if-env-changed=DEP_TAURI_IOS_LIBRARY_PATH");
    let dest = manifest.join("ios").join(".tauri").join("tauri-api");
    copy_tree(
        Path::new(&tauri_library),
        &dest,
        &[".build", "Package.resolved", "Tests"],
    )
    .expect("failed to copy the Tauri Swift package next to the VPN plugin");

    if env::var_os("IPHONEOS_DEPLOYMENT_TARGET").is_none() {
        env::set_var("IPHONEOS_DEPLOYMENT_TARGET", "17.0");
    }
    println!("cargo:rustc-link-lib=framework=NetworkExtension");
    tauri_utils::build::link_apple_library("xraybox-vpn", &package);
    export_swift_rs_runtime_symbols("xraybox-vpn");
}

/// `@_cdecl` functions that the swift-rs Rust crate imports from its Swift
/// runtime (`SwiftRs.o`).
#[cfg(target_os = "macos")]
const SWIFT_RS_RUNTIME_SYMBOLS: [&str; 3] =
    ["_retain_object", "_release_object", "_string_from_bytes"];

/// A release Swift build on Xcode 27 internalizes `SwiftRs.o`'s `@_cdecl`
/// exports (local `t` in `nm`), so linking the Rust lib fails with undefined
/// `_retain_object`/`_release_object`/`_string_from_bytes`. swift-rs promotes
/// only the package's own symbols, never those of its SwiftRs dependency.
/// Promote them in this one package's archive; promoting in several would
/// duplicate the globals and crash ld. A debug build keeps them global, so
/// this changes nothing there.
///
/// SwiftPM lays the archive out differently per Xcode beta, so every copy
/// under the package output directory is patched.
#[cfg(target_os = "macos")]
fn export_swift_rs_runtime_symbols(package_name: &str) {
    let package_out = PathBuf::from(env::var("OUT_DIR").unwrap())
        .join("swift-rs")
        .join(package_name);
    let mut archives = Vec::new();
    find_files_named(&package_out, &format!("lib{package_name}.a"), &mut archives);
    if archives.is_empty() {
        panic!("no lib{package_name}.a under {}", package_out.display());
    }

    let objcopy = rustup_llvm_objcopy();
    for archive in archives {
        let mut command = std::process::Command::new(&objcopy);
        for symbol in SWIFT_RS_RUNTIME_SYMBOLS {
            command.arg(format!("--globalize-symbol={symbol}"));
        }
        let status = command
            .arg(&archive)
            .status()
            .expect("failed to run llvm-objcopy");
        if !status.success() {
            panic!(
                "llvm-objcopy failed to globalize swift-rs symbols in {}",
                archive.display()
            );
        }
    }
}

#[cfg(target_os = "macos")]
fn rustup_llvm_objcopy() -> PathBuf {
    let sysroot = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .expect("failed to run `rustc --print sysroot`");
    let objcopy = PathBuf::from(String::from_utf8_lossy(&sysroot.stdout).trim())
        .join("lib/rustlib")
        .join(env::var("HOST").unwrap())
        .join("bin/llvm-objcopy");
    if !objcopy.is_file() {
        panic!(
            "{} is missing; run `rustup component add llvm-tools`",
            objcopy.display()
        );
    }
    objcopy
}

#[cfg(target_os = "macos")]
fn find_files_named(dir: &Path, name: &str, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_files_named(&path, name, found);
        } else if path.file_name().is_some_and(|file| file == name) {
            found.push(path);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn link_ios_vpn_plugin_on_macos() {
    panic!("the iOS VPN Swift package can only be linked on macOS");
}

fn copy_tree(source: &Path, target: &Path, ignore: &[&str]) -> io::Result<()> {
    if target.exists() {
        fs::remove_dir_all(target)?;
    }
    copy_tree_inner(source, source, target, ignore)
}

fn copy_tree_inner(
    root: &Path,
    source: &Path,
    target_root: &Path,
    ignore: &[&str],
) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let rel_str = rel.to_string_lossy();
        if ignore.iter().any(|skip| rel_str.starts_with(skip)) {
            continue;
        }
        let dest = target_root.join(rel);
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&dest)?;
            copy_tree_inner(root, &path, target_root, ignore)?;
        } else if entry.file_type()?.is_file() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &dest)?;
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    Ok(())
}

fn copy_wintun_beside_sidecar() {
    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest_dir.join("binaries").join("wintun.dll");
    println!("cargo:rerun-if-changed={}", src.display());

    if !src.is_file() {
        println!("cargo:warning=binaries/wintun.dll is missing; run scripts/fetch-xray.ps1");
        return;
    }

    let Some(dest_dir) = profile_dir_from_out_dir() else {
        println!("cargo:warning=could not derive target/<profile> from OUT_DIR");
        return;
    };
    let dest = dest_dir.join("wintun.dll");
    if let Err(err) = fs::copy(&src, &dest) {
        println!("cargo:warning=failed to copy wintun.dll next to the sidecar: {err}");
    }
}

// Wintun loads the DLL only from the running image's directory (plus
// System32). cwd and PATH are ignored, so the copy must land beside
// target/<profile>/xray.exe — the path sidecar_path() resolves first in
// `tauri dev`. OUT_DIR is target/<profile>/build/<pkg>-<hash>/out.
fn profile_dir_from_out_dir() -> Option<PathBuf> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").ok()?);
    ancestors(&out_dir, 3).map(Path::to_path_buf)
}

fn ancestors(path: &Path, n: usize) -> Option<&Path> {
    let mut current = path;
    for _ in 0..n {
        current = current.parent()?;
    }
    Some(current)
}
