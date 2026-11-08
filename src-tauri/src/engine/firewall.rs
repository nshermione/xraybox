#![cfg_attr(not(windows), allow(dead_code))]

use std::path::Path;
use std::process::{Command, Output};

use crate::engine::error::EngineError;
use crate::engine::windows::apply_hidden_console;

/// netsh rejects Windows verbatim prefixes (`\\?\`) as invalid program characters.
pub fn program_path(program: &Path) -> String {
    let raw = program.to_string_lossy();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        raw.into_owned()
    }
}

pub fn delete_rule_args(name: &str) -> Vec<String> {
    vec![
        "advfirewall".into(),
        "firewall".into(),
        "delete".into(),
        "rule".into(),
        format!("name={name}"),
    ]
}

/// Every rule this app owns is created delete-first so a stale copy from a
/// previous run cannot accumulate duplicates.
pub fn add_rule_args(name: &str, extra: &[(&str, &str)]) -> Vec<String> {
    let mut args = vec![
        "advfirewall".into(),
        "firewall".into(),
        "add".into(),
        "rule".into(),
        format!("name={name}"),
    ];
    args.extend(extra.iter().map(|(key, value)| format!("{key}={value}")));
    args.push("enable=yes".into());
    args
}

pub fn run(args: &[String]) -> Result<(), EngineError> {
    let output = run_raw(args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(error_from(args, &output))
    }
}

pub fn run_output(args: &[String]) -> Result<String, EngineError> {
    let output = run_raw(args)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(error_from(args, &output))
    }
}

/// Deleting a rule that was never added exits non-zero. Callers use this when
/// absence is the desired end state, so the exit code carries no information.
pub fn delete_rule_best_effort(name: &str) {
    let _ = run(&delete_rule_args(name));
}

fn run_raw(args: &[String]) -> Result<Output, EngineError> {
    let mut cmd = Command::new("netsh");
    cmd.args(args);
    apply_hidden_console(&mut cmd);
    cmd.output()
        .map_err(|err| EngineError::Process(format!("netsh failed: {err}")))
}

fn error_from(args: &[String], output: &Output) -> EngineError {
    EngineError::Process(format!(
        "netsh {} failed: {} {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim(),
        String::from_utf8_lossy(&output.stdout).trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_path_strips_windows_verbatim_prefix() {
        assert_eq!(
            program_path(Path::new(r"\\?\D:\projects\xraybox\xray.exe")),
            r"D:\projects\xraybox\xray.exe"
        );
    }

    #[test]
    fn program_path_strips_unc_verbatim_prefix() {
        assert_eq!(
            program_path(Path::new(r"\\?\UNC\fileserver\share\xray.exe")),
            r"\\fileserver\share\xray.exe"
        );
    }

    #[test]
    fn program_path_leaves_plain_paths_alone() {
        assert_eq!(
            program_path(Path::new(r"C:\app\xray.exe")),
            r"C:\app\xray.exe"
        );
    }

    #[test]
    fn add_rule_puts_name_first_and_enable_last() {
        let args = add_rule_args("Some Rule", &[("dir", "out"), ("action", "block")]);
        assert_eq!(args[0], "advfirewall");
        assert_eq!(args[1], "firewall");
        assert_eq!(args[2], "add");
        assert_eq!(args[3], "rule");
        assert_eq!(args[4], "name=Some Rule");
        assert_eq!(args[5], "dir=out");
        assert_eq!(args[6], "action=block");
        assert_eq!(args.last().unwrap(), "enable=yes");
    }

    #[test]
    fn delete_rule_targets_the_name_only() {
        let args = delete_rule_args("Some Rule");
        assert!(args.contains(&"delete".to_string()));
        assert_eq!(args.last().unwrap(), "name=Some Rule");
    }
}
