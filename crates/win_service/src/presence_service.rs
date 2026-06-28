use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

use common_protocol::ProtocolError;
use face_auth::RecognitionTemplates;
use face_engine::{FaceTemplate, FaceTemplateCodecError, FaceTemplateSet, OpenCvFaceModelConfig};

use crate::{
    camera_backend_profiles::apply_profile_to_config,
    camera_runtime::{
        InterfaceRuntimeState, InterfaceRuntimeStateSource, update_interface_runtime_state,
    },
    desktop_agent_launcher::ensure_desktop_input_presence_agent,
    desktop_input_state::{
        DesktopInputSnapshotActivitySource, desktop_input_agent_sample_interval,
    },
    desktop_session::active_user_session_id,
    presence_camera::{CameraPresenceObservationConfig, CameraPresenceObservationSource},
    presence_monitor::{NoopUnknownFaceAuditSink, PresenceMonitor, PresenceMonitorConfig},
    presence_person_camera::{
        PersonCameraPresenceObservationConfig, PersonCameraPresenceObservationSource,
    },
    presence_person_detector::{
        OpenCvDnnPersonDetectorConfig, OrtYoloV8PersonDetectorConfig, PersonDetectorConfig,
    },
    presence_policy::PresencePolicyConfig,
    presence_sampling_gate::{
        HumanInputGatedObservationSource, PresenceSamplingGate, PresenceSamplingGateConfig,
    },
    service_config::{
        PresenceDetectorKind, PresencePersonDetectorModel, PresenceTrackingMode, ServiceAuthConfig,
        ServiceAuthMode, ServicePresenceConfig,
    },
    service_log::write_service_event_detail,
    session_lock::WindowsSessionLocker,
};

const RUNTIME_DIR_NAME: &str = "runtime";
const PRESENCE_RUNTIME_STATUS_FILE_NAME: &str = "presence-runtime-status.json";

#[derive(serde::Serialize)]
struct PresenceRuntimeStatus<'a> {
    state: &'a str,
    session_id: Option<u32>,
    reason: &'a str,
    updated_at_unix_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresenceServiceCommand {
    StartForUserSession { session_id: u32 },
    StopForUserSession { session_id: u32 },
    ReloadCurrentSessionFromDesktopControl,
    Shutdown,
}

pub fn spawn_presence_service_controller() -> mpsc::Sender<PresenceServiceCommand> {
    let (sender, receiver) = mpsc::channel();
    let _ = thread::Builder::new()
        .name("winfaceunlock-presence-controller".to_owned())
        .spawn(move || run_presence_service_controller(receiver));
    sender
}

fn run_presence_service_controller(receiver: mpsc::Receiver<PresenceServiceCommand>) {
    let mut running_monitor: Option<RunningPresenceMonitor> = None;
    write_presence_runtime_status(
        "waiting-for-session",
        None,
        "waiting for unlock/logon or desktop settings reload",
    );
    write_service_event_detail(
        "PresenceMonitor.WaitingForSession",
        "reason=waiting-for-interface-state-event",
    );

    while let Ok(command) = receiver.recv() {
        match command {
            PresenceServiceCommand::StartForUserSession { session_id } => {
                if running_monitor
                    .as_ref()
                    .is_some_and(RunningPresenceMonitor::is_running)
                {
                    write_service_event_detail(
                        "PresenceMonitor.StartIgnored",
                        format!("session_id={session_id} reason=already-running"),
                    );
                    eprintln!(
                        "WinFaceUnlock presence monitor already running; ignoring session {session_id} start"
                    );
                    continue;
                }
                if let Some(previous_monitor) = running_monitor.take() {
                    write_service_event_detail(
                        "PresenceMonitor.StopBeforeStart",
                        format!("session_id={session_id}"),
                    );
                    previous_monitor.stop_and_join();
                }
                write_service_event_detail(
                    "PresenceMonitor.StartRequested",
                    format!("session_id={session_id} source=session-change"),
                );
                running_monitor = start_presence_monitor_thread(session_id);
            }
            PresenceServiceCommand::StopForUserSession { session_id } => {
                if let Some(previous_monitor) = running_monitor.take() {
                    write_service_event_detail(
                        "PresenceMonitor.StopRequested",
                        format!("session_id={session_id} source=session-change"),
                    );
                    eprintln!("WinFaceUnlock stopping presence monitor for session {session_id}");
                    previous_monitor.stop_and_join();
                }
            }
            PresenceServiceCommand::ReloadCurrentSessionFromDesktopControl => {
                update_interface_runtime_state(
                    InterfaceRuntimeState::DesktopUnlocked,
                    InterfaceRuntimeStateSource::DesktopControlReload,
                );
                if let Some(previous_monitor) = running_monitor.take() {
                    write_service_event_detail(
                        "PresenceMonitor.ReloadStoppingCurrent",
                        "reason=settings-reload",
                    );
                    previous_monitor.stop_and_join();
                }
                if let Some(session_id) = active_user_session_id() {
                    write_service_event_detail(
                        "PresenceMonitor.ReloadStartRequested",
                        format!("session_id={session_id} source=desktop-control"),
                    );
                    running_monitor = start_presence_monitor_thread(session_id);
                } else {
                    write_presence_runtime_status(
                        "waiting-for-session",
                        None,
                        "no active user session after settings reload",
                    );
                    write_service_event_detail(
                        "PresenceMonitor.ReloadWaitingForSession",
                        "reason=no-active-user-session",
                    );
                }
            }
            PresenceServiceCommand::Shutdown => {
                if let Some(previous_monitor) = running_monitor.take() {
                    write_service_event_detail(
                        "PresenceMonitor.ShutdownStoppingCurrent",
                        "reason=service-shutdown",
                    );
                    previous_monitor.stop_and_join();
                }
                return;
            }
        }
    }
}

struct RunningPresenceMonitor {
    stop_requested: Arc<AtomicBool>,
    join_handle: JoinHandle<()>,
}

impl RunningPresenceMonitor {
    fn is_running(&self) -> bool {
        !self.join_handle.is_finished()
    }

    fn stop_and_join(self) {
        self.stop_requested.store(true, Ordering::SeqCst);
        let _ = self.join_handle.join();
    }
}

fn start_presence_monitor_thread(session_id: u32) -> Option<RunningPresenceMonitor> {
    let presence_config = match ServicePresenceConfig::from_environment() {
        Ok(config) => config,
        Err(_) => {
            write_presence_runtime_status(
                "failed",
                Some(session_id),
                "presence config could not be read",
            );
            write_service_event_detail(
                "PresenceMonitor.StartFailed",
                format!("session_id={session_id} reason=presence-config-unavailable"),
            );
            return None;
        }
    };
    if !presence_config.presence_lock_enabled {
        write_presence_runtime_status("disabled", Some(session_id), "presence lock disabled");
        write_service_event_detail(
            "PresenceMonitor.Disabled",
            format!("session_id={session_id}"),
        );
        eprintln!("WinFaceUnlock presence monitor disabled; ignoring session {session_id} start");
        return None;
    }

    let auth_config = match ServiceAuthConfig::from_environment() {
        Ok(config) => config,
        Err(_) => {
            write_presence_runtime_status(
                "failed",
                Some(session_id),
                "service auth config could not be read",
            );
            write_service_event_detail(
                "PresenceMonitor.StartFailed",
                format!("session_id={session_id} reason=service-auth-config-unavailable"),
            );
            return None;
        }
    };
    let ServiceAuthMode::LocalCamera(local_camera_config) = auth_config.auth_mode else {
        write_presence_runtime_status(
            "failed",
            Some(session_id),
            "presence monitor requires local-camera auth config",
        );
        write_service_event_detail(
            "PresenceMonitor.StartFailed",
            format!("session_id={session_id} reason=local-camera-auth-required"),
        );
        eprintln!(
            "WinFaceUnlock presence monitor requires local-camera auth config; ignoring session {session_id} start"
        );
        return None;
    };

    write_presence_runtime_status("starting", Some(session_id), "session is active");
    write_service_event_detail(
        "PresenceMonitor.Starting",
        format!(
            "session_id={} camera_id={} detector_kind={:?} tracking_mode={:?}",
            session_id,
            local_camera_config.camera_id.0,
            presence_config.presence_detector_kind,
            presence_config.presence_tracking_mode
        ),
    );
    match ensure_desktop_input_presence_agent(session_id, desktop_input_agent_sample_interval()) {
        Ok(()) => {
            write_service_event_detail(
                "DesktopInputPresenceAgent.EnsureRequested",
                format!("session_id={session_id}"),
            );
        }
        Err(error) => {
            write_service_event_detail(
                "DesktopInputPresenceAgent.EnsureFailed",
                format!("session_id={session_id} error={error:?}"),
            );
        }
    }
    let stop_requested = Arc::new(AtomicBool::new(false));
    let thread_stop_requested = Arc::clone(&stop_requested);
    let join_handle = thread::Builder::new()
        .name(format!("winfaceunlock-presence-session-{session_id}"))
        .spawn(move || {
            write_presence_runtime_status("running", Some(session_id), "presence monitor started");
            write_service_event_detail(
                "PresenceMonitor.Running",
                format!("session_id={session_id}"),
            );
            let result = run_presence_monitor_for_local_camera(
                session_id,
                *local_camera_config,
                presence_config,
                thread_stop_requested,
            );
            match result {
                Ok(summary) => {
                    write_presence_runtime_status("stopped", Some(session_id), "monitor exited");
                    write_service_event_detail(
                        "PresenceMonitor.Stopped",
                        format!("session_id={session_id} summary={summary:?}"),
                    );
                    eprintln!("WinFaceUnlock presence monitor stopped: {summary:?}");
                }
                Err(error) => {
                    write_presence_runtime_status("failed", Some(session_id), "monitor failed");
                    write_service_event_detail(
                        "PresenceMonitor.Failed",
                        format!("session_id={session_id} error={error:?}"),
                    );
                    eprintln!("WinFaceUnlock presence monitor failed: {error:?}");
                }
            }
        })
        .map_err(|error| {
            write_service_event_detail(
                "PresenceMonitor.StartFailed",
                format!("session_id={session_id} reason=thread-spawn-failed error={error}"),
            );
        })
        .ok()?;

    Some(RunningPresenceMonitor {
        stop_requested,
        join_handle,
    })
}

fn run_presence_monitor_for_local_camera(
    session_id: u32,
    local_camera_config: crate::service_config::LocalCameraAuthConfig,
    presence_config: ServicePresenceConfig,
    stop_requested: Arc<AtomicBool>,
) -> Result<crate::presence_monitor::PresenceMonitorSummary, ProtocolError> {
    if matches!(
        presence_config.presence_detector_kind,
        PresenceDetectorKind::OpenCvDnnPerson | PresenceDetectorKind::MediaPipePoseLite
    ) && presence_config.presence_tracking_mode == PresenceTrackingMode::ContinuousLowFps
    {
        write_service_event_detail(
            "PresenceMonitor.CameraModeSelected",
            format!(
                "session_id={} mode=person-continuous camera_id={}",
                session_id, local_camera_config.camera_id.0
            ),
        );
        return run_person_presence_monitor_for_local_camera(
            session_id,
            local_camera_config,
            presence_config,
            stop_requested,
        );
    }

    write_service_event_detail(
        "PresenceMonitor.CameraModeSelected",
        format!(
            "session_id={} mode=face-policy camera_id={}",
            session_id, local_camera_config.camera_id.0
        ),
    );
    run_face_presence_monitor_for_local_camera(
        session_id,
        local_camera_config,
        presence_config,
        stop_requested,
    )
}

fn run_face_presence_monitor_for_local_camera(
    session_id: u32,
    local_camera_config: crate::service_config::LocalCameraAuthConfig,
    presence_config: ServicePresenceConfig,
    stop_requested: Arc<AtomicBool>,
) -> Result<crate::presence_monitor::PresenceMonitorSummary, ProtocolError> {
    let mut camera_config = local_camera_config.camera_config;
    apply_profile_to_config(&local_camera_config.camera_id, &mut camera_config);
    let templates = RecognitionTemplates::new(read_face_templates(
        &local_camera_config.face_template_path,
    )?);
    let model_config = OpenCvFaceModelConfig::new(
        local_camera_config.yunet_model_path,
        local_camera_config.sface_model_path,
    );
    let source = CameraPresenceObservationSource::new(CameraPresenceObservationConfig {
        camera_id: local_camera_config.camera_id,
        camera_config,
        model_config,
        templates,
        presence_owner_match_threshold: presence_config.presence_owner_match_threshold,
        pending_unknown_face_crop_path: None,
    })
    .map_err(|_| ProtocolError::TransportUnavailable)?;
    let source = HumanInputGatedObservationSource::new(
        source,
        PresenceSamplingGate::new(
            PresenceSamplingGateConfig::default(),
            DesktopInputSnapshotActivitySource::new(session_id),
        ),
        Arc::clone(&stop_requested),
    );

    let mut monitor = PresenceMonitor::new(
        PresenceMonitorConfig {
            presence_lock_enabled: true,
            max_monitor_iteration_count: None,
            sleep_between_checks: true,
            stop_requested: Some(stop_requested),
        },
        face_presence_policy_config(&presence_config),
        WindowsSessionLocker::user_session(session_id),
        NoopUnknownFaceAuditSink,
        source,
    );
    monitor
        .run()
        .map_err(|_| ProtocolError::TransportUnavailable)
}

fn run_person_presence_monitor_for_local_camera(
    session_id: u32,
    local_camera_config: crate::service_config::LocalCameraAuthConfig,
    presence_config: ServicePresenceConfig,
    stop_requested: Arc<AtomicBool>,
) -> Result<crate::presence_monitor::PresenceMonitorSummary, ProtocolError> {
    let mut camera_config = local_camera_config.camera_config;
    apply_profile_to_config(&local_camera_config.camera_id, &mut camera_config);
    let detector_config = person_detector_config_from_presence_config(&presence_config)?;
    write_service_event_detail(
        "PresenceMonitor.PersonConfigResolved",
        format!(
            "session_id={} detector_model={:?} detector_config={:?} model_path={} model_exists={} config_path={} config_exists={} debug_output_dir={}",
            session_id,
            presence_config.presence_person_detector_model,
            detector_config,
            presence_config.presence_person_model_path.display(),
            presence_config.presence_person_model_path.exists(),
            presence_config
                .presence_person_model_config_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<none>".to_owned()),
            presence_config
                .presence_person_model_config_path
                .as_ref()
                .map(|path| path.exists())
                .unwrap_or(true),
            presence_config
                .presence_person_debug_output_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<none>".to_owned())
        ),
    );
    let source =
        PersonCameraPresenceObservationSource::new(PersonCameraPresenceObservationConfig {
            camera_id: local_camera_config.camera_id,
            camera_config,
            detector_config,
            debug_output_dir: presence_config.presence_person_debug_output_dir.clone(),
        })
        .map_err(|error| {
            write_service_event_detail(
                "PresenceMonitor.PersonSourceCreateFailed",
                format!("session_id={session_id} error={error:?}"),
            );
            ProtocolError::TransportUnavailable
        })?;
    let source = HumanInputGatedObservationSource::new(
        source,
        PresenceSamplingGate::new(
            PresenceSamplingGateConfig::default(),
            DesktopInputSnapshotActivitySource::new(session_id),
        ),
        Arc::clone(&stop_requested),
    );

    let mut monitor = PresenceMonitor::new(
        PresenceMonitorConfig {
            presence_lock_enabled: true,
            max_monitor_iteration_count: None,
            sleep_between_checks: true,
            stop_requested: Some(stop_requested),
        },
        person_presence_policy_config(&presence_config),
        WindowsSessionLocker::user_session(session_id),
        NoopUnknownFaceAuditSink,
        source,
    );
    monitor.run().map_err(|error| {
        write_service_event_detail(
            "PresenceMonitor.PersonRunFailed",
            format!("session_id={session_id} error={error:?}"),
        );
        ProtocolError::TransportUnavailable
    })
}

fn person_detector_config_from_presence_config(
    presence_config: &ServicePresenceConfig,
) -> Result<PersonDetectorConfig, ProtocolError> {
    match presence_config.presence_person_detector_model {
        PresencePersonDetectorModel::MobileNetSsd => {
            let mut config = OpenCvDnnPersonDetectorConfig::mobilenet_ssd(
                presence_config.presence_person_model_path.clone(),
                presence_config
                    .presence_person_model_config_path
                    .clone()
                    .ok_or(ProtocolError::InvalidMessage)?,
            );
            config.confidence_threshold = presence_config.presence_person_confidence_threshold;
            Ok(PersonDetectorConfig::OpenCvDnn(config))
        }
        PresencePersonDetectorModel::YoloV8Onnx => {
            let mut config = OpenCvDnnPersonDetectorConfig::yolov8_onnx(
                presence_config.presence_person_model_path.clone(),
            );
            config.confidence_threshold = presence_config.presence_person_confidence_threshold;
            Ok(PersonDetectorConfig::OpenCvDnn(config))
        }
        PresencePersonDetectorModel::OrtYoloV8Onnx => {
            let mut config = OrtYoloV8PersonDetectorConfig::new(
                presence_config.presence_person_model_path.clone(),
            );
            config.confidence_threshold = presence_config.presence_person_confidence_threshold;
            Ok(PersonDetectorConfig::OrtYoloV8(config))
        }
    }
}

fn face_presence_policy_config(presence_config: &ServicePresenceConfig) -> PresencePolicyConfig {
    PresencePolicyConfig {
        presence_owner_match_threshold: presence_config.presence_owner_match_threshold,
        ..PresencePolicyConfig::default()
    }
}

fn person_presence_policy_config(presence_config: &ServicePresenceConfig) -> PresencePolicyConfig {
    let suspect_interval_ms =
        presence_detector_interval_ms(presence_config.presence_person_suspect_fps);
    PresencePolicyConfig {
        presence_no_face_suspect_interval_ms: suspect_interval_ms,
        presence_unknown_face_suspect_interval_ms: suspect_interval_ms,
        presence_owner_match_threshold: presence_config.presence_owner_match_threshold,
        presence_person_suspect_interval_ms: suspect_interval_ms,
        presence_person_absent_required_frames: presence_config.presence_absent_required_frames,
        ..PresencePolicyConfig::default()
    }
}

fn presence_detector_interval_ms(fps: f32) -> u64 {
    if fps <= 0.0 {
        return 500;
    }
    ((1_000.0 / fps).round() as u64).max(1)
}

fn read_face_templates(
    template_path: &std::path::Path,
) -> Result<Vec<FaceTemplate>, ProtocolError> {
    let bytes = fs::read(template_path).map_err(|_| ProtocolError::InvalidMessage)?;
    if let Ok(template_set) = FaceTemplateSet::from_json_bytes(&bytes) {
        return Ok(template_set.selected_templates());
    }

    FaceTemplate::from_json_bytes(&bytes)
        .map(|template| vec![template])
        .map_err(template_codec_to_protocol_error)
}

fn template_codec_to_protocol_error(_error: FaceTemplateCodecError) -> ProtocolError {
    ProtocolError::InvalidMessage
}

fn write_presence_runtime_status(state: &str, session_id: Option<u32>, reason: &str) {
    let status = PresenceRuntimeStatus {
        state,
        session_id,
        reason,
        updated_at_unix_ms: current_time_unix_ms(),
    };
    let path = presence_runtime_status_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&status) {
        let _ = fs::write(path, bytes);
    }
}

fn presence_runtime_status_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent().map(|install_dir| {
                install_dir
                    .join(RUNTIME_DIR_NAME)
                    .join(PRESENCE_RUNTIME_STATUS_FILE_NAME)
            })
        })
        .unwrap_or_else(|| {
            std::env::temp_dir()
                .join("WinFaceUnlock")
                .join(RUNTIME_DIR_NAME)
                .join(PRESENCE_RUNTIME_STATUS_FILE_NAME)
        })
}

fn current_time_unix_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    millis.min(i64::MAX as u128) as i64
}
