mod console_host;
mod credential_resolver;
mod named_pipe_host;
mod service_host;
mod simulated_auth;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = if args.iter().any(|arg| arg == "--service") {
        service_host::run_service_dispatcher()
            .map_err(|_| common_protocol::ProtocolError::TransportUnavailable)
    } else {
        console_host::run_from_args(args)
    };
    if let Err(error) = result {
        eprintln!("WinFaceUnlockService failed: {error:?}");
        std::process::exit(1);
    }
}
