mod commands;
mod face_calibration;
mod face_debug_snapshot;
mod liveness_screen_debug;
mod threshold_preview;

fn main() {
    if let Err(error) = commands::run_from_args(std::env::args()) {
        eprintln!("WinFaceUnlock diagnostics failed: {error}");
        std::process::exit(1);
    }
}
