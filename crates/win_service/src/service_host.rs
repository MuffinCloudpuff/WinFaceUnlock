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
    camera_backend_profiles::spawn_camera_backend_profile_refresh,
    named_pipe_host::{run_named_pipe_until_shutdown, wake_named_pipe_host},
    presence_service::{PresenceServiceCommand, spawn_presence_service_controller},
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
    spawn_camera_backend_profile_refresh();

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
                        SessionChangeReason::SessionLogon | SessionChangeReason::SessionUnlock => {
                            Some(PresenceServiceCommand::StartForUserSession { session_id })
                        }
                        SessionChangeReason::SessionLock
                        | SessionChangeReason::SessionLogoff
                        | SessionChangeReason::ConsoleDisconnect
                        | SessionChangeReason::RemoteDisconnect
                        | SessionChangeReason::SessionTerminate => {
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
    status_handle.set_service_status(service_status(
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
        0,
        Duration::default(),
    ))?;

    if let Err(error) = service_result {
        eprintln!("{SERVICE_NAME} named pipe loop stopped with error: {error:?}");
    }
    Ok(())
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
