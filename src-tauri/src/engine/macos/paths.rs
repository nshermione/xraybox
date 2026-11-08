//! Path policy for commands the user-level app sends to the root helper.
//! A passed path is not trusted: config and work directories must live under
//! the caller's app-data directory, must not be symlinks, and a config file
//! must be owned by that user. The sidecar may live outside app data (dev
//! binaries, the app bundle) but only under an expected file name.

use std::path::Path;

pub fn config_path_allowed(
    is_symlink: bool,
    is_file: bool,
    owner: u32,
    canonical: &Path,
    app_data: &Path,
    peer: u32,
) -> bool {
    if is_symlink || !is_file || owner != peer || peer == 0 {
        return false;
    }
    directory_contains(canonical, app_data)
}

pub fn directory_allowed(
    is_symlink: bool,
    is_dir: bool,
    canonical: &Path,
    app_data: &Path,
) -> bool {
    !is_symlink && is_dir && directory_contains(canonical, app_data)
}

fn directory_contains(canonical: &Path, app_data: &Path) -> bool {
    if app_data.as_os_str().is_empty() || app_data == Path::new("/") {
        return false;
    }
    canonical.starts_with(app_data) && canonical != app_data
}

pub fn sidecar_name_allowed(name: &str) -> bool {
    if name == "xray" || name == "xray.exe" {
        return true;
    }
    let Some(rest) = name.strip_prefix("xray-") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

pub fn sidecar_allowed(is_symlink: bool, is_file: bool, executable: bool, name: &str) -> bool {
    !is_symlink && is_file && executable && sidecar_name_allowed(name)
}

pub fn helper_needs_reinstall(local_hash: &str, remote_protocol: u32, remote_hash: &str) -> bool {
    local_hash.is_empty() || remote_protocol != super::PROTOCOL || remote_hash != local_hash
}

pub fn osascript_was_cancelled(stderr: &str, stdout: &str) -> bool {
    let text = format!("{stderr}\n{stdout}");
    text.contains("User canceled") || text.contains("(-128)") || text.contains("error -128")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn config_must_sit_inside_app_data_and_be_owned_by_the_peer() {
        let app = Path::new("/Users/ada/Library/Application Support/io.github.xraybox");
        let config = app.join("run/config.json");
        assert!(config_path_allowed(false, true, 501, &config, app, 501));
        assert!(!config_path_allowed(true, true, 501, &config, app, 501));
        assert!(!config_path_allowed(false, true, 0, &config, app, 501));
        assert!(!config_path_allowed(false, true, 501, &config, app, 502));
        assert!(!config_path_allowed(
            false,
            true,
            501,
            Path::new("/etc/passwd"),
            app,
            501
        ));
        assert!(!config_path_allowed(
            false,
            true,
            501,
            &config,
            Path::new("/"),
            501
        ));
    }

    #[test]
    fn directories_reject_the_app_data_root_itself() {
        let app = Path::new("/Users/ada/Library/Application Support/io.github.xraybox");
        assert!(directory_allowed(false, true, &app.join("run"), app));
        assert!(!directory_allowed(false, true, app, app));
        assert!(!directory_allowed(true, true, &app.join("assets"), app));
    }

    #[test]
    fn sidecar_names_are_xray_or_a_triple() {
        assert!(sidecar_name_allowed("xray"));
        assert!(sidecar_name_allowed("xray-aarch64-apple-darwin"));
        assert!(sidecar_name_allowed("xray.exe"));
        assert!(!sidecar_name_allowed("bash"));
        assert!(!sidecar_name_allowed("xray-../evil"));
        assert!(!sidecar_allowed(true, true, true, "xray"));
        assert!(!sidecar_allowed(false, true, false, "xray"));
        assert!(sidecar_allowed(
            false,
            true,
            true,
            "xray-aarch64-apple-darwin"
        ));
    }

    #[test]
    fn reinstall_when_the_hash_or_protocol_differs() {
        assert!(!helper_needs_reinstall(
            "abc",
            super::super::PROTOCOL,
            "abc"
        ));
        assert!(helper_needs_reinstall("abc", super::super::PROTOCOL, "def"));
        assert!(helper_needs_reinstall("abc", 0, "abc"));
        assert!(helper_needs_reinstall("", super::super::PROTOCOL, ""));
    }

    #[test]
    fn cancel_detection_matches_osascript() {
        assert!(osascript_was_cancelled(
            "0:128: execution error: User canceled. (-128)",
            ""
        ));
        assert!(!osascript_was_cancelled("install failed", ""));
    }
}
