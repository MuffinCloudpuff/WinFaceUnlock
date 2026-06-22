use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

use common_protocol::{ProtocolError, ServiceEvent, ServiceRequest, UserId};
use control_protocol::ControlSettingsPatch;
use control_status::{
    ControlStatusError, FaceTemplateStatusError, WindowsControlSettingsStore,
    WindowsFaceTemplateStatusStore,
};
use ipc::{
    IpcClient, IpcServer, NamedPipeClient, NamedPipeServer, PipeSecurity, ServiceConfigApplier,
    ServiceRequestHandler, SystemUnixTimeMillisClock,
};

use crate::{
    auth_issuer::DevelopmentAuthGrantIssuer,
    credential_resolver::StoreProtectedCredentialResolver,
    credential_store_config::{
        ServiceCredentialStore, ServiceCredentialStorePaths, open_service_credential_store,
    },
    presence_service::PresenceServiceCommand,
    service_config::ServiceAuthConfig,
    service_log::write_service_event_detail,
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
    let mut handler = build_development_handler_without_presence_reload()?;
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
    presence_command_sender: mpsc::Sender<PresenceServiceCommand>,
) -> Result<(), ProtocolError> {
    let mut server = NamedPipeServer::new(pipe_name);
    let mut handler = build_development_handler(presence_command_sender)?;

    server.start(PipeSecurity::service_default())?;
    while !shutdown_requested.load(Ordering::SeqCst) {
        let request = match server.receive() {
            Ok(request) => request,
            Err(error) => {
                if shutdown_requested.load(Ordering::SeqCst) {
                    break;
                }
                eprintln!("{pipe_name} request receive failed; keeping service alive: {error:?}");
                continue;
            }
        };
        let event = match handler.handle_request(request) {
            Ok(event) => event,
            Err(reason) => ServiceEvent::RequestRejected { reason },
        };
        if let Err(error) = server.send(event) {
            if shutdown_requested.load(Ordering::SeqCst) {
                break;
            }
            eprintln!("{pipe_name} response send failed; keeping service alive: {error:?}");
        }
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

pub(crate) type DevelopmentCredentialResolver =
    StoreProtectedCredentialResolver<ServiceCredentialStore>;
pub(crate) type DevelopmentServiceRequestHandler = ServiceRequestHandler<
    DevelopmentAuthGrantIssuer,
    DevelopmentCredentialResolver,
    ServiceFaceTemplateConfigApplier,
    SystemUnixTimeMillisClock,
>;

pub struct ServiceFaceTemplateConfigApplier {
    face_template_store: WindowsFaceTemplateStatusStore,
    settings_store: WindowsControlSettingsStore,
    presence_command_sender: Option<mpsc::Sender<PresenceServiceCommand>>,
}

impl ServiceFaceTemplateConfigApplier {
    fn from_environment_or_default(
        presence_command_sender: Option<mpsc::Sender<PresenceServiceCommand>>,
    ) -> Self {
        Self {
            face_template_store: WindowsFaceTemplateStatusStore::from_environment_or_default(),
            settings_store: WindowsControlSettingsStore::new(),
            presence_command_sender,
        }
    }
}

impl ServiceConfigApplier for ServiceFaceTemplateConfigApplier {
    fn apply_face_template(
        &mut self,
        template_path: &Path,
        camera_id: &str,
    ) -> Result<(), ProtocolError> {
        let install_dir = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .ok_or(ProtocolError::InvalidMessage)?;
        self.face_template_store
            .apply_local_camera_auth_config(template_path, camera_id, &install_dir)
            .map_err(face_template_status_error_to_protocol_error)
    }

    fn apply_control_settings(
        &mut self,
        patch: &ControlSettingsPatch,
    ) -> Result<(), ProtocolError> {
        self.settings_store
            .update_settings(patch)
            .map(|snapshot| {
                write_service_event_detail(
                    "ControlSettings.Applied",
                    format!(
                        "presence_lock_enabled={:?} logon_wake_mode={:?} logon_face_match_threshold={:?} snapshot_presence_lock_enabled={} snapshot_logon_wake_mode={:?} snapshot_logon_face_match_threshold={:.3}",
                        patch.presence_lock_enabled,
                        patch.logon_wake_mode,
                        patch.logon_face_match_threshold,
                        snapshot.presence_lock_enabled,
                        snapshot.logon_wake_mode,
                        snapshot.logon_face_match_threshold
                    ),
                );
                if patch.presence_lock_enabled.is_some()
                    && let Some(sender) = &self.presence_command_sender
                {
                    let _ =
                        sender.send(PresenceServiceCommand::ReloadCurrentSessionFromDesktopControl);
                }
            })
            .map_err(control_status_error_to_protocol_error)
    }
}

pub fn build_development_handler(
    presence_command_sender: mpsc::Sender<PresenceServiceCommand>,
) -> Result<DevelopmentServiceRequestHandler, ProtocolError> {
    build_development_handler_with_paths(
        &ServiceCredentialStorePaths::from_environment_or_default(),
        Some(presence_command_sender),
    )
}

pub fn build_development_handler_without_presence_reload()
-> Result<DevelopmentServiceRequestHandler, ProtocolError> {
    build_development_handler_with_paths(
        &ServiceCredentialStorePaths::from_environment_or_default(),
        None,
    )
}

pub(crate) fn build_development_handler_with_paths(
    paths: &ServiceCredentialStorePaths,
    presence_command_sender: Option<mpsc::Sender<PresenceServiceCommand>>,
) -> Result<DevelopmentServiceRequestHandler, ProtocolError> {
    build_development_handler_with_paths_and_auth_config(
        paths,
        presence_command_sender,
        ServiceAuthConfig::from_environment()?,
    )
}

pub(crate) fn build_development_handler_with_paths_and_auth_config(
    paths: &ServiceCredentialStorePaths,
    presence_command_sender: Option<mpsc::Sender<PresenceServiceCommand>>,
    auth_config: ServiceAuthConfig,
) -> Result<DevelopmentServiceRequestHandler, ProtocolError> {
    let dev_user_id = UserId("dev-user".to_owned());
    let context = open_service_credential_store(paths)?;
    let credential_store = context.store;
    let master_key = context.master_key;

    Ok(ServiceRequestHandler::new(
        DevelopmentAuthGrantIssuer::from_config(dev_user_id, auth_config)?,
        StoreProtectedCredentialResolver::with_master_key(credential_store, master_key),
        ServiceFaceTemplateConfigApplier::from_environment_or_default(presence_command_sender),
        SystemUnixTimeMillisClock,
    ))
}

fn face_template_status_error_to_protocol_error(error: FaceTemplateStatusError) -> ProtocolError {
    match error {
        FaceTemplateStatusError::PermissionDenied(_) => ProtocolError::Unauthorized,
        FaceTemplateStatusError::TemplateConfigMissing(_)
        | FaceTemplateStatusError::TemplateFileMissing(_)
        | FaceTemplateStatusError::TemplateParseFailed(_)
        | FaceTemplateStatusError::TemplateEmpty(_)
        | FaceTemplateStatusError::ServiceConfigUnavailable(_) => ProtocolError::InvalidMessage,
    }
}

fn control_status_error_to_protocol_error(error: ControlStatusError) -> ProtocolError {
    match error {
        ControlStatusError::PermissionDenied(_) | ControlStatusError::ElevationRequired(_) => {
            ProtocolError::Unauthorized
        }
        ControlStatusError::SettingsUnavailable(_)
        | ControlStatusError::SettingsPersistenceFailed(_) => ProtocolError::TransportUnavailable,
        _ => ProtocolError::InvalidMessage,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::mpsc,
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    use common_protocol::{AuthSource, AuthTriggerSource, ServiceEvent, ServiceRequest, SessionId};
    use ipc::{IpcClient, NamedPipeClient};

    use crate::{
        credential_store_config::{
            ensure_development_credential_if_missing, open_service_credential_store,
        },
        service_config::{ServiceAuthConfig, ServiceAuthMode},
    };

    use super::*;

    #[test]
    fn named_pipe_host_handles_health_check_once() -> Result<(), ProtocolError> {
        let pipe_name = unique_pipe_name();
        let (ready_tx, ready_rx) = mpsc::channel();
        let server_pipe_name = pipe_name.clone();

        let server_thread = thread::spawn(move || -> Result<ServiceEvent, ProtocolError> {
            let mut server = NamedPipeServer::new(server_pipe_name);
            let paths = unique_development_store_paths_with_dev_credential()?;
            let mut handler = build_development_handler_with_paths_and_auth_config(
                &paths,
                None,
                manual_test_auth_config(),
            )?;
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
        let paths = unique_development_store_paths_with_dev_credential()?;
        let mut handler = build_development_handler_with_paths_and_auth_config(
            &paths,
            None,
            manual_test_auth_config(),
        )?;

        let event = handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::ManualTest,
            trigger_source: AuthTriggerSource::InputTriggered,
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
            let paths = unique_development_store_paths_with_dev_credential()?;
            let mut handler = build_development_handler_with_paths_and_auth_config(
                &paths,
                None,
                manual_test_auth_config(),
            )?;
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
            trigger_source: AuthTriggerSource::InputTriggered,
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
            ServiceFaceTemplateConfigApplier,
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

    fn unique_development_store_paths_with_dev_credential()
    -> Result<ServiceCredentialStorePaths, ProtocolError> {
        let paths = unique_development_store_paths();
        let context = open_service_credential_store(&paths)?;
        let mut credential_store = context.store;
        ensure_development_credential_if_missing(
            &mut credential_store,
            &context.master_key,
            &UserId("dev-user".to_owned()),
        )?;
        Ok(paths)
    }

    fn manual_test_auth_config() -> ServiceAuthConfig {
        ServiceAuthConfig {
            auth_mode: ServiceAuthMode::ManualTestOnly,
        }
    }
}
