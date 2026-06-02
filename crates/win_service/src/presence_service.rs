use std::{
    fs,
    path::Path,
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
    desktop_session::active_user_session_id,
    presence_camera::{CameraPresenceObservationConfig, CameraPresenceObservationSource},
    presence_monitor::{NoopUnknownFaceAuditSink, PresenceMonitor, PresenceMonitorConfig},
    presence_person_camera::{
        PersonCameraPresenceObservationConfig, PersonCameraPresenceObservationSource,
    },
    presence_person_detector::OpenCvDnnPersonDetectorConfig,
    presence_policy::PresencePolicyConfig,
    service_config::{
        PresenceDetectorKind, PresencePersonDetectorModel, PresenceTrackingMode, ServiceAuthConfig,
        ServiceAuthMode, ServicePresenceConfig,
    },
    session_lock::WindowsSessionLocker,
};

const PRESENCE_RUNTIME_STATUS_PATH: &str =
    r"C:\ProgramData\WinFaceUnlock\presence-runtime-status.json";

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
    if let Some(session_id) = active_user_session_id() {
        running_monitor = start_presence_monitor_thread(session_id);
    } else {
        write_presence_runtime_status("waiting-for-session", None, "no active user session");
    }

    while let Ok(command) = receiver.recv() {
        match command {
            PresenceServiceCommand::StartForUserSession { session_id } => {
                if running_monitor
                    .as_ref()
                    .is_some_and(RunningPresenceMonitor::is_running)
                {
                    eprintln!(
                        "WinFaceUnlock presence monitor already running; ignoring session {session_id} start"
                    );
                    continue;
                }
                if let Some(previous_monitor) = running_monitor.take() {
                    previous_monitor.stop_and_join();
                }
                running_monitor = start_presence_monitor_thread(session_id);
            }
            PresenceServiceCommand::StopForUserSession { session_id } => {
                if let Some(previous_monitor) = running_monitor.take() {
                    eprintln!("WinFaceUnlock stopping presence monitor for session {session_id}");
                    previous_monitor.stop_and_join();
                }
            }
            PresenceServiceCommand::Shutdown => {
                if let Some(previous_monitor) = running_monitor.take() {
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
            return None;
        }
    };
    if !presence_config.presence_lock_enabled {
        write_presence_runtime_status("disabled", Some(session_id), "presence lock disabled");
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
            return None;
        }
    };
    let ServiceAuthMode::LocalCamera(local_camera_config) = auth_config.auth_mode else {
        write_presence_runtime_status(
            "failed",
            Some(session_id),
            "presence monitor requires local-camera auth config",
        );
        eprintln!(
            "WinFaceUnlock presence monitor requires local-camera auth config; ignoring session {session_id} start"
        );
        return None;
    };

    write_presence_runtime_status("starting", Some(session_id), "session is active");
    let stop_requested = Arc::new(AtomicBool::new(false));
    let thread_stop_requested = Arc::clone(&stop_requested);
    let join_handle = thread::Builder::new()
        .name(format!("winfaceunlock-presence-session-{session_id}"))
        .spawn(move || {
            write_presence_runtime_status("running", Some(session_id), "presence monitor started");
            let result = run_presence_monitor_for_local_camera(
                session_id,
                *local_camera_config,
                presence_config,
                thread_stop_requested,
            );
            match result {
                Ok(summary) => {
                    write_presence_runtime_status("stopped", Some(session_id), "monitor exited");
                    eprintln!("WinFaceUnlock presence monitor stopped: {summary:?}");
                }
                Err(error) => {
                    write_presence_runtime_status("failed", Some(session_id), "monitor failed");
                    eprintln!("WinFaceUnlock presence monitor failed: {error:?}");
                }
            }
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
    if presence_config.presence_detector_kind == PresenceDetectorKind::OpenCvDnnPerson
        && presence_config.presence_tracking_mode == PresenceTrackingMode::ContinuousLowFps
    {
        return run_person_presence_monitor_for_local_camera(
            session_id,
            local_camera_config,
            presence_config,
            stop_requested,
        );
    }

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
    let templates = RecognitionTemplates::new(read_face_templates(
        &local_camera_config.face_template_path,
    )?);
    let model_config = OpenCvFaceModelConfig::new(
        local_camera_config.yunet_model_path,
        local_camera_config.sface_model_path,
    );
    let source = CameraPresenceObservationSource::new(CameraPresenceObservationConfig {
        camera_id: local_camera_config.camera_id,
        camera_config: local_camera_config.camera_config,
        model_config,
        templates,
        presence_owner_match_threshold: presence_config.presence_owner_match_threshold,
        pending_unknown_face_crop_path: None,
    })
    .map_err(|_| ProtocolError::TransportUnavailable)?;

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
    let detector_config = person_detector_config_from_presence_config(&presence_config)?;
    let detector_config = OpenCvDnnPersonDetectorConfig {
        confidence_threshold: presence_config.presence_person_confidence_threshold,
        ..detector_config
    };
    let source =
        PersonCameraPresenceObservationSource::new(PersonCameraPresenceObservationConfig {
            camera_id: local_camera_config.camera_id,
            camera_config: local_camera_config.camera_config,
            detector_config,
            debug_output_dir: presence_config.presence_person_debug_output_dir.clone(),
        })
        .map_err(|_| ProtocolError::TransportUnavailable)?;

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
    monitor
        .run()
        .map_err(|_| ProtocolError::TransportUnavailable)
}

fn person_detector_config_from_presence_config(
    presence_config: &ServicePresenceConfig,
) -> Result<OpenCvDnnPersonDetectorConfig, ProtocolError> {
    match presence_config.presence_person_detector_model {
        PresencePersonDetectorModel::MobileNetSsd => {
            Ok(OpenCvDnnPersonDetectorConfig::mobilenet_ssd(
                presence_config.presence_person_model_path.clone(),
                presence_config
                    .presence_person_model_config_path
                    .clone()
                    .ok_or(ProtocolError::InvalidMessage)?,
            ))
        }
        PresencePersonDetectorModel::YoloV8Onnx => Ok(OpenCvDnnPersonDetectorConfig::yolov8_onnx(
            presence_config.presence_person_model_path.clone(),
        )),
    }
}

fn face_presence_policy_config(presence_config: &ServicePresenceConfig) -> PresencePolicyConfig {
    PresencePolicyConfig {
        presence_owner_match_threshold: presence_config.presence_owner_match_threshold,
        ..PresencePolicyConfig::default()
    }
}

fn person_presence_policy_config(presence_config: &ServicePresenceConfig) -> PresencePolicyConfig {
    let stable_interval_ms = presence_detector_interval_ms(presence_config.presence_detector_fps);
    let suspect_interval_ms =
        presence_detector_interval_ms(presence_config.presence_person_suspect_fps);
    PresencePolicyConfig {
        presence_stable_initial_interval_ms: stable_interval_ms,
        presence_stable_second_interval_ms: stable_interval_ms,
        presence_stable_max_interval_ms: stable_interval_ms,
        presence_no_face_suspect_interval_ms: suspect_interval_ms,
        presence_unknown_face_suspect_interval_ms: suspect_interval_ms,
        presence_owner_match_threshold: presence_config.presence_owner_match_threshold,
        presence_person_stable_interval_ms: stable_interval_ms,
        presence_person_suspect_interval_ms: suspect_interval_ms,
        presence_person_absent_required_frames: presence_config.presence_absent_required_frames,
        presence_person_boundary_margin_ratio: presence_config.presence_boundary_margin_ratio,
        presence_person_movement_delta_ratio: presence_config.presence_movement_delta_ratio,
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
    let path = Path::new(PRESENCE_RUNTIME_STATUS_PATH);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&status) {
        let _ = fs::write(path, bytes);
    }
}

fn current_time_unix_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    millis.min(i64::MAX as u128) as i64
}
