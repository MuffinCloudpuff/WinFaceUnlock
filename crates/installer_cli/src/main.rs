mod commands;
mod installation;
mod provider_registry;
mod resource_directory;
mod service_manager;
mod service_registry;
mod setup_backend;
mod setup_install;
mod setup_payload;
mod setup_preflight;
mod setup_presence;
mod user_startup;

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).map(String::as_str) == Some("setup-backend") {
        if let Err(error) = setup_backend::run_from_stdio() {
            eprintln!("winfaceunlock setup backend failed: {error}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(error) = commands::run_from_args(args) {
        eprintln!("winfaceunlock installer failed: {error}");
        std::process::exit(1);
    }
}
