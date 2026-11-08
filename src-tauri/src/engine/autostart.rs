use std::path::Path;
use std::process::Command;

use crate::engine::error::EngineError;
#[cfg(windows)]
use crate::engine::windows::apply_hidden_console;

pub const AUTOSTART_FLAG: &str = "--xraybox-autostart";
pub const TASK_NAME: &str = "Xraybox";

pub fn autostart_requested() -> bool {
    args_request_autostart(std::env::args())
}

pub fn args_request_autostart<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| arg.as_ref() == AUTOSTART_FLAG)
}

pub fn create_task_args(exe: &Path) -> Vec<String> {
    vec![
        "/Create".into(),
        "/TN".into(),
        TASK_NAME.into(),
        "/TR".into(),
        format!("\"{}\" {AUTOSTART_FLAG}", exe.display()),
        "/SC".into(),
        "ONLOGON".into(),
        "/RL".into(),
        "HIGHEST".into(),
        "/F".into(),
    ]
}

pub fn delete_task_args() -> Vec<String> {
    vec![
        "/Delete".into(),
        "/TN".into(),
        TASK_NAME.into(),
        "/F".into(),
    ]
}

/// Android BootReceiver and Windows task scheduler both use this rule:
/// restore only when start-on-boot is on, and either the tunnel was still
/// marked desired or auto-connect has a last successful start request.
pub fn should_restore_on_boot(
    start_on_boot: bool,
    auto_connect: bool,
    has_desired: bool,
    has_last: bool,
) -> bool {
    start_on_boot && (has_desired || (auto_connect && has_last))
}

pub fn apply_start_on_boot(enabled: bool) -> Result<(), EngineError> {
    #[cfg(windows)]
    {
        if enabled {
            let exe = std::env::current_exe().map_err(|err| {
                EngineError::Process(format!("cannot resolve current exe: {err}"))
            })?;
            run_schtasks(&create_task_args(&exe))
        } else {
            match run_schtasks(&delete_task_args()) {
                Ok(()) => Ok(()),
                Err(err) if task_missing(&err) => Ok(()),
                Err(err) => Err(err),
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        apply_macos_launch_agent(enabled)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = enabled;
        Ok(())
    }
}

pub const LAUNCH_AGENT_LABEL: &str = "io.github.xraybox.autostart";

pub fn launch_agent_plist(exe: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LAUNCH_AGENT_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>{AUTOSTART_FLAG}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
        exe = xml_escape(exe)
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(target_os = "macos")]
fn apply_macos_launch_agent(enabled: bool) -> Result<(), EngineError> {
    let home = std::env::var("HOME").map_err(|_| EngineError::Process("HOME is not set".into()))?;
    let path = Path::new(&home)
        .join("Library/LaunchAgents")
        .join("io.github.xraybox.autostart.plist");
    let uid = unsafe { libc::getuid() };
    let domain = format!("gui/{uid}");
    if let Some(text) = path.to_str() {
        let _ = Command::new("/bin/launchctl")
            .args(["bootout", &domain, text])
            .status();
    }
    if !enabled {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let exe = std::env::current_exe()
        .map_err(|err| EngineError::Process(format!("cannot resolve current exe: {err}")))?;
    std::fs::write(&path, launch_agent_plist(&exe.to_string_lossy()))?;
    let text = path.to_string_lossy().into_owned();
    let output = Command::new("/bin/launchctl")
        .args(["bootstrap", &domain, &text])
        .output()
        .map_err(|err| EngineError::Process(format!("start on boot failed: {err}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(EngineError::Process(format!(
            "start on boot failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(windows)]
fn run_schtasks(args: &[String]) -> Result<(), EngineError> {
    let mut cmd = Command::new("schtasks");
    cmd.args(args);
    apply_hidden_console(&mut cmd);
    let output = cmd
        .output()
        .map_err(|err| EngineError::Process(format!("start on boot failed: {err}")))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(EngineError::Process(format!(
            "start on boot failed: {} {}",
            stderr.trim(),
            stdout.trim()
        )))
    }
}

#[cfg(windows)]
fn task_missing(err: &EngineError) -> bool {
    let text = err.to_string().to_ascii_lowercase();
    text.contains("cannot find") || text.contains("the system cannot find")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn create_task_runs_elevated_on_logon() {
        let exe = PathBuf::from(r"C:\Program Files\xraybox\xraybox.exe");
        let args = create_task_args(&exe);
        assert!(args.contains(&"/Create".into()));
        assert!(args.contains(&TASK_NAME.into()));
        assert!(args.contains(&"ONLOGON".into()));
        assert!(args.contains(&"HIGHEST".into()));
        let tr = args
            .windows(2)
            .find(|pair| pair[0] == "/TR")
            .map(|pair| pair[1].as_str())
            .unwrap();
        assert!(tr.contains(AUTOSTART_FLAG));
        assert!(tr.contains("xraybox.exe"));
    }

    #[test]
    fn delete_task_force_removes_named_task() {
        assert_eq!(delete_task_args(), vec!["/Delete", "/TN", TASK_NAME, "/F"]);
    }

    #[test]
    fn autostart_flag_is_detected() {
        assert!(args_request_autostart(["--foo", AUTOSTART_FLAG]));
        assert!(!args_request_autostart(["--foo"]));
    }

    #[test]
    fn restore_on_boot_needs_start_on_boot() {
        assert!(!should_restore_on_boot(false, true, true, true));
        assert!(should_restore_on_boot(true, false, true, false));
        assert!(should_restore_on_boot(true, true, false, true));
        assert!(!should_restore_on_boot(true, true, false, false));
        assert!(!should_restore_on_boot(true, false, false, true));
    }

    #[test]
    fn launch_agent_plist_hides_on_login() {
        let plist = launch_agent_plist("/Applications/xraybox.app/Contents/MacOS/xraybox");
        assert!(plist.contains(LAUNCH_AGENT_LABEL));
        assert!(plist.contains(AUTOSTART_FLAG));
        assert!(plist.contains("/Applications/xraybox.app/Contents/MacOS/xraybox"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        let escaped = launch_agent_plist("/tmp/a&b");
        assert!(escaped.contains("/tmp/a&amp;b"));
        assert!(!escaped.contains("/tmp/a&b<"));
    }
}
