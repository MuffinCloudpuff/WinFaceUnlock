mod commands;
mod provider_registry;
mod service_manager;
mod service_registry;

fn main() {
    if let Err(error) = commands::run_from_args(std::env::args()) {
        eprintln!("winfaceunlock installer failed: {error}");
        std::process::exit(1);
    }
}
