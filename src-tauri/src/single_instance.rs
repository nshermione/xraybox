use crate::engine::windows::{CONNECT_AFTER_ELEVATE_FLAG, SKIP_ELEVATE_FLAG};

/// Named mutex so a second launch can see the first process before any window exists.
#[cfg(windows)]
pub const MUTEX_NAME: &str = "Local\\io.github.xraybox.single-instance";
/// Named event the first process waits on so a second launch can unhide the tray window.
#[cfg(windows)]
pub const SHOW_EVENT_NAME: &str = "Local\\io.github.xraybox.show";
#[cfg(windows)]
pub const HANDOFF_WAIT_MS: u32 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleInstancePlan {
    TakeOwnership,
    WaitForHandoff,
    FocusExisting,
}

pub fn is_elevation_handoff<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| {
        let arg = arg.as_ref();
        arg == SKIP_ELEVATE_FLAG || arg == CONNECT_AFTER_ELEVATE_FLAG
    })
}

pub fn plan(mutex_exists: bool, handoff: bool) -> SingleInstancePlan {
    if !mutex_exists {
        SingleInstancePlan::TakeOwnership
    } else if handoff {
        SingleInstancePlan::WaitForHandoff
    } else {
        SingleInstancePlan::FocusExisting
    }
}

pub fn claim_or_exit() {
    #[cfg(windows)]
    windows_imp::claim_or_exit();
    #[cfg(target_os = "macos")]
    macos_imp::claim_or_exit();
}

pub fn start_show_listener(app: &tauri::AppHandle) {
    #[cfg(windows)]
    windows_imp::start_show_listener(app);
    #[cfg(target_os = "macos")]
    macos_imp::start_show_listener(app);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = app;
    }
}

#[cfg(windows)]
mod windows_imp {
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
        INFINITE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, IsIconic,
        SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };

    use super::SingleInstancePlan;
    use super::{is_elevation_handoff, plan, HANDOFF_WAIT_MS, MUTEX_NAME, SHOW_EVENT_NAME};

    const ERROR_ALREADY_EXISTS: u32 = 183;
    const WINDOW_CLASS: &str = "Tauri Window";
    const WINDOW_TITLE: &str = "xraybox";

    static MUTEX_HANDLE: OnceLock<isize> = OnceLock::new();
    static SHOW_EVENT: OnceLock<isize> = OnceLock::new();

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn store_handle(slot: &OnceLock<isize>, handle: HANDLE) {
        let _ = slot.set(handle as isize);
    }

    fn create_show_event() {
        let name = wide(SHOW_EVENT_NAME);
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        if handle.is_null() {
            eprintln!("xraybox-boot: single-instance show event failed");
            return;
        }
        store_handle(&SHOW_EVENT, handle);
    }

    fn signal_existing() {
        let name = wide(SHOW_EVENT_NAME);
        let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if !event.is_null() {
            let _ = unsafe { SetEvent(event) };
            unsafe {
                CloseHandle(event);
            }
        }
        focus_existing_window();
    }

    fn focus_existing_window() {
        let class = wide(WINDOW_CLASS);
        let title = wide(WINDOW_TITLE);
        let mut hwnd = unsafe { FindWindowW(class.as_ptr(), title.as_ptr()) };
        if hwnd.is_null() {
            hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        }
        if hwnd.is_null() {
            return;
        }
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != 0 {
                let _ = AllowSetForegroundWindow(pid);
            }
            if IsIconic(hwnd) != 0 {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }

    fn wait_for_handoff(mutex: HANDLE) -> bool {
        let result = unsafe { WaitForSingleObject(mutex, HANDOFF_WAIT_MS) };
        result == WAIT_OBJECT_0 || result == WAIT_ABANDONED
    }

    pub fn claim_or_exit() {
        let name = wide(MUTEX_NAME);
        let mutex = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        if mutex.is_null() {
            eprintln!("xraybox-boot: single-instance mutex failed; continuing");
            return;
        }

        let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let handoff = is_elevation_handoff(std::env::args());
        match plan(exists, handoff) {
            SingleInstancePlan::TakeOwnership => {
                store_handle(&MUTEX_HANDLE, mutex);
                create_show_event();
                eprintln!("xraybox-boot: single instance claimed");
            }
            SingleInstancePlan::WaitForHandoff => {
                if !wait_for_handoff(mutex) {
                    eprintln!("xraybox-boot: elevation handoff timed out; exiting");
                    signal_existing();
                    std::process::exit(0);
                }
                store_handle(&MUTEX_HANDLE, mutex);
                create_show_event();
                eprintln!("xraybox-boot: single instance taken over after elevation");
            }
            SingleInstancePlan::FocusExisting => {
                eprintln!("xraybox-boot: existing instance focused, exiting");
                signal_existing();
                unsafe {
                    CloseHandle(mutex);
                }
                std::process::exit(0);
            }
        }
    }

    pub fn start_show_listener(app: &tauri::AppHandle) {
        let Some(&event) = SHOW_EVENT.get() else {
            return;
        };
        let handle = app.clone();
        std::thread::Builder::new()
            .name("xraybox-single-instance".into())
            .spawn(move || loop {
                let result = unsafe { WaitForSingleObject(event as HANDLE, INFINITE) };
                if result != WAIT_OBJECT_0 {
                    break;
                }
                let shown = handle.clone();
                let _ = handle.run_on_main_thread(move || crate::tray::show_main(&shown));
            })
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::autostart::AUTOSTART_FLAG;
    use crate::engine::windows::{CONNECT_AFTER_ELEVATE_FLAG, SKIP_ELEVATE_FLAG};

    #[test]
    fn first_launch_takes_ownership() {
        assert_eq!(plan(false, false), SingleInstancePlan::TakeOwnership);
    }

    #[test]
    fn second_launch_focuses_existing() {
        assert_eq!(plan(true, false), SingleInstancePlan::FocusExisting);
    }

    #[test]
    fn elevation_handoff_waits_when_parent_still_holds_mutex() {
        assert_eq!(plan(true, true), SingleInstancePlan::WaitForHandoff);
    }

    #[test]
    fn elevation_handoff_takes_ownership_if_parent_already_exited() {
        assert_eq!(plan(false, true), SingleInstancePlan::TakeOwnership);
    }

    #[test]
    fn skip_elevate_is_handoff() {
        assert!(is_elevation_handoff(["--foo", SKIP_ELEVATE_FLAG]));
    }

    #[test]
    fn connect_after_elevate_is_handoff() {
        assert!(is_elevation_handoff([CONNECT_AFTER_ELEVATE_FLAG]));
    }

    #[test]
    fn autostart_is_not_handoff() {
        assert!(!is_elevation_handoff(["--foo", AUTOSTART_FLAG]));
    }

    #[test]
    fn ordinary_args_are_not_handoff() {
        assert!(!is_elevation_handoff(["--foo"]));
    }
}

#[cfg(target_os = "macos")]
mod macos_imp {
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::unix::io::AsRawFd;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::OnceLock;
    use std::time::Duration;

    static LOCK_FILE: OnceLock<std::fs::File> = OnceLock::new();

    fn uid() -> u32 {
        unsafe { libc::getuid() }
    }

    fn lock_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("io.github.xraybox.{}.lock", uid()))
    }

    fn socket_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("io.github.xraybox.{}.sock", uid()))
    }

    pub fn claim_or_exit() {
        let path = lock_path();
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(err) => {
                eprintln!("xraybox-boot: single-instance lock failed: {err}");
                return;
            }
        };
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            let _ = LOCK_FILE.set(file);
            eprintln!("xraybox-boot: single instance claimed");
            return;
        }
        signal_show();
        std::process::exit(0);
    }

    fn signal_show() {
        let path = socket_path();
        for _ in 0..50 {
            if let Ok(mut stream) = UnixStream::connect(&path) {
                let _ = stream.write_all(b"show\n");
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        eprintln!("xraybox-boot: existing instance did not answer");
    }

    pub fn start_show_listener(app: &tauri::AppHandle) {
        let path = socket_path();
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("xraybox-boot: show socket failed: {err}");
                return;
            }
        };
        let handle = app.clone();
        std::thread::Builder::new()
            .name("xraybox-single-instance".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let mut buf = [0u8; 8];
                    let mut stream = stream;
                    let _ = stream.read(&mut buf);
                    let shown = handle.clone();
                    let _ = handle.run_on_main_thread(move || crate::tray::show_main(&shown));
                }
            })
            .ok();
    }
}
