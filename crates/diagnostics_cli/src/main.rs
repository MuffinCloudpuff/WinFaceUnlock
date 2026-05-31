mod commands;

fn main() {
    if let Err(error) = commands::run_from_args(std::env::args()) {
        eprintln!("WinFaceUnlock diagnostics failed: {error}");
        std::process::exit(1);
    }
}
