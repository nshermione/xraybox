use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

use super::process::read_tail;

const RELATIVE: &str = "logs/app.log";
const TAIL_MAX: usize = 96 * 1024;
const PENDING_CAP: usize = 200;

#[cfg(target_os = "android")]
const ANDROID_BOOT_LOG: &str = "/data/data/io.github.xraybox/files/xraybox-boot.log";

static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn init(path: PathBuf) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let pending = PENDING
        .lock()
        .ok()
        .map(|mut lines| std::mem::take(&mut *lines))
        .unwrap_or_default();
    for line in pending {
        let _ = write_line(&path, &line);
    }
    if let Ok(mut slot) = PATH.lock() {
        *slot = Some(path);
    }
}

pub fn append(message: &str) {
    let line = format_line(message);
    if let Ok(slot) = PATH.lock() {
        if let Some(path) = slot.as_ref() {
            let _ = write_line(path, &line);
            return;
        }
    }
    if let Ok(mut pending) = PENDING.lock() {
        if pending.len() < PENDING_CAP {
            pending.push(line);
        }
    }
}

pub fn read_app_log(app: &AppHandle) -> String {
    let mut candidates = Vec::new();
    if let Ok(dir) = app.path().app_data_dir() {
        candidates.push(dir.join(RELATIVE));
    }
    if let Ok(slot) = PATH.lock() {
        if let Some(path) = slot.as_ref() {
            if !candidates.iter().any(|item| item == path) {
                candidates.push(path.clone());
            }
        }
    }
    #[cfg(target_os = "android")]
    {
        candidates.push(PathBuf::from(ANDROID_BOOT_LOG));
    }
    newest_tail(&candidates)
}

fn newest_tail(paths: &[PathBuf]) -> String {
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for path in paths {
        let Ok(meta) = fs::metadata(path) else {
            continue;
        };
        if !meta.is_file() || meta.len() == 0 {
            continue;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(time, _)| modified >= *time) {
            best = Some((modified, path.clone()));
        }
    }
    best.and_then(|(_, path)| read_tail(&path, TAIL_MAX).ok())
        .unwrap_or_default()
}

fn format_line(message: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{}.{:03} {}\n",
        now.as_secs(),
        now.subsec_millis(),
        message.replace('\0', "")
    )
}

fn write_line(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xraybox-app-log-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_line_appends() {
        let dir = temp_dir("write");
        let path = dir.join("app.log");
        write_line(&path, "first\n").unwrap();
        write_line(&path, "second\n").unwrap();
        let body = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert!(body.contains("first"));
        assert!(body.contains("second"));
    }

    #[test]
    fn newest_tail_prefers_newer_file() {
        let dir = temp_dir("newest");
        let older = dir.join("older.log");
        let newer = dir.join("newer.log");
        fs::write(&older, "old line\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(15));
        fs::write(&newer, "new line\n").unwrap();
        let tail = newest_tail(&[older, newer]);
        let _ = fs::remove_dir_all(&dir);
        assert!(tail.contains("new line"));
        assert!(!tail.contains("old line"));
    }
}
