mod commands;
mod installation;
mod provider_registry;
mod resource_directory;
mod service_manager;
mod service_registry;

fn main() {
    if let Err(error) = commands::run_from_args(std::env::args()) {
        eprintln!("winfaceunlock installer failed: {error}");
        std::process::exit(1);
    }
}
