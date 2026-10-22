use std::{
    ffi::OsString,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use common_protocol::{PIPE_NAME, SERVICE_NAME};
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType, SessionChangeReason,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

use crate::{
    camera_runtime::{
        InterfaceRuntimeState, InterfaceRuntimeStateSource, update_interface_runtime_state,
    },
    named_pipe_host::{run_named_pipe_until_shutdown, wake_named_pipe_host},
    presence_service::{PresenceServiceCommand, spawn_presence_service_controller},
    service_log::write_service_event_detail,
    service_priority::raise_service_priority,
};

define_windows_service!(ffi_service_main, service_main);

pub fn run_service_dispatcher() -> windows_service::Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(error) = run_service_main() {
        eprintln!("{SERVICE_NAME} service host failed: {error:?}");
    }
}

fn run_service_main() -> windows_service::Result<()> {
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let handler_shutdown = Arc::clone(&shutdown_requested);
    let pipe_name = PIPE_NAME.to_owned();
    let presence_command_sender = spawn_presence_service_controller();
    let handler_presence_sender = presence_command_sender.clone();

    let status_handle =
        service_control_handler::register(
            SERVICE_NAME,
            move |control_event| match control_event {
                ServiceControl::Stop => {
                    handler_shutdown.store(true, Ordering::SeqCst);
                    let wake_pipe_name = pipe_name.clone();
                    let _ = thread::Builder::new()
                        .name("winfaceunlock-service-stop-wakeup".to_owned())
                        .spawn(move || {
                            let _ = wake_named_pipe_host(&wake_pipe_name);
                        });
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::SessionChange(session_change) => {
                    let session_id = session_change.notification.session_id;
                    let command = match session_change.reason {
                        SessionChangeReason::SessionLogon => {
                            update_interface_runtime_state(
                                InterfaceRuntimeState::DesktopUnlocked,
                                InterfaceRuntimeStateSource::ServiceSessionLogon,
                            );
                            Some(PresenceServiceCommand::StartForUserSession { session_id })
                        }
                        SessionChangeReason::SessionUnlock => {
                            update_interface_runtime_state(
                                InterfaceRuntimeState::DesktopUnlocked,
                                InterfaceRuntimeStateSource::ServiceSessionUnlock,
                            );
                            Some(PresenceServiceCommand::StartForUserSession { session_id })
                        }
                        SessionChangeReason::SessionLock
                        | SessionChangeReason::SessionLogoff
                        | SessionChangeReason::ConsoleDisconnect
                        | SessionChangeReason::RemoteDisconnect
                        | SessionChangeReason::SessionTerminate => {
                            let source = match session_change.reason {
                                SessionChangeReason::SessionLock => {
                                    InterfaceRuntimeStateSource::ServiceSessionLock
                                }
                                _ => InterfaceRuntimeStateSource::ServiceSessionEnd,
                            };
                            update_interface_runtime_state(
                                InterfaceRuntimeState::LockedOrLogon,
                                source,
                            );
                            Some(PresenceServiceCommand::StopForUserSession { session_id })
                        }
                        _ => None,
                    };
                    if let Some(command) = command {
                        let _ = handler_presence_sender.send(command);
                    }
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            },
        )?;

    status_handle.set_service_status(service_status(
        ServiceState::StartPending,
        ServiceControlAccept::empty(),
        1,
        Duration::from_secs(10),
    ))?;
    match raise_service_priority() {
        Ok(()) => write_service_event_detail("Service.PriorityConfigured", "class=AboveNormal"),
        Err(error) => write_service_event_detail(
            "Service.PriorityConfigurationFailed",
            format!("error={error}"),
        ),
    }
    status_handle.set_service_status(service_status(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SESSION_CHANGE,
        0,
        Duration::default(),
    ))?;

    let service_result = run_named_pipe_until_shutdown(
        PIPE_NAME,
        &shutdown_requested,
        presence_command_sender.clone(),
    );
    let _ = presence_command_sender.send(PresenceServiceCommand::Shutdown);

    status_handle.set_service_status(service_status(
        ServiceState::StopPending,
        ServiceControlAccept::empty(),
        1,
        Duration::from_secs(10),
    ))?;
    if let Err(error) = &service_result {
        write_service_event_detail("Service.PipeHostFailed", format!("error={error:?}"));
        eprintln!("{SERVICE_NAME} named pipe loop stopped with error: {error:?}");
    }
    status_handle.set_service_status(stopped_service_status(service_result.is_err()))?;
    Ok(())
}

fn stopped_service_status(failed: bool) -> ServiceStatus {
    let mut status = service_status(
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
        0,
        Duration::default(),
    );
    if failed {
        // Nonzero status is required for SCM's non-crash recovery actions.
        status.exit_code = ServiceExitCode::ServiceSpecific(1);
    }
    status
}

fn service_status(
    current_state: ServiceState,
    controls_accepted: ServiceControlAccept,
    checkpoint: u32,
    wait_hint: Duration,
) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state,
        controls_accepted,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint,
        wait_hint,
        process_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_host_failure_is_reported_to_scm() {
        let status = stopped_service_status(true);
        assert_eq!(status.current_state, ServiceState::Stopped);
        assert_eq!(status.exit_code, ServiceExitCode::ServiceSpecific(1));
    }

    #[test]
    fn intentional_shutdown_does_not_trigger_recovery() {
        assert_eq!(
            stopped_service_status(false).exit_code,
            ServiceExitCode::Win32(0)
        );
    }
}
