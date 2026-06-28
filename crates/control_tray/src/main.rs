#![cfg_attr(windows, windows_subsystem = "windows")]
#![allow(unsafe_code)]

#[cfg(windows)]
mod windows_tray;

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_tray::run() {
        eprintln!("winfaceunlock tray failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("control_tray is only supported on Windows.");
    std::process::exit(1);
}
