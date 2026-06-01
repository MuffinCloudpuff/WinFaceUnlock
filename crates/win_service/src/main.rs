fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = if args.iter().any(|arg| arg == "--service") {
        win_service::service_host::run_service_dispatcher()
            .map_err(|_| common_protocol::ProtocolError::TransportUnavailable)
    } else {
        win_service::console_host::run_from_args(args)
    };
    if let Err(error) = result {
        eprintln!("WinFaceUnlockService failed: {error:?}");
        std::process::exit(1);
    }
}
