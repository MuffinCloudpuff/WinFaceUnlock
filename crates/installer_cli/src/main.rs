mod commands;
mod service_manager;

fn main() {
    if let Err(error) = commands::run_from_args(std::env::args()) {
        eprintln!("winfaceunlock installer failed: {error}");
        std::process::exit(1);
    }
}
