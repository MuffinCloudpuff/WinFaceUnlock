use common_protocol::{
    AuthGrant, AuthSource, PIPE_NAME, ProtectedCredential, ProtocolError, SERVICE_NAME,
    ServiceEvent, ServiceRequest, SessionId,
};

use crate::named_pipe_host::{
    build_development_handler, run_named_pipe_once, run_named_pipe_requests,
};

#[derive(Clone, Debug, PartialEq)]
pub struct ConsoleSmokeReport {
    pub service_name: &'static str,
    pub health_event: ServiceEvent,
    pub issued_grant: AuthGrant,
    pub protected_credential: ProtectedCredential,
}

pub fn run_from_args(args: impl IntoIterator<Item = String>) -> Result<(), ProtocolError> {
    let args: Vec<String> = args.into_iter().collect();
    let should_run_smoke = args.iter().any(|arg| arg == "--console-smoke");
    let should_run_pipe_once = args
        .iter()
        .any(|arg| arg == "--pipe-once" || arg == "--console");
    let pipe_name = argument_value(&args, "--pipe-name").unwrap_or(PIPE_NAME);
    let pipe_request_limit = argument_value(&args, "--pipe-requests")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);

    if should_run_smoke {
        let report = run_console_smoke()?;
        print_console_smoke_report(&report);
    } else if should_run_pipe_once {
        if pipe_request_limit == 1 {
            println!("{SERVICE_NAME} named pipe host waiting on {pipe_name}");
            let event = run_named_pipe_once(pipe_name)?;
            println!("{SERVICE_NAME} named pipe host handled event: {event:?}");
        } else {
            println!(
                "{SERVICE_NAME} named pipe host waiting on {pipe_name} for {pipe_request_limit} requests"
            );
            let events = run_named_pipe_requests(pipe_name, pipe_request_limit)?;
            for event in events {
                println!("{SERVICE_NAME} named pipe host handled event: {event:?}");
            }
        }
    } else {
        println!("{SERVICE_NAME} console host");
        println!("Use --service when launched by the Windows Service Control Manager.");
        println!("Use --pipe-once [--pipe-requests N] to handle IPC requests.");
        println!("Use --console-smoke to run the in-process backend protocol smoke test.");
    }

    Ok(())
}

pub fn run_console_smoke() -> Result<ConsoleSmokeReport, ProtocolError> {
    let mut handler = build_development_handler()?;

    let health_event = handler.handle_request(ServiceRequest::HealthCheck)?;
    let issued_grant = match handler.handle_request(ServiceRequest::WakeAuth {
        session_id: SessionId("dev-session".to_owned()),
        source: AuthSource::ManualTest,
    })? {
        ServiceEvent::AuthSucceeded { grant } => grant,
        _ => return Err(ProtocolError::InvalidMessage),
    };
    let protected_credential = match handler.handle_request(ServiceRequest::FetchCredential {
        session_id: SessionId("dev-session".to_owned()),
        grant_id: issued_grant.grant_id.clone(),
        nonce: issued_grant.nonce.clone(),
    })? {
        ServiceEvent::CredentialReady {
            protected_credential,
            ..
        } => protected_credential,
        _ => return Err(ProtocolError::InvalidMessage),
    };

    Ok(ConsoleSmokeReport {
        service_name: SERVICE_NAME,
        health_event,
        issued_grant,
        protected_credential,
    })
}

fn print_console_smoke_report(report: &ConsoleSmokeReport) {
    println!("{} console smoke: ok", report.service_name);
    println!("health_event: {:?}", report.health_event);
    println!("grant_id: {}", report.issued_grant.grant_id.0);
    println!("session_id: {}", report.issued_grant.session_id.0);
    println!(
        "protected_credential_ref: {}",
        report.protected_credential.credential_ref.0
    );
}

fn argument_value<'args>(args: &'args [String], name: &str) -> Option<&'args str> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].as_str())
}

#[cfg(test)]
mod tests {
    use common_protocol::{CredentialRef, GrantId, Nonce};

    use super::*;

    #[test]
    fn console_smoke_completes_without_plaintext_credential() -> Result<(), ProtocolError> {
        let report = run_console_smoke()?;

        assert_eq!(report.health_event, ServiceEvent::HealthOk);
        assert_eq!(
            report.issued_grant.grant_id,
            GrantId("dev-grant-1".to_owned())
        );
        assert_eq!(report.issued_grant.nonce, Nonce("dev-nonce-1".to_owned()));
        assert_eq!(
            report.protected_credential.credential_ref,
            CredentialRef("dev-credential-ref".to_owned())
        );
        Ok(())
    }

    #[test]
    fn argument_value_reads_pipe_name() {
        let args = vec![
            "win_service".to_owned(),
            "--pipe-name".to_owned(),
            r"\\.\pipe\test".to_owned(),
        ];

        assert_eq!(argument_value(&args, "--pipe-name"), Some(r"\\.\pipe\test"));
    }
}
