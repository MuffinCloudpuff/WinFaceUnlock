use std::sync::atomic::{AtomicBool, Ordering};

use common_protocol::{ProtocolError, ServiceEvent, ServiceRequest, UserId};
use ipc::{
    IpcClient, IpcServer, NamedPipeClient, NamedPipeServer, PipeSecurity, ServiceRequestHandler,
    SystemUnixTimeMillisClock,
};

use crate::{
    auth_issuer::DevelopmentAuthGrantIssuer,
    credential_resolver::StoreProtectedCredentialResolver,
    credential_store_config::{
        ServiceCredentialStore, ServiceCredentialStorePaths,
        ensure_development_credential_if_missing, open_service_credential_store,
    },
};

pub fn run_named_pipe_once(pipe_name: &str) -> Result<ServiceEvent, ProtocolError> {
    let mut events = run_named_pipe_requests(pipe_name, 1)?;
    events.pop().ok_or(ProtocolError::TransportUnavailable)
}

pub fn run_named_pipe_requests(
    pipe_name: &str,
    request_limit: usize,
) -> Result<Vec<ServiceEvent>, ProtocolError> {
    let mut server = NamedPipeServer::new(pipe_name);
    let mut handler = build_development_handler()?;
    let mut events = Vec::with_capacity(request_limit);

    server.start(PipeSecurity::service_default())?;
    for _ in 0..request_limit {
        let request = server.receive()?;
        let event = match handler.handle_request(request) {
            Ok(event) => event,
            Err(reason) => ServiceEvent::RequestRejected { reason },
        };
        server.send(event.clone())?;
        events.push(event);
    }
    server.stop();

    Ok(events)
}

pub fn run_named_pipe_until_shutdown(
    pipe_name: &str,
    shutdown_requested: &AtomicBool,
) -> Result<(), ProtocolError> {
    let mut server = NamedPipeServer::new(pipe_name);
    let mut handler = build_development_handler()?;

    server.start(PipeSecurity::service_default())?;
    while !shutdown_requested.load(Ordering::SeqCst) {
        let request = server.receive()?;
        let event = match handler.handle_request(request) {
            Ok(event) => event,
            Err(reason) => ServiceEvent::RequestRejected { reason },
        };
        server.send(event)?;
    }
    server.stop();

    Ok(())
}

pub fn wake_named_pipe_host(pipe_name: &str) -> Result<(), ProtocolError> {
    let mut client = NamedPipeClient::new(pipe_name);
    client.connect()?;
    let _event = client.request(ServiceRequest::HealthCheck)?;
    client.disconnect();
    Ok(())
}

type DevelopmentCredentialResolver = StoreProtectedCredentialResolver<ServiceCredentialStore>;
type DevelopmentServiceRequestHandler = ServiceRequestHandler<
    DevelopmentAuthGrantIssuer,
    DevelopmentCredentialResolver,
    SystemUnixTimeMillisClock,
>;

pub fn build_development_handler() -> Result<DevelopmentServiceRequestHandler, ProtocolError> {
    build_development_handler_with_paths(&ServiceCredentialStorePaths::from_environment_or_default())
}

fn build_development_handler_with_paths(
    paths: &ServiceCredentialStorePaths,
) -> Result<DevelopmentServiceRequestHandler, ProtocolError> {
    let dev_user_id = UserId("dev-user".to_owned());
    let context = open_service_credential_store(paths)?;
    let mut credential_store = context.store;
    let master_key = context.master_key;
    ensure_development_credential_if_missing(&mut credential_store, &master_key, &dev_user_id)?;

    Ok(ServiceRequestHandler::new(
        DevelopmentAuthGrantIssuer::from_environment(dev_user_id)?,
        StoreProtectedCredentialResolver::with_master_key(credential_store, master_key),
        SystemUnixTimeMillisClock,
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        sync::mpsc,
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    use common_protocol::{AuthSource, ServiceEvent, ServiceRequest, SessionId};
    use ipc::{IpcClient, NamedPipeClient};

    use super::*;

    #[test]
    fn named_pipe_host_handles_health_check_once() -> Result<(), ProtocolError> {
        let pipe_name = unique_pipe_name();
        let (ready_tx, ready_rx) = mpsc::channel();
        let server_pipe_name = pipe_name.clone();

        let server_thread = thread::spawn(move || -> Result<ServiceEvent, ProtocolError> {
            let mut server = NamedPipeServer::new(server_pipe_name);
            let paths = unique_development_store_paths();
            let mut handler = build_development_handler_with_paths(&paths)?;
            server.start(PipeSecurity::service_default())?;
            ready_tx
                .send(())
                .map_err(|_| ProtocolError::TransportUnavailable)?;
            let events = run_server_requests(&mut server, &mut handler, 1)?;
            Ok(events[0].clone())
        });

        ready_rx
            .recv()
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        let mut client = NamedPipeClient::new(pipe_name);
        client.connect()?;
        let event = client.request(ServiceRequest::HealthCheck)?;
        client.disconnect();

        let server_event = server_thread
            .join()
            .map_err(|_| ProtocolError::TransportUnavailable)??;
        assert_eq!(event, ServiceEvent::HealthOk);
        assert_eq!(server_event, ServiceEvent::HealthOk);
        Ok(())
    }

    #[test]
    fn development_handler_can_issue_manual_test_grant() -> Result<(), ProtocolError> {
        let paths = unique_development_store_paths();
        let mut handler = build_development_handler_with_paths(&paths)?;

        let event = handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::ManualTest,
        })?;

        assert!(matches!(event, ServiceEvent::AuthSucceeded { .. }));
        Ok(())
    }

    #[test]
    fn named_pipe_host_keeps_grant_state_across_requests() -> Result<(), ProtocolError> {
        let pipe_name = unique_pipe_name();
        let (ready_tx, ready_rx) = mpsc::channel();
        let server_pipe_name = pipe_name.clone();

        let server_thread = thread::spawn(move || -> Result<Vec<ServiceEvent>, ProtocolError> {
            let mut server = NamedPipeServer::new(server_pipe_name);
            let paths = unique_development_store_paths();
            let mut handler = build_development_handler_with_paths(&paths)?;
            server.start(PipeSecurity::service_default())?;
            ready_tx
                .send(())
                .map_err(|_| ProtocolError::TransportUnavailable)?;
            run_server_requests(&mut server, &mut handler, 3)
        });

        ready_rx
            .recv()
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        let mut client = NamedPipeClient::new(pipe_name.clone());
        client.connect()?;
        let wake_event = client.request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::ManualTest,
        })?;
        client.disconnect();
        let ServiceEvent::AuthSucceeded { grant } = wake_event else {
            return Err(ProtocolError::InvalidMessage);
        };

        let mut fetch_client = NamedPipeClient::new(pipe_name.clone());
        fetch_client.connect()?;
        let credential_event = fetch_client.request(ServiceRequest::FetchCredential {
            session_id: grant.session_id.clone(),
            grant_id: grant.grant_id.clone(),
            nonce: grant.nonce.clone(),
        })?;
        fetch_client.disconnect();

        let mut replay_client = NamedPipeClient::new(pipe_name);
        replay_client.connect()?;
        let replay_event = replay_client.request(ServiceRequest::FetchCredential {
            session_id: grant.session_id.clone(),
            grant_id: grant.grant_id.clone(),
            nonce: grant.nonce.clone(),
        })?;
        replay_client.disconnect();

        let server_events = server_thread
            .join()
            .map_err(|_| ProtocolError::TransportUnavailable)??;
        assert_eq!(server_events.len(), 3);
        assert!(matches!(
            credential_event,
            ServiceEvent::CredentialReady { .. }
        ));
        assert_eq!(
            replay_event,
            ServiceEvent::RequestRejected {
                reason: ProtocolError::UsedGrant,
            }
        );
        Ok(())
    }

    fn run_server_requests(
        server: &mut NamedPipeServer,
        handler: &mut ServiceRequestHandler<
            DevelopmentAuthGrantIssuer,
            DevelopmentCredentialResolver,
            SystemUnixTimeMillisClock,
        >,
        request_limit: usize,
    ) -> Result<Vec<ServiceEvent>, ProtocolError> {
        let mut events = Vec::with_capacity(request_limit);
        for _ in 0..request_limit {
            let request = server.receive()?;
            let event = match handler.handle_request(request) {
                Ok(event) => event,
                Err(reason) => ServiceEvent::RequestRejected { reason },
            };
            server.send(event.clone())?;
            events.push(event);
        }
        server.stop();
        Ok(events)
    }

    fn unique_pipe_name() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        format!(
            r"\\.\pipe\winfaceunlock-service-test-{}-{nanos}",
            std::process::id()
        )
    }

    fn unique_development_store_paths() -> ServiceCredentialStorePaths {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let prefix = format!("winfaceunlock-dev-test-{}-{nanos}", std::process::id());
        ServiceCredentialStorePaths::from_store_dir(std::env::temp_dir().join(prefix))
    }
}
