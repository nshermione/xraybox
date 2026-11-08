// GUI subsystem on all builds. Debug/`tauri dev` must not allocate a black
// console (conhost) for this exe. Parent CLI terminals still receive logs.
#![windows_subsystem = "windows"]

fn main() {
    #[cfg(target_os = "macos")]
    if xraybox_lib::helper_requested() {
        xraybox_lib::run_helper();
        return;
    }
    xraybox_lib::run()
}
