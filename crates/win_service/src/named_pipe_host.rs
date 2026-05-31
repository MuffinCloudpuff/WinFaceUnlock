use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use common_protocol::{
    AccountType, CredentialRef, ProtocolError, ServiceEvent, ServiceRequest, UserId,
};
use credential_store::{
    CredentialBlob, CredentialBlobAlgorithm, CredentialStore, KeyProtector, MasterKey,
    ProtectedMasterKeyFile, RepositoryCredentialStore, SqlCipherRepository, UserRecord,
    WindowsDpapiKeyProtector,
};
use hardware_binding::HardwareFingerprint;
use ipc::{
    IpcClient, IpcServer, NamedPipeClient, NamedPipeServer, PipeSecurity, ServiceRequestHandler,
    SystemUnixTimeMillisClock,
};

use crate::{
    credential_resolver::StoreProtectedCredentialResolver, simulated_auth::SimulatedAuthGrantIssuer,
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

type DevelopmentCredentialStore = RepositoryCredentialStore<SqlCipherRepository>;
type DevelopmentCredentialResolver = StoreProtectedCredentialResolver<DevelopmentCredentialStore>;
type DevelopmentServiceRequestHandler = ServiceRequestHandler<
    SimulatedAuthGrantIssuer,
    DevelopmentCredentialResolver,
    SystemUnixTimeMillisClock,
>;

pub fn build_development_handler() -> Result<DevelopmentServiceRequestHandler, ProtocolError> {
    build_development_handler_with_paths(&DevelopmentCredentialStorePaths::default())
}

struct DevelopmentCredentialStorePaths {
    master_key_path: PathBuf,
    database_path: PathBuf,
}

impl DevelopmentCredentialStorePaths {
    fn default() -> Self {
        Self {
            master_key_path: std::env::temp_dir()
                .join("winfaceunlock-dev-protected-master-key.bin"),
            database_path: std::env::temp_dir().join("winfaceunlock-dev-credential-store.db"),
        }
    }
}

fn build_development_handler_with_paths(
    paths: &DevelopmentCredentialStorePaths,
) -> Result<DevelopmentServiceRequestHandler, ProtocolError> {
    let dev_user_id = UserId("dev-user".to_owned());
    let master_key = load_or_create_development_master_key(&paths.master_key_path)?;
    let repository = SqlCipherRepository::open(&paths.database_path, &master_key)
        .map_err(|_| ProtocolError::TransportUnavailable)?;
    let mut credential_store = RepositoryCredentialStore::new(repository);
    credential_store
        .initialize(&HardwareFingerprint::empty())
        .map_err(|_| ProtocolError::TransportUnavailable)?;
    credential_store
        .save_encrypted_credential_blob(
            CredentialRef("dev-credential-ref".to_owned()),
            development_placeholder_credential_blob(),
        )
        .map_err(|_| ProtocolError::TransportUnavailable)?;
    credential_store
        .upsert_user(UserRecord {
            user_id: dev_user_id.clone(),
            user_sid: "S-1-5-21-dev-user".to_owned(),
            username: "dev-user".to_owned(),
            account_type: AccountType::Local,
            credential_ref: CredentialRef("dev-credential-ref".to_owned()),
        })
        .map_err(|_| ProtocolError::TransportUnavailable)?;

    Ok(ServiceRequestHandler::new(
        SimulatedAuthGrantIssuer::for_user(dev_user_id),
        StoreProtectedCredentialResolver::new(credential_store),
        SystemUnixTimeMillisClock,
    ))
}

fn development_placeholder_credential_blob() -> CredentialBlob {
    CredentialBlob::new(
        CredentialBlobAlgorithm::Aes256GcmV1,
        vec![1; 12],
        vec![2; 16],
    )
}

fn load_or_create_development_master_key(
    master_key_path: &Path,
) -> Result<MasterKey, ProtocolError> {
    let key_file = ProtectedMasterKeyFile::new(master_key_path.to_path_buf());
    let key_protector = WindowsDpapiKeyProtector::new();

    if key_file.path().exists() {
        let protected = key_file
            .load()
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        key_protector
            .unprotect_master_key(&protected)
            .map_err(|_| ProtocolError::TransportUnavailable)
    } else {
        let master_key = key_protector
            .generate_master_key()
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        let protected = key_protector
            .protect_master_key(&master_key)
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        key_file
            .save(&protected)
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        Ok(master_key)
    }
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
            SimulatedAuthGrantIssuer,
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

    fn unique_development_store_paths() -> DevelopmentCredentialStorePaths {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let prefix = format!("winfaceunlock-dev-test-{}-{nanos}", std::process::id());
        DevelopmentCredentialStorePaths {
            master_key_path: std::env::temp_dir().join(format!("{prefix}-master-key.bin")),
            database_path: std::env::temp_dir().join(format!("{prefix}-store.db")),
        }
    }
}
