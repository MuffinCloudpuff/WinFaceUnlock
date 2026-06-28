use std::{
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use common_protocol::{
    AccountType, AuthFailureReason, AuthGrant, AuthSource, AuthTriggerSource, CredentialRef,
    GrantId, Nonce, PIPE_NAME, ProtocolError, SERVICE_NAME, ServiceEvent, ServiceRequest,
    SessionId, UserId,
};
use face_auth::{
    AttemptPolicy, AttemptPolicyConfig, FaceAuthenticator, FaceEnrollmentService,
    FaceQualityPolicy, GuidedEnrollmentConfig, GuidedEnrollmentStep, GuidedFaceEnrollmentService,
    GuidedFrameObservation, RecognitionTemplates, build_guided_enrollment_report,
};
use face_engine::{
    FaceEngineError, FaceMatchDecision, FaceModelProvider, FaceSampleRejectReason, FaceTemplate,
    FaceTemplateCodecError, FaceTemplateMatcher, FaceTemplateRef, FaceTemplateSet,
    OpenCvFaceModelConfig, OpenCvFaceModelProvider,
};
use face_liveness::{MiniFasNetLivenessProviderConfig, ScreenReplayLivenessProviderConfig};
use face_pose::{FacePoseProvider, LandmarkFacePoseProvider};
#[cfg(feature = "mediapipe-pose")]
use face_pose_mediapipe::{
    MediaPipeFacePoseProvider, MediaPipeFacePoseProviderConfig, MediaPipeFacePoseProviderError,
};
use ipc::{IpcClient, NamedPipeClient};
use opencv::{
    core::{AlgorithmHint, Mat, MatTraitConst, Rect, Scalar, Vector},
    imgcodecs, imgproc,
    prelude::VectorToVec,
};
use video_provider::{CameraId, OpenCvCameraProvider, OpenCvCameraProviderConfig, PixelFormat};
use video_provider::{VideoError, VideoFrame, VideoFrameProvider};
use win_service::camera_backend_profiles::apply_profile_to_config;
use win_service::camera_runtime::{
    InterfaceRuntimeState, InterfaceRuntimeStateSource, update_interface_runtime_state,
};
use win_service::credential_store_config::{
    ServiceCredentialStorePaths, WindowsCredentialEnrollment, enroll_windows_credential,
};
use win_service::presence_audit::{PresenceAuditConfig, PresenceAuditStore, UnknownFaceAuditEvent};
use win_service::presence_camera::{
    CameraPresenceObservationConfig, CameraPresenceObservationSource,
};
use win_service::presence_helper::{
    LocalProcessPresenceHelper, PresenceHelperClient, PresenceHelperRequest, PresenceHelperResponse,
};
use win_service::presence_monitor::{
    PresenceMonitor, PresenceMonitorConfig, PresenceMonitorError, PresenceObservationSource,
    UnknownFaceAuditSink,
};
use win_service::presence_person_camera::{
    PersonCameraPresenceObservationConfig, PersonCameraPresenceObservationSource,
};
use win_service::presence_person_detector::{
    OpenCvDnnPersonDetector, OpenCvDnnPersonDetectorConfig, OrtYoloV8PersonDetectorConfig,
    PersonDetection, PersonDetectorConfig, PresenceDetector, PresencePersonDetectorError,
};
use win_service::presence_policy::{
    PresenceObservation, PresencePolicy, PresencePolicyConfig, PresencePolicyDecision,
};
use win_service::session_lock::{SessionLockError, SessionLocker, WindowsSessionLocker};

use crate::face_calibration::{FaceCalibrationConfig, FaceCalibrationError, run_face_calibration};
use crate::face_debug_snapshot::{
    FaceDebugSnapshotConfig, FaceDebugSnapshotError, run_face_debug_snapshot,
};
use crate::liveness_screen_debug::{
    LivenessScreenDebugConfig, LivenessScreenDebugError, run_liveness_screen_debug,
};
use crate::threshold_preview::{
    ThresholdPreviewConfig, ThresholdPreviewError, ThresholdPreviewMethod, run_threshold_preview,
};

const DEFAULT_YUNET_MODEL_PATH: &str = "models/face_detection_yunet_2023mar.onnx";
const DEFAULT_SFACE_MODEL_PATH: &str = "models/face_recognition_sface_2021dec.onnx";
const DEFAULT_MINIFASNET_MODEL_PATH: &str = "models/minifasnet_v2.onnx";
const DEFAULT_PROJECT_FACE_MATCH_THRESHOLD: f32 = 0.75;
const DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD: f32 = 0.50;
const ENROLLMENT_STATUS_FILE_NAME: &str = "enrollment_status.json";
const PREVIEW_EVENT_PREFIX: &str = "WINFACEUNLOCK_PREVIEW_FRAME ";
const AUTH_RESULT_POLL_ATTEMPTS: usize = 80;
const AUTH_RESULT_POLL_DELAY: Duration = Duration::from_millis(500);
#[cfg(feature = "mediapipe-pose")]
const DEFAULT_MEDIAPIPE_BRIDGE_DLL_PATH: &str = "native/winfaceunlock_mediapipe_bridge.dll";
#[cfg(feature = "mediapipe-pose")]
const DEFAULT_MEDIAPIPE_FACE_LANDMARKER_TASK_PATH: &str = "models/face_landmarker.task";

#[derive(Debug)]
pub enum DiagnosticError {
    Protocol(ProtocolError),
    Video(VideoError),
    Face(FaceEngineError),
    FaceCalibration(FaceCalibrationError),
    FaceDebugSnapshot(FaceDebugSnapshotError),
    LivenessScreenDebug(LivenessScreenDebugError),
    ThresholdPreview(ThresholdPreviewError),
    PresencePersonDetector(PresencePersonDetectorError),
    #[cfg(not(feature = "mediapipe-pose"))]
    MediaPipeFeatureDisabled,
    #[cfg(feature = "mediapipe-pose")]
    FacePoseProvider(MediaPipeFacePoseProviderError),
    TemplateCodec(FaceTemplateCodecError),
    IoFailed,
    InvalidArgument,
    AuthRejected(AuthFailureReason),
    PasswordPromptFailed,
    PasswordConfirmationMismatch,
    GuidedEnrollmentStepIncomplete {
        step: String,
        accepted_frame_count: u32,
        required_frame_count: u32,
        attempted_frame_count: u32,
    },
}

impl fmt::Display for DiagnosticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => write!(formatter, "protocol error: {error:?}"),
            Self::Video(error) => write!(formatter, "video error: {error:?}"),
            Self::Face(error) => write!(formatter, "face engine error: {error:?}"),
            Self::FaceCalibration(error) => write!(formatter, "face calibration error: {error}"),
            Self::FaceDebugSnapshot(error) => {
                write!(formatter, "face debug snapshot error: {error}")
            }
            Self::LivenessScreenDebug(error) => {
                write!(formatter, "liveness screen debug error: {error}")
            }
            Self::ThresholdPreview(error) => write!(formatter, "threshold preview error: {error}"),
            Self::PresencePersonDetector(error) => {
                write!(formatter, "presence person detector error: {error}")
            }
            #[cfg(not(feature = "mediapipe-pose"))]
            Self::MediaPipeFeatureDisabled => write!(
                formatter,
                "mediapipe pose provider requires diagnostics_cli feature mediapipe-pose"
            ),
            #[cfg(feature = "mediapipe-pose")]
            Self::FacePoseProvider(error) => {
                write!(formatter, "face pose provider error: {error:?}")
            }
            Self::TemplateCodec(error) => write!(formatter, "template codec error: {error:?}"),
            Self::IoFailed => write!(formatter, "I/O operation failed"),
            Self::InvalidArgument => write!(formatter, "invalid or missing argument"),
            Self::AuthRejected(reason) => write!(formatter, "authentication rejected: {reason:?}"),
            Self::PasswordPromptFailed => write!(formatter, "password prompt failed"),
            Self::PasswordConfirmationMismatch => {
                write!(formatter, "password confirmation mismatch")
            }
            Self::GuidedEnrollmentStepIncomplete {
                step,
                accepted_frame_count,
                required_frame_count,
                attempted_frame_count,
            } => write!(
                formatter,
                "guided enrollment step incomplete: step={step} accepted_frame_count={accepted_frame_count} required_frame_count={required_frame_count} attempted_frame_count={attempted_frame_count}"
            ),
        }
    }
}

impl From<ProtocolError> for DiagnosticError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<VideoError> for DiagnosticError {
    fn from(value: VideoError) -> Self {
        Self::Video(value)
    }
}

impl From<FaceEngineError> for DiagnosticError {
    fn from(value: FaceEngineError) -> Self {
        Self::Face(value)
    }
}

impl From<FaceCalibrationError> for DiagnosticError {
    fn from(value: FaceCalibrationError) -> Self {
        Self::FaceCalibration(value)
    }
}

impl From<FaceDebugSnapshotError> for DiagnosticError {
    fn from(value: FaceDebugSnapshotError) -> Self {
        Self::FaceDebugSnapshot(value)
    }
}

impl From<LivenessScreenDebugError> for DiagnosticError {
    fn from(value: LivenessScreenDebugError) -> Self {
        Self::LivenessScreenDebug(value)
    }
}

impl From<ThresholdPreviewError> for DiagnosticError {
    fn from(value: ThresholdPreviewError) -> Self {
        Self::ThresholdPreview(value)
    }
}

impl From<PresencePersonDetectorError> for DiagnosticError {
    fn from(value: PresencePersonDetectorError) -> Self {
        Self::PresencePersonDetector(value)
    }
}

impl From<FaceTemplateCodecError> for DiagnosticError {
    fn from(value: FaceTemplateCodecError) -> Self {
        Self::TemplateCodec(value)
    }
}

pub fn run_from_args(args: impl IntoIterator<Item = String>) -> Result<(), DiagnosticError> {
    let args: Vec<String> = args.into_iter().collect();
    let pipe_name = argument_value(&args, "--pipe-name").unwrap_or(PIPE_NAME);

    if args.iter().any(|arg| arg == "health-check") {
        let event = send_health_check(pipe_name)?;
        println!("{SERVICE_NAME} health-check: {event:?}");
    } else if args.iter().any(|arg| arg == "pipe-check") {
        let mut client = NamedPipeClient::new(pipe_name);
        match client.connect() {
            Ok(()) => {
                client.disconnect();
                println!("{SERVICE_NAME} pipe-check: connected");
            }
            Err(error) => {
                println!(
                    "{SERVICE_NAME} pipe-check: {error:?} win32_error={:?}",
                    client.last_connect_error()
                );
                return Err(error.into());
            }
        }
    } else if args.iter().any(|arg| arg == "wake-auth") {
        let session_id = SessionId(
            argument_value(&args, "--session-id")
                .unwrap_or("diagnostics-session")
                .to_owned(),
        );
        let source = wake_auth_source(&args)?;
        let event = send_wake_auth(pipe_name, session_id, source)?;
        print_wake_auth_event(&event);
    } else if args.iter().any(|arg| arg == "fetch-credential") {
        let session_id = SessionId(
            argument_value(&args, "--session-id")
                .unwrap_or("diagnostics-session")
                .to_owned(),
        );
        let grant_id = GrantId(
            argument_value(&args, "--grant-id")
                .ok_or(DiagnosticError::InvalidArgument)?
                .to_owned(),
        );
        let nonce = Nonce(
            argument_value(&args, "--nonce")
                .ok_or(DiagnosticError::InvalidArgument)?
                .to_owned(),
        );
        let event = send_fetch_credential(pipe_name, session_id, grant_id, nonce)?;
        println!("{SERVICE_NAME} fetch-credential: {event:?}");
    } else if args.iter().any(|arg| arg == "fetch-credential-material") {
        let session_id = SessionId(
            argument_value(&args, "--session-id")
                .unwrap_or("diagnostics-session")
                .to_owned(),
        );
        let grant_id = GrantId(
            argument_value(&args, "--grant-id")
                .ok_or(DiagnosticError::InvalidArgument)?
                .to_owned(),
        );
        let nonce = Nonce(
            argument_value(&args, "--nonce")
                .ok_or(DiagnosticError::InvalidArgument)?
                .to_owned(),
        );
        let event = send_fetch_credential_material(pipe_name, session_id, grant_id, nonce)?;
        print_credential_material_event(&event);
    } else if args.iter().any(|arg| arg == "enroll-windows-credential") {
        run_enroll_windows_credential(&args)?;
    } else if args.iter().any(|arg| arg == "service-camera-auth") {
        run_service_camera_auth(pipe_name, &args)?;
    } else if args.iter().any(|arg| arg == "list-cameras") {
        run_list_cameras(&args)?;
    } else if args.iter().any(|arg| arg == "camera-open-benchmark") {
        run_camera_open_benchmark(&args)?;
    } else if args.iter().any(|arg| arg == "test-camera") {
        run_test_camera(&args)?;
    } else if args.iter().any(|arg| arg == "test-face") {
        run_test_face(&args)?;
    } else if args.iter().any(|arg| arg == "enroll-face") {
        run_enroll_face(&args)?;
    } else if args.iter().any(|arg| arg == "enroll-camera") {
        run_enroll_camera(&args)?;
    } else if args.iter().any(|arg| arg == "guided-enroll") {
        run_guided_enroll(&args)?;
    } else if args.iter().any(|arg| arg == "enrollment-report") {
        run_enrollment_report(&args)?;
    } else if args.iter().any(|arg| arg == "face-debug-snapshot") {
        run_face_debug_snapshot_command(&args)?;
    } else if args.iter().any(|arg| arg == "face-calibrate") {
        run_face_calibrate_command(&args)?;
    } else if args.iter().any(|arg| arg == "liveness-screen-debug") {
        run_liveness_screen_debug_command(&args)?;
    } else if args.iter().any(|arg| arg == "threshold-preview") {
        run_threshold_preview_command(&args)?;
    } else if args.iter().any(|arg| arg == "presence-check-once") {
        run_presence_check_once(&args)?;
    } else if args.iter().any(|arg| arg == "presence-policy-simulate") {
        run_presence_policy_simulate(&args)?;
    } else if args.iter().any(|arg| arg == "presence-monitor-simulate") {
        run_presence_monitor_simulate(&args)?;
    } else if args
        .iter()
        .any(|arg| arg == "presence-monitor-camera-debug")
    {
        run_presence_monitor_camera_debug(&args)?;
    } else if args.iter().any(|arg| arg == "presence-person-benchmark") {
        run_presence_person_benchmark(&args)?;
    } else if args.iter().any(|arg| arg == "presence-person-lock-debug") {
        run_presence_person_lock_debug(&args)?;
    } else if args.iter().any(|arg| arg == "screen-snapshot-debug") {
        run_screen_snapshot_debug(&args)?;
    } else if args.iter().any(|arg| arg == "face-auth-debug") {
        run_face_auth_debug(&args)?;
    } else if args.iter().any(|arg| arg == "verify-face") {
        run_verify_face(&args)?;
    } else if args.iter().any(|arg| arg == "camera-auth") {
        run_camera_auth(&args)?;
    } else if args.iter().any(|arg| arg == "calibrate-threshold") {
        run_calibrate_threshold(&args)?;
    } else {
        print_usage();
    }

    Ok(())
}

fn run_enroll_windows_credential(args: &[String]) -> Result<(), DiagnosticError> {
    let user_id = UserId(
        argument_value(args, "--user-id")
            .unwrap_or("dev-user")
            .to_owned(),
    );
    let username = argument_value(args, "--username")
        .ok_or(DiagnosticError::InvalidArgument)?
        .to_owned();
    let account_type = account_type_argument(args)?;
    let user_sid = argument_value(args, "--user-sid")
        .unwrap_or("S-1-5-21-winfaceunlock-pending")
        .to_owned();
    let credential_ref = CredentialRef(
        argument_value(args, "--credential-ref")
            .map(str::to_owned)
            .unwrap_or_else(|| format!("windows-credential-{}", user_id.0)),
    );
    let store_paths = argument_value(args, "--store-dir")
        .map(|path| ServiceCredentialStorePaths::from_store_dir(PathBuf::from(path)))
        .unwrap_or_else(ServiceCredentialStorePaths::from_environment_or_default);

    println!(
        "credential_store_database: {}",
        store_paths.database_path.display()
    );
    println!(
        "credential_store_master_key: {}",
        store_paths.master_key_path.display()
    );
    println!("user_id: {}", user_id.0);
    println!("username: {username}");
    println!("account_type: {account_type:?}");
    println!("credential_ref: {}", credential_ref.0);
    let password = prompt_password_twice()?;

    enroll_windows_credential(
        &store_paths,
        WindowsCredentialEnrollment {
            user_id,
            user_sid,
            username,
            account_type,
            credential_ref,
            password,
        },
    )
    .map_err(DiagnosticError::Protocol)?;
    println!("windows_credential_enrolled: true");
    Ok(())
}

fn prompt_password_twice() -> Result<String, DiagnosticError> {
    let password = rpassword::prompt_password("Windows password: ")
        .map_err(|_| DiagnosticError::PasswordPromptFailed)?;
    let confirmation = rpassword::prompt_password("Confirm Windows password: ")
        .map_err(|_| DiagnosticError::PasswordPromptFailed)?;
    if password != confirmation {
        return Err(DiagnosticError::PasswordConfirmationMismatch);
    }
    if password.is_empty() {
        return Err(DiagnosticError::InvalidArgument);
    }
    Ok(password)
}

fn account_type_argument(args: &[String]) -> Result<AccountType, DiagnosticError> {
    match argument_value(args, "--account-type").unwrap_or("local") {
        "local" => Ok(AccountType::Local),
        "microsoft" | "microsoft-account" => Ok(AccountType::MicrosoftAccount),
        "domain" => Ok(AccountType::Domain),
        _ => Err(DiagnosticError::InvalidArgument),
    }
}

pub fn send_health_check(pipe_name: &str) -> Result<ServiceEvent, ProtocolError> {
    send_request(pipe_name, ServiceRequest::HealthCheck)
}

pub fn send_wake_auth(
    pipe_name: &str,
    session_id: SessionId,
    source: AuthSource,
) -> Result<ServiceEvent, ProtocolError> {
    send_request(
        pipe_name,
        ServiceRequest::WakeAuth {
            session_id,
            source,
            trigger_source: AuthTriggerSource::CredentialScreenEntered,
        },
    )
}

pub fn send_fetch_auth_result(
    pipe_name: &str,
    session_id: SessionId,
) -> Result<ServiceEvent, ProtocolError> {
    send_request(pipe_name, ServiceRequest::FetchAuthResult { session_id })
}

pub fn send_fetch_credential(
    pipe_name: &str,
    session_id: SessionId,
    grant_id: GrantId,
    nonce: Nonce,
) -> Result<ServiceEvent, ProtocolError> {
    send_request(
        pipe_name,
        ServiceRequest::FetchCredential {
            session_id,
            grant_id,
            nonce,
        },
    )
}

pub fn send_fetch_credential_material(
    pipe_name: &str,
    session_id: SessionId,
    grant_id: GrantId,
    nonce: Nonce,
) -> Result<ServiceEvent, ProtocolError> {
    send_request(
        pipe_name,
        ServiceRequest::FetchCredentialMaterial {
            session_id,
            grant_id,
            nonce,
        },
    )
}

fn run_service_camera_auth(pipe_name: &str, args: &[String]) -> Result<(), DiagnosticError> {
    let session_id = SessionId(
        argument_value(args, "--session-id")
            .unwrap_or("phase4-service-camera-auth")
            .to_owned(),
    );
    let wake_event = send_wake_auth(pipe_name, session_id.clone(), AuthSource::LocalCamera)?;
    print_wake_auth_event(&wake_event);
    let grant = wait_for_auth_grant(pipe_name, wake_event)?;

    let credential_event = send_fetch_credential(
        pipe_name,
        session_id,
        grant.grant_id.clone(),
        grant.nonce.clone(),
    )?;
    println!("{SERVICE_NAME} fetch-credential: {credential_event:?}");
    if matches!(credential_event, ServiceEvent::CredentialReady { .. }) {
        Ok(())
    } else {
        Err(DiagnosticError::Protocol(ProtocolError::InvalidMessage))
    }
}

fn auth_grant_from_event(event: ServiceEvent) -> Result<AuthGrant, DiagnosticError> {
    match event {
        ServiceEvent::AuthSucceeded { grant } => Ok(grant),
        ServiceEvent::AuthFailed { reason, .. } => Err(DiagnosticError::AuthRejected(reason)),
        ServiceEvent::RequestRejected { reason } => Err(DiagnosticError::Protocol(reason)),
        _ => Err(DiagnosticError::Protocol(ProtocolError::InvalidMessage)),
    }
}

fn wait_for_auth_grant(
    pipe_name: &str,
    wake_event: ServiceEvent,
) -> Result<AuthGrant, DiagnosticError> {
    if !matches!(wake_event, ServiceEvent::AuthStarted { .. }) {
        return auth_grant_from_event(wake_event);
    }

    let ServiceEvent::AuthStarted { session_id } = wake_event else {
        unreachable!();
    };
    for _ in 0..AUTH_RESULT_POLL_ATTEMPTS {
        thread::sleep(AUTH_RESULT_POLL_DELAY);
        let event = send_fetch_auth_result(pipe_name, session_id.clone())?;
        print_wake_auth_event(&event);
        match event {
            ServiceEvent::AuthStarted { .. } => continue,
            other => return auth_grant_from_event(other),
        }
    }

    Err(DiagnosticError::Protocol(
        ProtocolError::TransportUnavailable,
    ))
}

fn run_test_camera(args: &[String]) -> Result<(), DiagnosticError> {
    let mut provider = build_camera_provider(args)?;
    let sources = provider.list_sources()?;
    print_camera_sources(&sources);

    let camera_id = selected_camera_id(args, &sources)?;
    provider.open(&camera_id)?;
    let frame = provider.read_frame()?;
    provider.close();

    println!(
        "frame: width={} height={} format={:?} bytes={}",
        frame.width,
        frame.height,
        frame.format,
        frame.data.len()
    );
    Ok(())
}

fn run_list_cameras(args: &[String]) -> Result<(), DiagnosticError> {
    let provider = build_camera_provider(args)?;
    let sources = provider.list_sources()?;
    print_camera_sources(&sources);
    Ok(())
}

fn run_camera_open_benchmark(args: &[String]) -> Result<(), DiagnosticError> {
    use opencv::{
        prelude::{MatTraitConst, VideoCaptureTrait, VideoCaptureTraitConst},
        videoio::{self, VideoCapture},
    };
    use std::time::Instant;

    let camera_id = selected_camera_id(args, &[])?;
    let camera_index = camera_id.camera_index()?;
    let backend_filter = argument_value(args, "--backend");
    let backends = [
        ("msmf", videoio::CAP_MSMF),
        ("dshow", videoio::CAP_DSHOW),
        ("any", videoio::CAP_ANY),
    ];

    for (backend_name, backend) in backends {
        if backend_filter.is_some_and(|filter| filter != backend_name) {
            continue;
        }

        let open_started = Instant::now();
        let capture_result = VideoCapture::new(camera_index, backend);
        let open_ms = open_started.elapsed().as_millis();
        let Ok(mut capture) = capture_result else {
            println!("backend={backend_name} open_ms={open_ms} opened=false error=new_failed");
            continue;
        };

        if let Some(width) = optional_u32(args, "--frame-width")? {
            let _ = capture.set(videoio::CAP_PROP_FRAME_WIDTH, f64::from(width));
        }
        if let Some(height) = optional_u32(args, "--frame-height")? {
            let _ = capture.set(videoio::CAP_PROP_FRAME_HEIGHT, f64::from(height));
        }

        let opened = capture.is_opened().unwrap_or(false);
        if !opened {
            println!("backend={backend_name} open_ms={open_ms} opened=false");
            let _ = capture.release();
            continue;
        }

        let read_started = Instant::now();
        let mut frame = opencv::core::Mat::default();
        let read_ok = capture.read(&mut frame).unwrap_or(false);
        let read_ms = read_started.elapsed().as_millis();
        println!(
            "backend={backend_name} open_ms={open_ms} opened=true read_ms={read_ms} read_ok={read_ok} width={} height={} empty={}",
            frame.cols(),
            frame.rows(),
            frame.empty()
        );
        let _ = capture.release();
    }

    Ok(())
}

fn run_test_face(args: &[String]) -> Result<(), DiagnosticError> {
    let image_path = argument_value(args, "--image").ok_or(DiagnosticError::InvalidArgument)?;
    let mut model_provider = build_loaded_model_provider(args)?;
    let frame = OpenCvFaceModelProvider::read_image_frame(image_path)?;
    let faces = model_provider.detect(&frame)?;
    println!("detected_face_count: {}", faces.len());
    if faces.len() == 1 {
        let embedding = model_provider.extract(&frame, &faces[0])?;
        println!("embedding_dimensions: {}", embedding.values.len());
        println!("face_confidence: {}", faces[0].confidence);
    }
    model_provider.unload_models();
    Ok(())
}

fn run_enroll_face(args: &[String]) -> Result<(), DiagnosticError> {
    let image_path = argument_value(args, "--image").ok_or(DiagnosticError::InvalidArgument)?;
    let template_out =
        argument_value(args, "--template-out").ok_or(DiagnosticError::InvalidArgument)?;
    let user_id = UserId(
        argument_value(args, "--user-id")
            .unwrap_or("dev-user")
            .to_owned(),
    );
    let template_ref = FaceTemplateRef(
        argument_value(args, "--template-ref")
            .map(str::to_owned)
            .unwrap_or_else(default_template_ref),
    );
    let model_provider = build_loaded_model_provider(args)?;
    let mut enrollment = FaceEnrollmentService::new(model_provider);
    let frame = OpenCvFaceModelProvider::read_image_frame(image_path)?;
    let outcome = enrollment.enroll_frame(&frame, user_id, template_ref)?;

    fs::write(template_out, outcome.template.to_json_bytes()?)
        .map_err(|_| DiagnosticError::IoFailed)?;
    println!("template_saved: {template_out}");
    println!("detected_face_count: {}", outcome.detected_face_count);
    Ok(())
}

fn run_enroll_camera(args: &[String]) -> Result<(), DiagnosticError> {
    let template_out =
        argument_value(args, "--template-out").ok_or(DiagnosticError::InvalidArgument)?;
    let user_id = UserId(
        argument_value(args, "--user-id")
            .unwrap_or("dev-user")
            .to_owned(),
    );
    let template_ref = FaceTemplateRef(
        argument_value(args, "--template-ref")
            .map(str::to_owned)
            .unwrap_or_else(default_template_ref),
    );
    let max_frames = optional_u32(args, "--max-frames")?.unwrap_or(30);

    let mut provider = build_camera_provider(args)?;
    let camera_id = selected_camera_id_without_forced_scan(args, &provider)?;
    provider = build_camera_provider_for_camera(args, &camera_id)?;
    provider.open(&camera_id)?;

    let model_provider = build_loaded_model_provider(args)?;
    let mut enrollment = FaceEnrollmentService::new(model_provider);
    let mut last_face_error = None;
    for _ in 0..max_frames {
        let frame = provider.read_frame()?;
        match enrollment.enroll_frame(&frame, user_id.clone(), template_ref.clone()) {
            Ok(outcome) => {
                provider.close();
                fs::write(template_out, outcome.template.to_json_bytes()?)
                    .map_err(|_| DiagnosticError::IoFailed)?;
                println!("template_saved: {template_out}");
                println!("camera_enrollment_passed: true");
                println!("detected_face_count: {}", outcome.detected_face_count);
                return Ok(());
            }
            Err(error @ FaceEngineError::NoFaceDetected)
            | Err(error @ FaceEngineError::MultipleFacesDetected) => {
                last_face_error = Some(error);
            }
            Err(error) => {
                provider.close();
                return Err(DiagnosticError::Face(error));
            }
        }
    }

    provider.close();
    Err(DiagnosticError::Face(
        last_face_error.unwrap_or(FaceEngineError::NoFaceDetected),
    ))
}

fn run_guided_enroll(args: &[String]) -> Result<(), DiagnosticError> {
    let output_dir =
        PathBuf::from(argument_value(args, "--output-dir").unwrap_or("face-enrollment"));
    fs::create_dir_all(&output_dir).map_err(|_| DiagnosticError::IoFailed)?;
    write_guided_enrollment_status(&output_dir, "starting", None, 0, None, None)?;
    match run_guided_enroll_with_output_dir(args, &output_dir) {
        Ok(()) => Ok(()),
        Err(error) => {
            if guided_enrollment_status_is_still_starting(&output_dir) {
                let _ = write_guided_enrollment_status(
                    &output_dir,
                    "failed",
                    None,
                    0,
                    None,
                    Some(guided_enrollment_startup_error_code(&error)),
                );
            }
            Err(error)
        }
    }
}

fn run_guided_enroll_with_output_dir(
    args: &[String],
    output_dir: &Path,
) -> Result<(), DiagnosticError> {
    let user_id = UserId(
        argument_value(args, "--user-id")
            .unwrap_or("dev-user")
            .to_owned(),
    );
    let accepted_frames_per_step = optional_u32(args, "--accepted-frames-per-step")?.unwrap_or(6);
    let max_frames_per_step = optional_u32(args, "--max-frames-per-step")?
        .or(optional_u32(args, "--frames-per-step")?)
        .unwrap_or(180);
    let max_wait_frames_per_step =
        optional_u32(args, "--max-wait-frames-per-step")?.unwrap_or(max_frames_per_step);
    let pose_ready_consecutive = optional_u32(args, "--pose-ready-consecutive")?.unwrap_or(3);
    let pose_ready_min_fit_score = optional_f32(args, "--pose-ready-min-fit")?.unwrap_or(0.25);
    let frame_delay_ms = optional_u32(args, "--frame-delay-ms")?.unwrap_or(0);
    let save_debug_images = args.iter().any(|arg| arg == "--save-debug-images");
    let allow_partial_enrollment = args.iter().any(|arg| arg == "--allow-partial-enrollment");
    let frame_match_threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PROJECT_FACE_MATCH_THRESHOLD);
    let enrollment_id = argument_value(args, "--enrollment-id")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("guided-enrollment-{}", current_time_unix_ms()));

    let mut provider = build_camera_provider(args)?;
    let camera_id = selected_camera_id_without_forced_scan(args, &provider)?;
    provider = build_camera_provider_for_camera(args, &camera_id)?;
    provider.open(&camera_id)?;

    let model_provider = build_loaded_model_provider(args)?;
    let quality_policy = FaceQualityPolicy {
        min_pose_fit_score: pose_ready_min_fit_score,
        ..FaceQualityPolicy::default()
    };
    let config = GuidedEnrollmentConfig {
        frames_per_step: max_frames_per_step,
        frame_match_threshold,
        quality_policy,
        ..GuidedEnrollmentConfig::default()
    };
    let pose_provider = build_pose_provider(args)?;
    let guided_steps = GuidedEnrollmentStep::supported_ordered_steps(pose_provider.capabilities());
    if guided_steps.is_empty() {
        provider.close();
        write_guided_enrollment_status(
            output_dir,
            "failed",
            None,
            0,
            None,
            Some("model_unavailable"),
        )?;
        return Err(DiagnosticError::InvalidArgument);
    }
    let mut enrollment = GuidedFaceEnrollmentService::new(model_provider, pose_provider, config);
    let mut frame_index = 0_u32;
    let debug_frames_dir = output_dir.join("debug_frames");
    let aligned_faces_dir = output_dir.join("aligned_faces");
    if save_debug_images {
        fs::create_dir_all(&debug_frames_dir).map_err(|_| DiagnosticError::IoFailed)?;
        fs::create_dir_all(&aligned_faces_dir).map_err(|_| DiagnosticError::IoFailed)?;
    }

    println!(
        "pose_provider: {} capabilities={:?}",
        enrollment.pose_provider_name(),
        enrollment.pose_provider_capabilities()
    );
    let mut partial_enrollment_reasons = Vec::new();
    for (step_index, step) in guided_steps.iter().enumerate() {
        write_guided_enrollment_status(
            output_dir,
            "waiting_for_pose",
            Some(*step),
            0,
            Some(pose_ready_consecutive),
            None,
        )?;
        println!(
            "[{}/{}] {}",
            step_index + 1,
            guided_steps.len(),
            step.prompt()
        );

        let mut pose_ready_count = 0_u32;
        let mut wait_attempted_frame_count = 0_u32;
        while pose_ready_count < pose_ready_consecutive
            && wait_attempted_frame_count < max_wait_frames_per_step
        {
            let frame = provider.read_frame()?;
            emit_guided_enrollment_preview_frame(&enrollment_id, &frame, frame_index)?;
            let observation = enrollment.preview_frame_for_step(&frame, *step, frame_index)?;
            let last_frame_result =
                frame_result_for_guided_observation(&observation, pose_ready_min_fit_score);
            if guided_observation_passes_pose_ready_gate(&observation, pose_ready_min_fit_score) {
                pose_ready_count = pose_ready_count.saturating_add(1);
                println!(
                    "step={} waiting_for_pose=false pose_ready_count={}/{} quality_score={} pose_fit_score={} pose_estimate={:?}",
                    step.label(),
                    pose_ready_count,
                    pose_ready_consecutive,
                    observation.quality_score,
                    observation.pose_fit_score,
                    observation.pose_estimate
                );
            } else {
                pose_ready_count = 0;
                if wait_attempted_frame_count.is_multiple_of(15) {
                    println!(
                        "step={} waiting_for_pose=true pose_ready_count=0/{} reject_reason={:?} quality_score={} pose_fit_score={} pose_estimate={:?}",
                        step.label(),
                        pose_ready_consecutive,
                        observation.reject_reason,
                        observation.quality_score,
                        observation.pose_fit_score,
                        observation.pose_estimate
                    );
                }
            }
            write_guided_enrollment_status(
                output_dir,
                "waiting_for_pose",
                Some(*step),
                pose_ready_count,
                Some(pose_ready_consecutive),
                Some(last_frame_result),
            )?;
            if save_debug_images {
                save_guided_debug_images(
                    &mut enrollment,
                    &frame,
                    &observation,
                    &debug_frames_dir,
                    &aligned_faces_dir,
                )?;
            }
            frame_index = frame_index.saturating_add(1);
            wait_attempted_frame_count = wait_attempted_frame_count.saturating_add(1);
            if frame_delay_ms > 0 {
                thread::sleep(Duration::from_millis(u64::from(frame_delay_ms)));
            }
        }
        if pose_ready_count < pose_ready_consecutive {
            let incomplete_step = DiagnosticError::GuidedEnrollmentStepIncomplete {
                step: format!("{}:waiting_for_pose", step.label()),
                accepted_frame_count: pose_ready_count,
                required_frame_count: pose_ready_consecutive,
                attempted_frame_count: wait_attempted_frame_count,
            };
            if allow_partial_enrollment || guided_enrollment_step_is_optional(*step) {
                println!("partial_enrollment_step_skipped: {incomplete_step}");
                partial_enrollment_reasons.push(incomplete_step.to_string());
                continue;
            }
            provider.close();
            write_guided_enrollment_status(
                output_dir,
                "failed",
                Some(*step),
                pose_ready_count,
                Some(pose_ready_consecutive),
                Some("pose_not_ready"),
            )?;
            return Err(incomplete_step);
        }

        println!(
            "step={} pose_confirmed=true recording_started=true",
            step.label()
        );
        write_guided_enrollment_status(
            output_dir,
            "capturing",
            Some(*step),
            0,
            Some(accepted_frames_per_step),
            None,
        )?;

        let mut accepted_frame_count = 0_u32;
        let mut attempted_frame_count = 0_u32;
        while accepted_frame_count < accepted_frames_per_step
            && attempted_frame_count < max_frames_per_step
        {
            let frame = provider.read_frame()?;
            emit_guided_enrollment_preview_frame(&enrollment_id, &frame, frame_index)?;
            let observation = enrollment.observe_frame(
                &frame,
                &user_id,
                *step,
                frame_index,
                current_time_unix_ms(),
            )?;
            let last_frame_result =
                frame_result_for_guided_observation(&observation, pose_ready_min_fit_score);
            if guided_observation_passes_sample_gate(&observation, pose_ready_min_fit_score) {
                accepted_frame_count = accepted_frame_count.saturating_add(1);
                println!(
                    "step={} recording_frame_count={}/{} quality_score={} pose_fit_score={} pose_estimate={:?}",
                    step.label(),
                    accepted_frame_count,
                    accepted_frames_per_step,
                    observation.quality_score,
                    observation.pose_fit_score,
                    observation.pose_estimate
                );
            } else if attempted_frame_count.is_multiple_of(15) {
                println!(
                    "step={} recording_waiting_for_valid_frame=true recording_frame_count={}/{} reject_reason={:?} quality_score={} pose_fit_score={} pose_estimate={:?}",
                    step.label(),
                    accepted_frame_count,
                    accepted_frames_per_step,
                    observation.reject_reason,
                    observation.quality_score,
                    observation.pose_fit_score,
                    observation.pose_estimate
                );
            }
            write_guided_enrollment_status(
                output_dir,
                "capturing",
                Some(*step),
                accepted_frame_count,
                Some(accepted_frames_per_step),
                Some(last_frame_result),
            )?;
            if save_debug_images {
                save_guided_debug_images(
                    &mut enrollment,
                    &frame,
                    &observation,
                    &debug_frames_dir,
                    &aligned_faces_dir,
                )?;
            }
            frame_index = frame_index.saturating_add(1);
            attempted_frame_count = attempted_frame_count.saturating_add(1);
            if frame_delay_ms > 0 {
                thread::sleep(Duration::from_millis(u64::from(frame_delay_ms)));
            }
        }
        if accepted_frame_count < accepted_frames_per_step {
            let incomplete_step = DiagnosticError::GuidedEnrollmentStepIncomplete {
                step: step.label().to_owned(),
                accepted_frame_count,
                required_frame_count: accepted_frames_per_step,
                attempted_frame_count,
            };
            if allow_partial_enrollment || guided_enrollment_step_is_optional(*step) {
                println!("partial_enrollment_step_incomplete: {incomplete_step}");
                partial_enrollment_reasons.push(incomplete_step.to_string());
                continue;
            }
            provider.close();
            write_guided_enrollment_status(
                output_dir,
                "failed",
                Some(*step),
                accepted_frame_count,
                Some(accepted_frames_per_step),
                Some("quality_rejected"),
            )?;
            return Err(incomplete_step);
        }
    }

    provider.close();
    write_guided_enrollment_status(output_dir, "finishing", None, 0, None, None)?;
    let template_set = enrollment.finish(
        user_id.clone(),
        enrollment_id,
        "yunet".to_owned(),
        "2023mar".to_owned(),
        current_time_unix_ms(),
    );
    let report = build_guided_enrollment_report(&template_set);

    let selected_templates_path = output_dir.join("selected_templates.json");
    let report_path = output_dir.join("enrollment_report.json");
    let partial_report_path = output_dir.join("partial_enrollment_reasons.json");
    fs::write(&selected_templates_path, template_set.to_json_bytes()?)
        .map_err(|_| DiagnosticError::IoFailed)?;
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&report).map_err(|_| DiagnosticError::IoFailed)?,
    )
    .map_err(|_| DiagnosticError::IoFailed)?;
    if allow_partial_enrollment {
        fs::write(
            &partial_report_path,
            serde_json::to_vec_pretty(&partial_enrollment_reasons)
                .map_err(|_| DiagnosticError::IoFailed)?,
        )
        .map_err(|_| DiagnosticError::IoFailed)?;
    }

    println!("guided_enrollment_completed: true");
    println!(
        "partial_enrollment_used: {}",
        allow_partial_enrollment && !partial_enrollment_reasons.is_empty()
    );
    println!("user_id: {}", user_id.0);
    println!(
        "selected_template_count: {}",
        report.selected_template_count
    );
    println!("rejected_sample_count: {}", report.rejected_sample_count);
    println!(
        "selected_templates_path: {}",
        selected_templates_path.display()
    );
    println!("enrollment_report_path: {}", report_path.display());
    if allow_partial_enrollment {
        println!(
            "partial_enrollment_reasons_path: {}",
            partial_report_path.display()
        );
    }
    Ok(())
}

fn guided_enrollment_startup_error_code(error: &DiagnosticError) -> &'static str {
    match error {
        DiagnosticError::Video(VideoError::CameraNotFound) => "camera_not_found",
        DiagnosticError::Video(VideoError::OpenFailed) => "camera_open_failed",
        DiagnosticError::Video(VideoError::CameraAlreadyOpen) => "camera_already_open",
        DiagnosticError::Video(VideoError::CameraNotOpen) => "camera_not_open",
        DiagnosticError::Video(VideoError::ReadFailed) => "camera_read_failed",
        DiagnosticError::Video(VideoError::EmptyFrame) => "camera_empty_frame",
        DiagnosticError::Video(VideoError::UnsupportedFormat) => "camera_unsupported_format",
        DiagnosticError::Face(FaceEngineError::ModelPathMissing) => "model_path_missing",
        DiagnosticError::Face(FaceEngineError::ModelLoadFailed) => "model_load_failed",
        _ => "guided_enrollment_failed",
    }
}

fn guided_enrollment_status_is_still_starting(output_dir: &Path) -> bool {
    let status_path = output_dir.join(ENROLLMENT_STATUS_FILE_NAME);
    let Ok(status_bytes) = fs::read(status_path) else {
        return true;
    };
    let Ok(status) = serde_json::from_slice::<serde_json::Value>(&status_bytes) else {
        return true;
    };
    status
        .get("session_state")
        .and_then(serde_json::Value::as_str)
        .map(|session_state| session_state == "starting")
        .unwrap_or(true)
}

fn run_enrollment_report(args: &[String]) -> Result<(), DiagnosticError> {
    let template_path =
        argument_value(args, "--template").ok_or(DiagnosticError::InvalidArgument)?;
    let template_set = read_template_set(template_path)?;
    let report = build_guided_enrollment_report(&template_set);

    println!("enrollment_id: {}", report.enrollment_id);
    println!("user_id: {}", report.user_id);
    println!(
        "selected_template_count: {}",
        report.selected_template_count
    );
    println!("rejected_sample_count: {}", report.rejected_sample_count);
    println!(
        "average_selected_quality_score: {:?}",
        report.quality_summary.average_selected_quality_score
    );
    println!(
        "minimum_selected_quality_score: {:?}",
        report.quality_summary.minimum_selected_quality_score
    );
    for count in report.pose_group_counts {
        println!(
            "pose_group_count: {:?} {}",
            count.pose_group, count.selected_template_count
        );
    }
    for count in report.reject_reason_counts {
        println!(
            "reject_reason_count: {:?} {}",
            count.reason, count.rejected_sample_count
        );
    }
    Ok(())
}

fn run_face_debug_snapshot_command(args: &[String]) -> Result<(), DiagnosticError> {
    let output_dir = PathBuf::from(argument_value(args, "--output-dir").unwrap_or("face-debug"));
    let yunet_model_path = model_path(args, "--yunet-model", DEFAULT_YUNET_MODEL_PATH);
    let sface_model_path = model_path(args, "--sface-model", DEFAULT_SFACE_MODEL_PATH);
    let threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PROJECT_FACE_MATCH_THRESHOLD);
    let mut model_config = OpenCvFaceModelConfig::new(yunet_model_path, sface_model_path);
    model_config.recognizer.match_threshold = threshold;

    run_face_debug_snapshot(FaceDebugSnapshotConfig {
        output_dir,
        scenario: argument_value(args, "--scenario")
            .unwrap_or("unlabeled")
            .to_owned(),
        start_delay_seconds: optional_u32(args, "--start-delay-seconds")?.unwrap_or(3),
        camera_id: argument_value(args, "--camera-id").map(|value| CameraId(value.to_owned())),
        max_camera_index: 8,
        requested_frame_width: optional_u32(args, "--frame-width")?,
        requested_frame_height: optional_u32(args, "--frame-height")?,
        frames: optional_u32(args, "--frames")?.unwrap_or(30),
        frame_delay_ms: optional_u32(args, "--frame-delay-ms")?.unwrap_or(60),
        model_config,
        save_aligned_faces: args.iter().any(|arg| arg == "--save-aligned-face"),
    })?;

    Ok(())
}

fn run_face_calibrate_command(args: &[String]) -> Result<(), DiagnosticError> {
    let template_path =
        argument_value(args, "--template").ok_or(DiagnosticError::InvalidArgument)?;
    let output_dir =
        PathBuf::from(argument_value(args, "--output-dir").unwrap_or("face-calibration"));
    let yunet_model_path = model_path(args, "--yunet-model", DEFAULT_YUNET_MODEL_PATH);
    let sface_model_path = model_path(args, "--sface-model", DEFAULT_SFACE_MODEL_PATH);
    let threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PROJECT_FACE_MATCH_THRESHOLD);
    let mut model_config = OpenCvFaceModelConfig::new(yunet_model_path, sface_model_path);
    model_config.recognizer.match_threshold = threshold;

    run_face_calibration(FaceCalibrationConfig {
        output_dir,
        scenario: argument_value(args, "--scenario")
            .unwrap_or("unlabeled")
            .to_owned(),
        start_delay_seconds: optional_u32(args, "--start-delay-seconds")?.unwrap_or(3),
        camera_id: argument_value(args, "--camera-id").map(|value| CameraId(value.to_owned())),
        max_camera_index: 8,
        requested_frame_width: optional_u32(args, "--frame-width")?,
        requested_frame_height: optional_u32(args, "--frame-height")?,
        frames: optional_u32(args, "--frames")?.unwrap_or(100),
        frame_delay_ms: optional_u32(args, "--frame-delay-ms")?.unwrap_or(60),
        model_config,
        templates: read_recognition_templates(template_path)?,
        threshold_min: optional_f32(args, "--threshold-min")?.unwrap_or(0.40),
        threshold_max: optional_f32(args, "--threshold-max")?.unwrap_or(0.80),
        threshold_step: optional_f32(args, "--threshold-step")?.unwrap_or(0.05),
        required_consecutive_match_count: optional_u32(args, "--required-consecutive")?
            .unwrap_or(3),
    })?;

    Ok(())
}

fn run_liveness_screen_debug_command(args: &[String]) -> Result<(), DiagnosticError> {
    let output_dir =
        PathBuf::from(argument_value(args, "--output-dir").unwrap_or("liveness-debug\\screen"));
    let yunet_model_path = model_path(args, "--yunet-model", DEFAULT_YUNET_MODEL_PATH);
    let sface_model_path = model_path(args, "--sface-model", DEFAULT_SFACE_MODEL_PATH);
    let threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PROJECT_FACE_MATCH_THRESHOLD);
    let mut model_config = OpenCvFaceModelConfig::new(yunet_model_path, sface_model_path);
    model_config.recognizer.match_threshold = threshold;

    run_liveness_screen_debug(LivenessScreenDebugConfig {
        output_dir,
        camera_id: argument_value(args, "--camera-id").map(|value| CameraId(value.to_owned())),
        max_camera_index: 8,
        requested_frame_width: optional_u32(args, "--frame-width")?,
        requested_frame_height: optional_u32(args, "--frame-height")?,
        frames: optional_u32(args, "--frames")?.unwrap_or(60),
        frame_delay_ms: optional_u32(args, "--frame-delay-ms")?.unwrap_or(60),
        model_config,
        screen_replay_geometry_provider_config: screen_replay_geometry_config(args)?,
        minifasnet_provider_config: minifasnet_liveness_config(args)?,
        save_debug_images: args.iter().any(|arg| arg == "--save-debug-images"),
        save_minifasnet_crops: args.iter().any(|arg| arg == "--save-minifasnet-crops"),
    })?;

    Ok(())
}

fn screen_replay_geometry_config(
    args: &[String],
) -> Result<Option<ScreenReplayLivenessProviderConfig>, DiagnosticError> {
    if !args
        .iter()
        .any(|arg| arg == "--enable-screen-geometry-diagnostics")
    {
        return Ok(None);
    }

    Ok(Some(ScreenReplayLivenessProviderConfig {
        binary_threshold: optional_f64(args, "--binary-threshold")?.unwrap_or(150.0),
        binary_mask_upper_threshold: optional_f64(args, "--binary-mask-upper-threshold")?
            .unwrap_or(50.0),
        min_screen_area_ratio: optional_f32(args, "--min-screen-area-ratio")?.unwrap_or(0.08),
        max_screen_area_ratio: optional_f32(args, "--max-screen-area-ratio")?.unwrap_or(0.90),
        min_rectangularity_score: optional_f32(args, "--min-rectangularity-score")?.unwrap_or(0.45),
        min_brightness_contrast_score: optional_f32(args, "--min-brightness-contrast-score")?
            .unwrap_or(0.05),
        min_face_inside_screen_ratio: optional_f32(args, "--min-face-inside-screen-ratio")?
            .unwrap_or(0.95),
        min_screen_aspect_ratio: optional_f32(args, "--min-screen-aspect-ratio")?.unwrap_or(0.35),
        max_screen_aspect_ratio: optional_f32(args, "--max-screen-aspect-ratio")?.unwrap_or(3.20),
    }))
}

fn minifasnet_liveness_config(
    args: &[String],
) -> Result<Option<MiniFasNetLivenessProviderConfig>, DiagnosticError> {
    if args.iter().any(|arg| arg == "--disable-minifasnet") {
        return Ok(None);
    }

    let explicit_model_path = argument_value(args, "--minifasnet-model").map(PathBuf::from);
    let model_path = explicit_model_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MINIFASNET_MODEL_PATH));
    if explicit_model_path.is_none() && !model_path.exists() {
        return Ok(None);
    }

    Ok(Some(MiniFasNetLivenessProviderConfig {
        model_path,
        crop_scale: optional_f32(args, "--minifasnet-crop-scale")?.unwrap_or(2.7),
        input_width: 80,
        input_height: 80,
        min_live_score: optional_f32(args, "--minifasnet-min-live-score")?.unwrap_or(0.80),
        min_spoof_score: optional_f32(args, "--minifasnet-min-spoof-score")?.unwrap_or(0.70),
        reject_on_model_spoof: !args.iter().any(|arg| arg == "--minifasnet-diagnostic-only"),
    }))
}

fn run_threshold_preview_command(args: &[String]) -> Result<(), DiagnosticError> {
    run_threshold_preview(ThresholdPreviewConfig {
        camera_id: argument_value(args, "--camera-id").map(|value| CameraId(value.to_owned())),
        max_camera_index: 8,
        requested_frame_width: optional_u32(args, "--frame-width")?,
        requested_frame_height: optional_u32(args, "--frame-height")?,
        method: threshold_preview_method(args)?,
        adaptive_block_size: optional_i32(args, "--adaptive-block-size")?.unwrap_or(31),
        adaptive_c: optional_f64(args, "--adaptive-c")?.unwrap_or(5.0),
        binary_threshold: optional_f64(args, "--binary-threshold")?.unwrap_or(150.0),
        binary_mask_upper_threshold: optional_f64(args, "--binary-mask-upper-threshold")?
            .unwrap_or(50.0),
        frame_delay_ms: optional_u32(args, "--frame-delay-ms")?.unwrap_or(1),
    })?;

    Ok(())
}

fn build_pose_provider(args: &[String]) -> Result<Box<dyn FacePoseProvider>, DiagnosticError> {
    match argument_value(args, "--pose-provider").unwrap_or("landmark") {
        "landmark" => Ok(Box::new(LandmarkFacePoseProvider)),
        #[cfg(feature = "mediapipe-pose")]
        "mediapipe" => {
            let bridge_dll_path = PathBuf::from(
                argument_value(args, "--mediapipe-bridge")
                    .unwrap_or(DEFAULT_MEDIAPIPE_BRIDGE_DLL_PATH),
            );
            let face_landmarker_task_path = PathBuf::from(
                argument_value(args, "--mediapipe-model")
                    .unwrap_or(DEFAULT_MEDIAPIPE_FACE_LANDMARKER_TASK_PATH),
            );
            let config =
                MediaPipeFacePoseProviderConfig::new(bridge_dll_path, face_landmarker_task_path);
            MediaPipeFacePoseProvider::load(config)
                .map(|provider| Box::new(provider) as Box<dyn FacePoseProvider>)
                .map_err(DiagnosticError::FacePoseProvider)
        }
        #[cfg(not(feature = "mediapipe-pose"))]
        "mediapipe" => Err(DiagnosticError::MediaPipeFeatureDisabled),
        _ => Err(DiagnosticError::InvalidArgument),
    }
}

fn save_guided_debug_images(
    enrollment: &mut GuidedFaceEnrollmentService<
        OpenCvFaceModelProvider,
        Box<dyn FacePoseProvider>,
    >,
    frame: &video_provider::VideoFrame,
    observation: &GuidedFrameObservation,
    debug_frames_dir: &std::path::Path,
    aligned_faces_dir: &std::path::Path,
) -> Result<(), DiagnosticError> {
    let base_name = format!(
        "{:05}_{}_{}",
        observation.frame_index,
        observation.step.label(),
        if observation.accepted_for_step {
            "accepted"
        } else {
            "rejected"
        }
    );
    let debug_frame_path = debug_frames_dir.join(format!("{base_name}.jpg"));
    OpenCvFaceModelProvider::write_detection_debug_frame(
        frame,
        &observation.detected_faces,
        &debug_frame_path,
    )?;

    if let Some(face) = observation.detected_faces.first() {
        let aligned_face_path = aligned_faces_dir.join(format!("{base_name}.jpg"));
        enrollment
            .model_provider_mut()
            .write_aligned_face(frame, face, &aligned_face_path)?;
    }

    Ok(())
}

fn write_guided_enrollment_status(
    output_dir: &Path,
    session_state: &str,
    step: Option<GuidedEnrollmentStep>,
    accepted_sample_count: u32,
    required_sample_count: Option<u32>,
    last_frame_result: Option<&str>,
) -> Result<(), DiagnosticError> {
    let current_step = step.map(GuidedEnrollmentStep::label);
    let current_instruction_code = step.map(instruction_code_for_guided_step);
    let status_path = output_dir.join(ENROLLMENT_STATUS_FILE_NAME);
    let status = serde_json::json!({
        "session_state": session_state,
        "current_step": current_step,
        "current_instruction_code": current_instruction_code,
        "accepted_sample_count": accepted_sample_count,
        "required_sample_count": required_sample_count,
        "last_frame_result": last_frame_result,
    });
    fs::write(
        status_path,
        serde_json::to_vec_pretty(&status).map_err(|_| DiagnosticError::IoFailed)?,
    )
    .map_err(|_| DiagnosticError::IoFailed)
}

fn emit_guided_enrollment_preview_frame(
    enrollment_id: &str,
    frame: &video_provider::VideoFrame,
    frame_index: u32,
) -> Result<(), DiagnosticError> {
    let image_base64 = encode_preview_frame_base64(frame)?;
    let event = serde_json::json!({
        "enrollment_session_id": enrollment_id,
        "frame_seq": frame_index,
        "updated_at_unix_ms": current_time_unix_ms(),
        "mime_type": "image/jpeg",
        "image_base64": image_base64,
    });
    println!(
        "{PREVIEW_EVENT_PREFIX}{}",
        serde_json::to_string(&event).map_err(|_| DiagnosticError::IoFailed)?
    );
    Ok(())
}

fn encode_preview_frame_base64(
    frame: &video_provider::VideoFrame,
) -> Result<String, DiagnosticError> {
    let channels = match frame.format {
        PixelFormat::Bgr8 | PixelFormat::Rgb8 => 3,
        PixelFormat::Gray8 => 1,
    };
    let mat = Mat::from_slice(&frame.data).map_err(|_| DiagnosticError::IoFailed)?;
    let mat = mat
        .reshape(channels, frame.height as i32)
        .map_err(|_| DiagnosticError::IoFailed)?;
    let mut image = mat.try_clone().map_err(|_| DiagnosticError::IoFailed)?;
    if frame.format == PixelFormat::Rgb8 {
        let mut bgr = Mat::default();
        imgproc::cvt_color(
            &image,
            &mut bgr,
            imgproc::COLOR_RGB2BGR,
            0,
            AlgorithmHint::ALGO_HINT_DEFAULT,
        )
        .map_err(|_| DiagnosticError::IoFailed)?;
        image = bgr;
    }

    let target_width = 480;
    if image.cols() > target_width {
        let target_height = (image.rows() * target_width / image.cols()).max(1);
        let mut resized = Mat::default();
        imgproc::resize(
            &image,
            &mut resized,
            opencv::core::Size::new(target_width, target_height),
            0.0,
            0.0,
            imgproc::INTER_AREA,
        )
        .map_err(|_| DiagnosticError::IoFailed)?;
        image = resized;
    }

    let mut encoded = Vector::<u8>::new();
    let params = Vector::from_slice(&[imgcodecs::IMWRITE_JPEG_QUALITY, 70]);
    imgcodecs::imencode(".jpg", &image, &mut encoded, &params)
        .map_err(|_| DiagnosticError::IoFailed)?
        .then_some(())
        .ok_or(DiagnosticError::IoFailed)?;
    Ok(BASE64_STANDARD.encode(encoded.to_vec()))
}

fn instruction_code_for_guided_step(step: GuidedEnrollmentStep) -> &'static str {
    match step {
        GuidedEnrollmentStep::FrontalPrimary => "look_at_camera",
        GuidedEnrollmentStep::YawLeftMild => "turn_head_left",
        GuidedEnrollmentStep::YawRightMild => "turn_head_right",
        GuidedEnrollmentStep::PitchDownMild => "tilt_head_down",
        GuidedEnrollmentStep::PitchUpMild => "tilt_head_up",
        GuidedEnrollmentStep::BlinkMotion => "blink_once",
    }
}

fn guided_enrollment_step_is_optional(step: GuidedEnrollmentStep) -> bool {
    matches!(
        step,
        GuidedEnrollmentStep::PitchDownMild | GuidedEnrollmentStep::PitchUpMild
    )
}

fn frame_result_for_guided_observation(
    observation: &GuidedFrameObservation,
    min_pose_fit_score: f32,
) -> &'static str {
    if guided_observation_passes_pose_ready_gate(observation, min_pose_fit_score) {
        return "face_accepted";
    }

    match observation.reject_reason {
        Some(FaceSampleRejectReason::NoFaceDetected) => "no_face_detected",
        Some(FaceSampleRejectReason::MultipleFacesDetected) => "multiple_faces_detected",
        Some(FaceSampleRejectReason::PoseOutOfExpectedRange) => "pose_not_ready",
        Some(_) => "quality_rejected",
        None => "pose_not_ready",
    }
}

fn guided_observation_passes_pose_ready_gate(
    observation: &GuidedFrameObservation,
    pose_ready_min_fit_score: f32,
) -> bool {
    matches!(
        observation.reject_reason,
        None | Some(FaceSampleRejectReason::BlurTooHigh)
            | Some(FaceSampleRejectReason::UnderExposed)
    ) && observation.pose_fit_score >= pose_ready_min_fit_score
}

fn guided_observation_passes_sample_gate(
    observation: &GuidedFrameObservation,
    pose_ready_min_fit_score: f32,
) -> bool {
    observation.accepted_for_step
        && observation.reject_reason.is_none()
        && observation.pose_fit_score >= pose_ready_min_fit_score
}

fn run_presence_check_once(args: &[String]) -> Result<(), DiagnosticError> {
    let template_path =
        argument_value(args, "--template").ok_or(DiagnosticError::InvalidArgument)?;
    let threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD);
    let templates = read_recognition_templates(template_path)?;

    let mut camera_provider = build_camera_provider(args)?;
    let sources = camera_provider.list_sources()?;
    let camera_id = selected_camera_id(args, &sources)?;
    camera_provider.open(&camera_id)?;
    let frame = camera_provider.read_frame();
    camera_provider.close();
    let frame = frame?;

    let mut model_provider = build_loaded_model_provider_with_threshold(args, threshold)?;
    let faces = model_provider.detect(&frame)?;
    println!("presence_frame_captured: true");
    println!("camera_id: {}", camera_id.0);
    println!("detected_face_count: {}", faces.len());

    if faces.is_empty() {
        print_presence_observation_and_decision(
            PresenceObservation::NoFaceDetected,
            threshold,
            None,
        );
        return Ok(());
    }

    if faces.len() > 1 {
        print_presence_observation_and_decision(
            PresenceObservation::UnknownFace {
                owner_match_score: 0.0,
            },
            threshold,
            None,
        );
        return Ok(());
    }

    let detected_face = &faces[0];
    let candidate = model_provider.extract(&frame, detected_face)?;
    let matcher = FaceTemplateMatcher::new(threshold);
    let recognition_model = model_provider.recognition_model().clone();
    let best_match =
        matcher.best_compatible_match(templates.as_slice(), &recognition_model, &candidate);
    let Some(best_match) = best_match else {
        println!("presence_owner_match_threshold: {threshold}");
        println!("presence_owner_match_passed: false");
        println!("presence_decision: TemplateModelMismatch");
        return Ok(());
    };

    let observation = match best_match.decision {
        FaceMatchDecision::MatchAccepted => PresenceObservation::OwnerPresent {
            owner_match_score: best_match.score,
        },
        FaceMatchDecision::MatchRejectedBelowThreshold => PresenceObservation::UnknownFace {
            owner_match_score: best_match.score,
        },
    };

    let decision =
        print_presence_observation_and_decision(observation, threshold, Some(&best_match));
    if decision.unknown_face_audit_capture_requested {
        save_presence_unknown_face_audit(
            args,
            &mut model_provider,
            &frame,
            detected_face,
            best_match.score,
            threshold,
        )?;
    }
    Ok(())
}

fn print_presence_observation_and_decision(
    observation: PresenceObservation,
    threshold: f32,
    best_match: Option<&face_engine::FaceTemplateMatch>,
) -> PresencePolicyDecision {
    let mut policy = PresencePolicy::new(PresencePolicyConfig {
        presence_owner_match_threshold: threshold,
        ..PresencePolicyConfig::default()
    });
    let decision = policy.record_observation(observation);
    let owner_match_score = match observation {
        PresenceObservation::OwnerPresent { owner_match_score }
        | PresenceObservation::UnknownFace { owner_match_score } => Some(owner_match_score),
        PresenceObservation::NoFaceDetected
        | PresenceObservation::PersonPresent { .. }
        | PresenceObservation::PersonAbsent
        | PresenceObservation::CameraUnavailable => None,
    };

    println!("presence_owner_match_threshold: {threshold}");
    println!(
        "presence_owner_match_score: {}",
        optional_score_text(owner_match_score)
    );
    println!(
        "presence_owner_match_passed: {}",
        matches!(observation, PresenceObservation::OwnerPresent { .. })
    );
    if let Some(best_match) = best_match {
        println!("best_template_ref: {}", best_match.template_ref.0);
        println!("best_template_pose_group: {:?}", best_match.pose_group);
    }
    print_presence_policy_decision(&decision);
    decision
}

fn save_presence_unknown_face_audit(
    args: &[String],
    model_provider: &mut OpenCvFaceModelProvider,
    frame: &video_provider::VideoFrame,
    detected_face: &face_engine::DetectedFace,
    match_score: f32,
    threshold: f32,
) -> Result<(), DiagnosticError> {
    let audit_dir = argument_value(args, "--audit-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PresenceAuditConfig::program_data_default().audit_dir);
    let screen_snapshot_disabled = args.iter().any(|arg| arg == "--disable-screen-snapshot");
    let config = PresenceAuditConfig {
        audit_dir: audit_dir.clone(),
        presence_audit_enabled: true,
        presence_audit_save_full_frame_thumbnail: false,
        presence_audit_save_screen_snapshot: !screen_snapshot_disabled,
        presence_audit_max_record_count: 50,
    };
    fs::create_dir_all(&audit_dir).map_err(|_| DiagnosticError::IoFailed)?;

    let event_id = format!("unknown-face-{}", current_time_unix_ms());
    let face_crop_path = audit_dir.join(format!("{event_id}-face.jpg"));
    model_provider.write_aligned_face(frame, detected_face, &face_crop_path)?;

    let store = PresenceAuditStore::new(config);
    let screen_snapshot_path = if store.screen_snapshot_enabled() {
        let path = audit_dir.join(format!("{event_id}-screen.bmp"));
        let helper = LocalProcessPresenceHelper;
        match helper.handle_request(PresenceHelperRequest::CaptureScreenSnapshot {
            event_id: event_id.clone(),
            output_path: path,
        }) {
            PresenceHelperResponse::ScreenSnapshotCaptured { image_path, .. } => {
                println!(
                    "presence_audit_screen_snapshot_path: {}",
                    image_path.display()
                );
                Some(image_path)
            }
            PresenceHelperResponse::ScreenSnapshotUnavailable { reason, .. } => {
                println!("presence_audit_screen_snapshot_failed: {reason:?}");
                None
            }
        }
    } else {
        None
    };

    let metadata_path = store
        .save_unknown_face_event(&UnknownFaceAuditEvent {
            event_id,
            captured_at_unix_ms: current_time_unix_ms(),
            decision: "unknown_face_detected".to_owned(),
            match_score,
            presence_owner_match_threshold: threshold,
            face_crop_path: Some(face_crop_path.clone()),
            optional_frame_thumbnail_path: None,
            optional_screen_snapshot_path: screen_snapshot_path,
            lock_requested: false,
        })
        .map_err(|_| DiagnosticError::IoFailed)?;

    println!(
        "presence_audit_face_crop_path: {}",
        face_crop_path.display()
    );
    println!("presence_audit_metadata_path: {}", metadata_path.display());
    Ok(())
}

fn run_presence_policy_simulate(args: &[String]) -> Result<(), DiagnosticError> {
    let events = argument_value(args, "--events").ok_or(DiagnosticError::InvalidArgument)?;
    let threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD);
    let mut policy = PresencePolicy::new(PresencePolicyConfig {
        presence_owner_match_threshold: threshold,
        ..PresencePolicyConfig::default()
    });

    for (event_index, event_name) in events.split(',').enumerate() {
        let observation = presence_observation_from_name(event_name.trim())?;
        let decision = policy.record_observation(observation);
        println!("event_index: {event_index}");
        println!("presence_observation: {}", event_name.trim());
        print_presence_policy_decision(&decision);
    }
    Ok(())
}

fn run_presence_monitor_simulate(args: &[String]) -> Result<(), DiagnosticError> {
    let events = argument_value(args, "--events").ok_or(DiagnosticError::InvalidArgument)?;
    let observations = events
        .split(',')
        .map(|event| presence_observation_from_name(event.trim()))
        .collect::<Result<Vec<_>, _>>()?;
    let threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD);
    let mut monitor = PresenceMonitor::new(
        PresenceMonitorConfig {
            presence_lock_enabled: true,
            max_monitor_iteration_count: optional_u32(args, "--max-iterations")?,
            sleep_between_checks: false,
            stop_requested: None,
        },
        PresencePolicyConfig {
            presence_owner_match_threshold: threshold,
            ..PresencePolicyConfig::default()
        },
        DiagnosticRecordingLocker,
        DiagnosticRecordingAuditSink,
        DiagnosticSequenceObservationSource { observations },
    );
    let summary = monitor
        .run()
        .map_err(|_| DiagnosticError::InvalidArgument)?;

    println!("presence_monitor_completed: true");
    println!(
        "presence_monitor_iteration_count: {}",
        summary.iteration_count
    );
    println!(
        "presence_monitor_unknown_face_audit_request_count: {}",
        summary.unknown_face_audit_request_count
    );
    println!(
        "presence_monitor_lock_request_count: {}",
        summary.lock_request_count
    );
    println!("presence_monitor_stop_reason: {:?}", summary.stop_reason);
    Ok(())
}

fn run_presence_monitor_camera_debug(args: &[String]) -> Result<(), DiagnosticError> {
    let template_path =
        argument_value(args, "--template").ok_or(DiagnosticError::InvalidArgument)?;
    let threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD);
    let max_iterations = optional_u32(args, "--iterations")?.unwrap_or(3);
    let templates = read_recognition_templates(template_path)?;
    let camera_id = CameraId(
        argument_value(args, "--camera-id")
            .unwrap_or("opencv-index:0")
            .to_owned(),
    );
    let mut model_config = OpenCvFaceModelConfig::new(
        model_path(args, "--yunet-model", DEFAULT_YUNET_MODEL_PATH),
        model_path(args, "--sface-model", DEFAULT_SFACE_MODEL_PATH),
    );
    model_config.recognizer.match_threshold = threshold;

    let source = CameraPresenceObservationSource::new(CameraPresenceObservationConfig {
        camera_id,
        camera_config: OpenCvCameraProviderConfig {
            max_camera_index: 8,
            requested_frame_width: optional_u32(args, "--frame-width")?,
            requested_frame_height: optional_u32(args, "--frame-height")?,
            preferred_backend: None,
        },
        model_config,
        templates,
        presence_owner_match_threshold: threshold,
        pending_unknown_face_crop_path: None,
    })
    .map_err(|_| DiagnosticError::InvalidArgument)?;
    let mut monitor = PresenceMonitor::new(
        PresenceMonitorConfig {
            presence_lock_enabled: true,
            max_monitor_iteration_count: Some(max_iterations),
            sleep_between_checks: false,
            stop_requested: None,
        },
        PresencePolicyConfig {
            presence_owner_match_threshold: threshold,
            ..PresencePolicyConfig::default()
        },
        DiagnosticRecordingLocker,
        DiagnosticRecordingAuditSink,
        source,
    );
    let summary = monitor
        .run()
        .map_err(|_| DiagnosticError::InvalidArgument)?;

    println!("presence_monitor_camera_debug_completed: true");
    println!(
        "presence_monitor_iteration_count: {}",
        summary.iteration_count
    );
    println!(
        "presence_monitor_unknown_face_audit_request_count: {}",
        summary.unknown_face_audit_request_count
    );
    println!(
        "presence_monitor_lock_request_count: {}",
        summary.lock_request_count
    );
    println!("presence_monitor_stop_reason: {:?}", summary.stop_reason);
    Ok(())
}

fn run_presence_person_benchmark(args: &[String]) -> Result<(), DiagnosticError> {
    let detector_label = argument_value(args, "--detector").unwrap_or("mobilenet-ssd");
    let confidence_threshold =
        optional_f32(args, "--confidence")?.unwrap_or(DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD);
    let requested_fps = optional_f32(args, "--fps")?.unwrap_or(2.0);
    if requested_fps <= 0.0 {
        return Err(DiagnosticError::InvalidArgument);
    }
    let duration_seconds = optional_u32(args, "--duration-seconds")?.unwrap_or(120);
    if duration_seconds == 0 {
        return Err(DiagnosticError::InvalidArgument);
    }

    let detector_config =
        person_detector_config_from_args(args, detector_label, confidence_threshold)?;
    let debug_output_dir = argument_value(args, "--output-dir").map(PathBuf::from);
    if let Some(output_dir) = &debug_output_dir {
        fs::create_dir_all(output_dir).map_err(|_| DiagnosticError::IoFailed)?;
        let metadata_path = output_dir.join("presence_person_benchmark_frames.jsonl");
        if metadata_path.exists() {
            fs::remove_file(metadata_path).map_err(|_| DiagnosticError::IoFailed)?;
        }
    }
    let mut detector = OpenCvDnnPersonDetector::new(detector_config);
    let detector_load_started = Instant::now();
    detector.load_model()?;
    let detector_load_ms = detector_load_started.elapsed().as_secs_f64() * 1_000.0;

    let mut camera_provider = build_camera_provider(args)?;
    let sources = camera_provider.list_sources()?;
    let camera_id = selected_camera_id(args, &sources)?;
    camera_provider.open(&camera_id)?;

    let frame_interval = Duration::from_secs_f32(1.0 / requested_fps);
    let benchmark_started = Instant::now();
    let benchmark_duration = Duration::from_secs(duration_seconds as u64);
    let mut frame_count = 0_u32;
    let mut person_frame_count = 0_u32;
    let mut no_person_frame_count = 0_u32;
    let mut camera_error_count = 0_u32;
    let mut inference_error_count = 0_u32;
    let mut max_person_count = 0_usize;
    let mut inference_latencies_ms = Vec::new();

    while benchmark_started.elapsed() < benchmark_duration {
        let frame_started = Instant::now();
        match camera_provider.read_frame() {
            Ok(frame) => {
                frame_count = frame_count.saturating_add(1);
                let inference_started = Instant::now();
                match detector.detect_persons(&frame) {
                    Ok(detections) => {
                        inference_latencies_ms
                            .push(inference_started.elapsed().as_secs_f64() * 1_000.0);
                        if let Some(output_dir) = &debug_output_dir {
                            save_person_benchmark_debug_frame(
                                output_dir,
                                frame_count,
                                &frame,
                                &detections,
                            )?;
                        }
                        max_person_count = max_person_count.max(detections.len());
                        if detections.is_empty() {
                            no_person_frame_count = no_person_frame_count.saturating_add(1);
                        } else {
                            person_frame_count = person_frame_count.saturating_add(1);
                        }
                    }
                    Err(_) => {
                        inference_error_count = inference_error_count.saturating_add(1);
                    }
                }
            }
            Err(_) => {
                camera_error_count = camera_error_count.saturating_add(1);
            }
        }

        let elapsed = frame_started.elapsed();
        if elapsed < frame_interval {
            thread::sleep(frame_interval - elapsed);
        }
    }

    camera_provider.close();
    detector.unload_model();

    let elapsed_seconds = benchmark_started.elapsed().as_secs_f64();
    let actual_fps = if elapsed_seconds > 0.0 {
        frame_count as f64 / elapsed_seconds
    } else {
        0.0
    };
    let person_frame_ratio = if frame_count > 0 {
        person_frame_count as f64 / frame_count as f64
    } else {
        0.0
    };
    let latency = LatencySummary::from_samples(&inference_latencies_ms);

    println!("presence_person_benchmark_completed: true");
    println!("detector: {detector_label}");
    println!("camera_id: {}", camera_id.0);
    println!("requested_fps: {requested_fps}");
    println!("actual_fps: {actual_fps}");
    println!("duration_seconds: {elapsed_seconds}");
    println!("detector_load_ms: {detector_load_ms}");
    println!("captured_frame_count: {frame_count}");
    println!("person_frame_count: {person_frame_count}");
    println!("no_person_frame_count: {no_person_frame_count}");
    println!("person_frame_ratio: {person_frame_ratio}");
    println!("max_person_count: {max_person_count}");
    println!("camera_error_count: {camera_error_count}");
    println!("inference_error_count: {inference_error_count}");
    println!("inference_latency_count: {}", latency.count);
    println!("inference_latency_avg_ms: {}", latency.avg_ms);
    println!("inference_latency_p50_ms: {}", latency.p50_ms);
    println!("inference_latency_p90_ms: {}", latency.p90_ms);
    println!("inference_latency_max_ms: {}", latency.max_ms);
    if let Some(output_dir) = &debug_output_dir {
        println!("debug_output_dir: {}", output_dir.display());
    }
    Ok(())
}

fn run_presence_person_lock_debug(args: &[String]) -> Result<(), DiagnosticError> {
    let detector_label = argument_value(args, "--detector").unwrap_or("yolov8-onnx");
    let confidence_threshold =
        optional_f32(args, "--confidence")?.unwrap_or(DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD);
    let max_iterations = optional_u32(args, "--iterations")?.unwrap_or(8);
    let detector_config =
        person_detector_runtime_config_from_args(args, detector_label, confidence_threshold)?;
    update_interface_runtime_state(
        InterfaceRuntimeState::DesktopUnlocked,
        InterfaceRuntimeStateSource::DesktopControlReload,
    );
    let camera_id = CameraId(
        argument_value(args, "--camera-id")
            .unwrap_or("opencv-index:0")
            .to_owned(),
    );
    let source =
        PersonCameraPresenceObservationSource::new(PersonCameraPresenceObservationConfig {
            camera_id,
            camera_config: OpenCvCameraProviderConfig {
                max_camera_index: 8,
                requested_frame_width: optional_u32(args, "--frame-width")?,
                requested_frame_height: optional_u32(args, "--frame-height")?,
                preferred_backend: None,
            },
            detector_config,
            debug_output_dir: argument_value(args, "--output-dir").map(PathBuf::from),
        })
        .map_err(|_| DiagnosticError::InvalidArgument)?;
    let real_lock_requested = args.iter().any(|arg| arg == "--real-lock");
    let mut monitor = PresenceMonitor::new(
        PresenceMonitorConfig {
            presence_lock_enabled: true,
            max_monitor_iteration_count: Some(max_iterations),
            sleep_between_checks: true,
            stop_requested: None,
        },
        PresencePolicyConfig {
            presence_person_absent_required_frames: optional_u32(args, "--absent-frames")?
                .unwrap_or(6),
            ..PresencePolicyConfig::default()
        },
        DiagnosticMaybeRealLocker {
            real_lock_requested,
        },
        DiagnosticRecordingAuditSink,
        source,
    );
    let summary = monitor
        .run()
        .map_err(|_| DiagnosticError::InvalidArgument)?;

    println!("presence_person_lock_debug_completed: true");
    println!("real_lock_requested: {real_lock_requested}");
    println!(
        "presence_monitor_iteration_count: {}",
        summary.iteration_count
    );
    println!(
        "presence_monitor_lock_request_count: {}",
        summary.lock_request_count
    );
    println!("presence_monitor_stop_reason: {:?}", summary.stop_reason);
    Ok(())
}

fn save_person_benchmark_debug_frame(
    output_dir: &Path,
    frame_index: u32,
    frame: &VideoFrame,
    detections: &[PersonDetection],
) -> Result<(), DiagnosticError> {
    let mut mat = video_frame_to_mat_for_debug(frame)?;
    for detection in detections {
        let rect = Rect::new(
            detection.bbox.x as i32,
            detection.bbox.y as i32,
            detection.bbox.width as i32,
            detection.bbox.height as i32,
        );
        imgproc::rectangle(
            &mut mat,
            rect,
            Scalar::new(0.0, 255.0, 0.0, 0.0),
            2,
            imgproc::LINE_8,
            0,
        )
        .map_err(|_| DiagnosticError::IoFailed)?;
        imgproc::put_text(
            &mut mat,
            &format!("{:.2}", detection.confidence),
            opencv::core::Point::new(detection.bbox.x as i32, detection.bbox.y as i32 - 6),
            imgproc::FONT_HERSHEY_SIMPLEX,
            0.6,
            Scalar::new(0.0, 255.0, 0.0, 0.0),
            1,
            imgproc::LINE_8,
            false,
        )
        .map_err(|_| DiagnosticError::IoFailed)?;
    }

    let outcome = if detections.is_empty() { "miss" } else { "hit" };
    let image_path = output_dir.join(format!("frame-{frame_index:06}-{outcome}.jpg"));
    let image_path_text = image_path.display().to_string();
    imgcodecs::imwrite(&image_path_text, &mat, &Vector::<i32>::new())
        .map_err(|_| DiagnosticError::IoFailed)?;

    let metadata = serde_json::json!({
        "frame_index": frame_index,
        "outcome": outcome,
        "image_path": image_path_text,
        "frame_width": frame.width,
        "frame_height": frame.height,
        "detections": detections.iter().map(|detection| serde_json::json!({
            "confidence": detection.confidence,
            "bbox": {
                "x": detection.bbox.x,
                "y": detection.bbox.y,
                "width": detection.bbox.width,
                "height": detection.bbox.height,
            },
            "normalized_bbox": {
                "x_min": detection.normalized_bbox.x_min,
                "y_min": detection.normalized_bbox.y_min,
                "x_max": detection.normalized_bbox.x_max,
                "y_max": detection.normalized_bbox.y_max,
            }
        })).collect::<Vec<_>>(),
    });
    let mut metadata_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(output_dir.join("presence_person_benchmark_frames.jsonl"))
        .map_err(|_| DiagnosticError::IoFailed)?;
    writeln!(metadata_file, "{metadata}").map_err(|_| DiagnosticError::IoFailed)?;
    Ok(())
}

fn video_frame_to_mat_for_debug(frame: &VideoFrame) -> Result<Mat, DiagnosticError> {
    let channels = match frame.format {
        PixelFormat::Bgr8 | PixelFormat::Rgb8 => 3,
        PixelFormat::Gray8 => 1,
    };
    let mat = Mat::from_slice(&frame.data).map_err(|_| DiagnosticError::IoFailed)?;
    let mat = mat
        .reshape(channels, frame.height as i32)
        .map_err(|_| DiagnosticError::IoFailed)?;
    let mut mat = mat.try_clone().map_err(|_| DiagnosticError::IoFailed)?;

    if frame.format == PixelFormat::Rgb8 {
        let mut bgr = Mat::default();
        imgproc::cvt_color(
            &mat,
            &mut bgr,
            imgproc::COLOR_RGB2BGR,
            0,
            AlgorithmHint::ALGO_HINT_DEFAULT,
        )
        .map_err(|_| DiagnosticError::IoFailed)?;
        mat = bgr;
    }

    Ok(mat)
}

fn person_detector_config_from_args(
    args: &[String],
    detector_label: &str,
    confidence_threshold: f32,
) -> Result<OpenCvDnnPersonDetectorConfig, DiagnosticError> {
    let mut config = match detector_label {
        "mobilenet-ssd" => OpenCvDnnPersonDetectorConfig::mobilenet_ssd(
            model_path(args, "--model", "models/MobileNetSSD_deploy.caffemodel"),
            model_path(args, "--config", "models/MobileNetSSD_deploy.prototxt"),
        ),
        "ssdlite-onnx" => OpenCvDnnPersonDetectorConfig::ssdlite_onnx(model_path(
            args,
            "--model",
            "models/ssdlite_mobilenet_v3.onnx",
        )),
        "yolov8-onnx" => OpenCvDnnPersonDetectorConfig::yolov8_onnx(model_path(
            args,
            "--model",
            "models/yolov8n.onnx",
        )),
        _ => return Err(DiagnosticError::InvalidArgument),
    };
    config.confidence_threshold = confidence_threshold;
    if let Some(input_width) = optional_i32(args, "--input-width")? {
        config.input_width = input_width;
    }
    if let Some(input_height) = optional_i32(args, "--input-height")? {
        config.input_height = input_height;
    }
    Ok(config)
}

fn person_detector_runtime_config_from_args(
    args: &[String],
    detector_label: &str,
    confidence_threshold: f32,
) -> Result<PersonDetectorConfig, DiagnosticError> {
    if detector_label == "ort-yolov8-onnx" {
        let mut config =
            OrtYoloV8PersonDetectorConfig::new(model_path(args, "--model", "models/yolov8n.onnx"));
        config.confidence_threshold = confidence_threshold;
        if let Some(input_width) = optional_i32(args, "--input-width")? {
            config.input_width = input_width.max(1) as usize;
        }
        if let Some(input_height) = optional_i32(args, "--input-height")? {
            config.input_height = input_height.max(1) as usize;
        }
        return Ok(PersonDetectorConfig::OrtYoloV8(config));
    }

    person_detector_config_from_args(args, detector_label, confidence_threshold)
        .map(PersonDetectorConfig::OpenCvDnn)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LatencySummary {
    count: usize,
    avg_ms: f64,
    p50_ms: f64,
    p90_ms: f64,
    max_ms: f64,
}

impl LatencySummary {
    fn from_samples(samples: &[f64]) -> Self {
        if samples.is_empty() {
            return Self {
                count: 0,
                avg_ms: 0.0,
                p50_ms: 0.0,
                p90_ms: 0.0,
                max_ms: 0.0,
            };
        }

        let mut sorted = samples.to_vec();
        sorted.sort_by(|left, right| left.total_cmp(right));
        let sum: f64 = sorted.iter().sum();
        Self {
            count: sorted.len(),
            avg_ms: sum / sorted.len() as f64,
            p50_ms: percentile_sorted_f64(&sorted, 0.50),
            p90_ms: percentile_sorted_f64(&sorted, 0.90),
            max_ms: sorted[sorted.len() - 1],
        }
    }
}

struct DiagnosticSequenceObservationSource {
    observations: Vec<PresenceObservation>,
}

impl PresenceObservationSource for DiagnosticSequenceObservationSource {
    fn next_observation(&mut self) -> Result<Option<PresenceObservation>, PresenceMonitorError> {
        if self.observations.is_empty() {
            return Ok(None);
        }
        Ok(Some(self.observations.remove(0)))
    }
}

#[derive(Default)]
struct DiagnosticRecordingAuditSink;

impl UnknownFaceAuditSink for DiagnosticRecordingAuditSink {
    fn capture_unknown_face_audit(
        &mut self,
        _decision: &PresencePolicyDecision,
    ) -> Result<(), PresenceMonitorError> {
        Ok(())
    }
}

#[derive(Default)]
struct DiagnosticRecordingLocker;

impl SessionLocker for DiagnosticRecordingLocker {
    fn request_lock_workstation(&self) -> Result<(), SessionLockError> {
        Ok(())
    }
}

struct DiagnosticMaybeRealLocker {
    real_lock_requested: bool,
}

impl SessionLocker for DiagnosticMaybeRealLocker {
    fn request_lock_workstation(&self) -> Result<(), SessionLockError> {
        if self.real_lock_requested {
            return WindowsSessionLocker::current_workstation().request_lock_workstation();
        }
        Ok(())
    }
}

fn run_screen_snapshot_debug(args: &[String]) -> Result<(), DiagnosticError> {
    let output_path =
        PathBuf::from(argument_value(args, "--output").ok_or(DiagnosticError::InvalidArgument)?);
    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|_| DiagnosticError::IoFailed)?;
    }
    let helper = LocalProcessPresenceHelper;
    match helper.handle_request(PresenceHelperRequest::CaptureScreenSnapshot {
        event_id: "screen-snapshot-debug".to_owned(),
        output_path,
    }) {
        PresenceHelperResponse::ScreenSnapshotCaptured {
            image_path,
            width,
            height,
            ..
        } => {
            println!("screen_snapshot_completed: true");
            println!("screen_snapshot_path: {}", image_path.display());
            println!("screen_snapshot_width: {width}");
            println!("screen_snapshot_height: {height}");
            Ok(())
        }
        PresenceHelperResponse::ScreenSnapshotUnavailable { reason, .. } => {
            println!("screen_snapshot_completed: false");
            println!("screen_snapshot_failure_reason: {reason:?}");
            Err(DiagnosticError::IoFailed)
        }
    }
}

fn presence_observation_from_name(name: &str) -> Result<PresenceObservation, DiagnosticError> {
    match name {
        "owner" | "owner-present" => Ok(PresenceObservation::OwnerPresent {
            owner_match_score: 0.70,
        }),
        "no-face" => Ok(PresenceObservation::NoFaceDetected),
        "unknown" | "unknown-face" => Ok(PresenceObservation::UnknownFace {
            owner_match_score: 0.20,
        }),
        "person" | "person-present" => Ok(PresenceObservation::PersonPresent {
            confidence: 0.80,
            bbox_center_x_ratio: 0.50,
            bbox_area_ratio: 0.40,
        }),
        "person-left" | "person-left-boundary" => Ok(PresenceObservation::PersonPresent {
            confidence: 0.80,
            bbox_center_x_ratio: 0.08,
            bbox_area_ratio: 0.20,
        }),
        "person-absent" => Ok(PresenceObservation::PersonAbsent),
        "camera-unavailable" => Ok(PresenceObservation::CameraUnavailable),
        _ => Err(DiagnosticError::InvalidArgument),
    }
}

fn print_presence_policy_decision(decision: &PresencePolicyDecision) {
    println!("presence_monitor_state: {:?}", decision.monitor_state);
    println!(
        "presence_next_check_interval_ms: {}",
        decision.next_check_interval_ms
    );
    println!(
        "presence_no_face_consecutive_count: {}",
        decision.no_face_consecutive_count
    );
    println!(
        "presence_unknown_face_consecutive_count: {}",
        decision.unknown_face_consecutive_count
    );
    println!(
        "presence_unknown_face_audit_capture_requested: {}",
        decision.unknown_face_audit_capture_requested
    );
    println!("presence_lock_requested: {}", decision.lock_requested);
    println!("presence_lock_reason: {:?}", decision.lock_reason);
}

fn optional_score_text(score: Option<f32>) -> String {
    score
        .map(|score| score.to_string())
        .unwrap_or_else(|| "None".to_owned())
}

fn run_face_auth_debug(args: &[String]) -> Result<(), DiagnosticError> {
    let template_path =
        argument_value(args, "--template").ok_or(DiagnosticError::InvalidArgument)?;
    let threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PROJECT_FACE_MATCH_THRESHOLD);
    let max_frames = optional_u32(args, "--frames")?.unwrap_or(30);
    let templates = read_recognition_templates(template_path)?;

    let mut provider = build_camera_provider(args)?;
    let sources = provider.list_sources()?;
    let camera_id = selected_camera_id(args, &sources)?;
    provider.open(&camera_id)?;

    let model_provider = build_loaded_model_provider(args)?;
    let matcher = FaceTemplateMatcher::new(threshold);
    let policy = AttemptPolicy::new(AttemptPolicyConfig {
        required_consecutive_match_count: 1,
        ..AttemptPolicyConfig::default()
    });
    let mut authenticator = FaceAuthenticator::new(model_provider, matcher, policy);

    for frame_index in 0..max_frames {
        let frame = provider.read_frame()?;
        match authenticator.authenticate_frame(&frame, &templates, current_time_unix_ms()) {
            Ok(outcome) => {
                println!(
                    "frame_index={frame_index} auth_match_passed=true best_score={} best_template_id={} best_pose_group={:?}",
                    outcome.match_score,
                    outcome.matched_template.template_ref.0,
                    outcome.matched_pose_group
                );
            }
            Err(reason) => {
                println!("frame_index={frame_index} auth_match_passed=false reason={reason:?}");
            }
        }
    }

    provider.close();
    Ok(())
}

fn run_verify_face(args: &[String]) -> Result<(), DiagnosticError> {
    let image_path = argument_value(args, "--image").ok_or(DiagnosticError::InvalidArgument)?;
    let template_path =
        argument_value(args, "--template").ok_or(DiagnosticError::InvalidArgument)?;
    let threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PROJECT_FACE_MATCH_THRESHOLD);
    let templates = read_recognition_templates(template_path)?;
    let model_provider = build_loaded_model_provider(args)?;
    let matcher = FaceTemplateMatcher::new(threshold);
    let policy = AttemptPolicy::new(AttemptPolicyConfig {
        required_consecutive_match_count: 1,
        ..AttemptPolicyConfig::default()
    });
    let mut authenticator = FaceAuthenticator::new(model_provider, matcher, policy);
    let frame = OpenCvFaceModelProvider::read_image_frame(image_path)?;
    let outcome = authenticator
        .authenticate_frame(&frame, &templates, current_time_unix_ms())
        .map_err(DiagnosticError::AuthRejected)?;

    println!("auth_match_passed: true");
    println!("matched_user_id: {}", outcome.matched_user_id.0);
    println!("match_score: {}", outcome.match_score);
    println!(
        "matched_template_ref: {}",
        outcome.matched_template.template_ref.0
    );
    println!("matched_pose_group: {:?}", outcome.matched_pose_group);
    Ok(())
}

fn run_camera_auth(args: &[String]) -> Result<(), DiagnosticError> {
    let template_path =
        argument_value(args, "--template").ok_or(DiagnosticError::InvalidArgument)?;
    let threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PROJECT_FACE_MATCH_THRESHOLD);
    let required_consecutive_match_count =
        optional_u32(args, "--required-consecutive")?.unwrap_or(2);
    let max_frames = optional_u32(args, "--max-frames")?.unwrap_or(30);
    let templates = read_recognition_templates(template_path)?;

    let mut provider = build_camera_provider(args)?;
    let sources = provider.list_sources()?;
    let camera_id = selected_camera_id(args, &sources)?;
    provider.open(&camera_id)?;

    let model_provider = build_loaded_model_provider(args)?;
    let matcher = FaceTemplateMatcher::new(threshold);
    let policy = AttemptPolicy::new(AttemptPolicyConfig {
        required_consecutive_match_count,
        ..AttemptPolicyConfig::default()
    });
    let mut authenticator = FaceAuthenticator::new(model_provider, matcher, policy);

    let mut last_rejection = None;
    for _ in 0..max_frames {
        let frame = provider.read_frame()?;
        match authenticator.authenticate_frame(&frame, &templates, current_time_unix_ms()) {
            Ok(outcome) => {
                provider.close();
                println!("camera_auth_passed: true");
                println!("matched_user_id: {}", outcome.matched_user_id.0);
                println!("match_score: {}", outcome.match_score);
                println!("matched_pose_group: {:?}", outcome.matched_pose_group);
                return Ok(());
            }
            Err(reason) => last_rejection = Some(reason),
        }
    }

    provider.close();
    Err(DiagnosticError::AuthRejected(
        last_rejection.unwrap_or(AuthFailureReason::Timeout),
    ))
}

fn run_calibrate_threshold(args: &[String]) -> Result<(), DiagnosticError> {
    let template_path =
        argument_value(args, "--template").ok_or(DiagnosticError::InvalidArgument)?;
    let target_sample_count = optional_u32(args, "--samples")?.unwrap_or(20);
    let max_frames = optional_u32(args, "--max-frames")?.unwrap_or(120);
    let template = read_template(template_path)?;
    let matcher = FaceTemplateMatcher::new(f32::NEG_INFINITY);

    let mut provider = build_camera_provider(args)?;
    let sources = provider.list_sources()?;
    let camera_id = selected_camera_id(args, &sources)?;
    provider.open(&camera_id)?;

    let mut model_provider = build_loaded_model_provider(args)?;
    let mut scores = Vec::new();
    let mut no_face_count = 0_u32;
    let mut multiple_face_count = 0_u32;
    let mut internal_error_count = 0_u32;

    for _ in 0..max_frames {
        if scores.len() >= target_sample_count as usize {
            break;
        }

        let frame = provider.read_frame()?;
        let faces = match model_provider.detect(&frame) {
            Ok(faces) => faces,
            Err(FaceEngineError::NoFaceDetected) => {
                no_face_count = no_face_count.saturating_add(1);
                continue;
            }
            Err(_) => {
                internal_error_count = internal_error_count.saturating_add(1);
                continue;
            }
        };
        if faces.is_empty() {
            no_face_count = no_face_count.saturating_add(1);
            continue;
        }
        if faces.len() > 1 {
            multiple_face_count = multiple_face_count.saturating_add(1);
            continue;
        }

        match model_provider.extract(&frame, &faces[0]) {
            Ok(candidate) => {
                let matched = matcher.compare_embeddings(&template.embedding, &candidate);
                scores.push(matched.score);
            }
            Err(_) => internal_error_count = internal_error_count.saturating_add(1),
        }
    }

    provider.close();
    model_provider.unload_models();

    println!("calibration_template_ref: {}", template.template_ref.0);
    println!("calibration_user_id: {}", template.user_id);
    println!("requested_sample_count: {target_sample_count}");
    println!("max_frames: {max_frames}");
    println!("collected_score_count: {}", scores.len());
    println!("no_face_frame_count: {no_face_count}");
    println!("multiple_face_frame_count: {multiple_face_count}");
    println!("internal_error_frame_count: {internal_error_count}");
    print_score_summary(&scores)?;
    Ok(())
}

fn print_score_summary(scores: &[f32]) -> Result<(), DiagnosticError> {
    if scores.is_empty() {
        return Err(DiagnosticError::AuthRejected(
            AuthFailureReason::NoFaceDetected,
        ));
    }

    let mut sorted_scores = scores.to_vec();
    sorted_scores.sort_by(|left, right| left.total_cmp(right));
    let sum: f32 = sorted_scores.iter().sum();
    let average = sum / sorted_scores.len() as f32;
    let min = sorted_scores[0];
    let max = sorted_scores[sorted_scores.len() - 1];
    let p10 = percentile_sorted(&sorted_scores, 0.10);
    let p50 = percentile_sorted(&sorted_scores, 0.50);
    let p90 = percentile_sorted(&sorted_scores, 0.90);

    println!("score_min: {min}");
    println!("score_avg: {average}");
    println!("score_max: {max}");
    println!("score_p10: {p10}");
    println!("score_p50: {p50}");
    println!("score_p90: {p90}");
    println!(
        "threshold_0_55_pass_count: {}",
        scores.iter().filter(|score| **score >= 0.55).count()
    );
    println!(
        "threshold_0_60_pass_count: {}",
        scores.iter().filter(|score| **score >= 0.60).count()
    );
    println!(
        "threshold_0_75_pass_count: {}",
        scores.iter().filter(|score| **score >= 0.75).count()
    );
    println!(
        "threshold_0_85_pass_count: {}",
        scores.iter().filter(|score| **score >= 0.85).count()
    );
    Ok(())
}

fn percentile_sorted(sorted_scores: &[f32], percentile: f32) -> f32 {
    if sorted_scores.len() == 1 {
        return sorted_scores[0];
    }

    let clamped_percentile = percentile.clamp(0.0, 1.0);
    let last_index = sorted_scores.len() - 1;
    let index = (last_index as f32 * clamped_percentile).round() as usize;
    sorted_scores[index.min(last_index)]
}

fn percentile_sorted_f64(sorted_scores: &[f64], percentile: f64) -> f64 {
    if sorted_scores.len() == 1 {
        return sorted_scores[0];
    }

    let clamped_percentile = percentile.clamp(0.0, 1.0);
    let last_index = sorted_scores.len() - 1;
    let index = (last_index as f64 * clamped_percentile).round() as usize;
    sorted_scores[index.min(last_index)]
}

fn build_loaded_model_provider(
    args: &[String],
) -> Result<OpenCvFaceModelProvider, DiagnosticError> {
    let yunet_model_path = model_path(args, "--yunet-model", DEFAULT_YUNET_MODEL_PATH);
    let sface_model_path = model_path(args, "--sface-model", DEFAULT_SFACE_MODEL_PATH);
    let threshold =
        optional_f32(args, "--threshold")?.unwrap_or(DEFAULT_PROJECT_FACE_MATCH_THRESHOLD);

    let mut config = OpenCvFaceModelConfig::new(yunet_model_path, sface_model_path);
    config.recognizer.match_threshold = threshold;
    let mut model_provider = OpenCvFaceModelProvider::new(config);
    model_provider.load_models()?;
    Ok(model_provider)
}

fn build_loaded_model_provider_with_threshold(
    args: &[String],
    threshold: f32,
) -> Result<OpenCvFaceModelProvider, DiagnosticError> {
    let yunet_model_path = model_path(args, "--yunet-model", DEFAULT_YUNET_MODEL_PATH);
    let sface_model_path = model_path(args, "--sface-model", DEFAULT_SFACE_MODEL_PATH);

    let mut config = OpenCvFaceModelConfig::new(yunet_model_path, sface_model_path);
    config.recognizer.match_threshold = threshold;
    let mut model_provider = OpenCvFaceModelProvider::new(config);
    model_provider.load_models()?;
    Ok(model_provider)
}

fn selected_camera_id(
    args: &[String],
    sources: &[video_provider::CameraInfo],
) -> Result<CameraId, DiagnosticError> {
    if let Some(camera_id) = argument_value(args, "--camera-id") {
        return Ok(CameraId(camera_id.to_owned()));
    }

    sources
        .first()
        .map(|source| source.id.clone())
        .ok_or(DiagnosticError::Video(VideoError::CameraNotFound))
}

fn selected_camera_id_without_forced_scan(
    args: &[String],
    provider: &dyn video_provider::VideoFrameProvider,
) -> Result<CameraId, DiagnosticError> {
    if let Some(camera_id) = argument_value(args, "--camera-id") {
        return Ok(CameraId(camera_id.to_owned()));
    }

    let sources = provider.list_sources()?;
    selected_camera_id(args, &sources)
}

fn build_camera_provider(args: &[String]) -> Result<OpenCvCameraProvider, DiagnosticError> {
    Ok(OpenCvCameraProvider::new(camera_provider_config(args)?))
}

fn camera_provider_config(args: &[String]) -> Result<OpenCvCameraProviderConfig, DiagnosticError> {
    let mut config = OpenCvCameraProviderConfig {
        max_camera_index: 8,
        requested_frame_width: optional_u32(args, "--frame-width")?,
        requested_frame_height: optional_u32(args, "--frame-height")?,
        preferred_backend: None,
    };
    if let Some(camera_id) = argument_value(args, "--camera-id") {
        apply_profile_to_config(&CameraId(camera_id.to_owned()), &mut config);
    }
    Ok(config)
}

fn camera_provider_config_for_camera(
    args: &[String],
    camera_id: &CameraId,
) -> Result<OpenCvCameraProviderConfig, DiagnosticError> {
    let mut config = camera_provider_config(args)?;
    apply_profile_to_config(camera_id, &mut config);
    Ok(config)
}

fn build_camera_provider_for_camera(
    args: &[String],
    camera_id: &CameraId,
) -> Result<OpenCvCameraProvider, DiagnosticError> {
    Ok(OpenCvCameraProvider::new(
        camera_provider_config_for_camera(args, camera_id)?,
    ))
}

fn print_camera_sources(sources: &[video_provider::CameraInfo]) {
    println!("camera_count: {}", sources.len());
    for source in sources {
        println!("camera: {} {}", source.id.0, source.display_name);
    }
}

fn read_template(template_path: &str) -> Result<FaceTemplate, DiagnosticError> {
    let bytes = fs::read(template_path).map_err(|_| DiagnosticError::IoFailed)?;
    FaceTemplate::from_json_bytes(&bytes).map_err(DiagnosticError::TemplateCodec)
}

fn read_template_set(template_path: &str) -> Result<FaceTemplateSet, DiagnosticError> {
    let bytes = fs::read(template_path).map_err(|_| DiagnosticError::IoFailed)?;
    FaceTemplateSet::from_json_bytes(&bytes).map_err(DiagnosticError::TemplateCodec)
}

fn read_recognition_templates(
    template_path: &str,
) -> Result<RecognitionTemplates, DiagnosticError> {
    let bytes = fs::read(template_path).map_err(|_| DiagnosticError::IoFailed)?;
    if let Ok(template_set) = FaceTemplateSet::from_json_bytes(&bytes) {
        return Ok(RecognitionTemplates::new(template_set.selected_templates()));
    }

    let template = FaceTemplate::from_json_bytes(&bytes).map_err(DiagnosticError::TemplateCodec)?;
    Ok(RecognitionTemplates::new(vec![template]))
}

fn model_path(args: &[String], argument_name: &str, default_path: &str) -> PathBuf {
    argument_value(args, argument_name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_path))
}

fn optional_f32(args: &[String], argument_name: &str) -> Result<Option<f32>, DiagnosticError> {
    argument_value(args, argument_name)
        .map(|value| value.parse::<f32>())
        .transpose()
        .map_err(|_| DiagnosticError::InvalidArgument)
}

fn optional_u32(args: &[String], argument_name: &str) -> Result<Option<u32>, DiagnosticError> {
    argument_value(args, argument_name)
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|_| DiagnosticError::InvalidArgument)
}

fn optional_i32(args: &[String], argument_name: &str) -> Result<Option<i32>, DiagnosticError> {
    argument_value(args, argument_name)
        .map(|value| value.parse::<i32>())
        .transpose()
        .map_err(|_| DiagnosticError::InvalidArgument)
}

fn optional_f64(args: &[String], argument_name: &str) -> Result<Option<f64>, DiagnosticError> {
    argument_value(args, argument_name)
        .map(|value| value.parse::<f64>())
        .transpose()
        .map_err(|_| DiagnosticError::InvalidArgument)
}

fn threshold_preview_method(args: &[String]) -> Result<ThresholdPreviewMethod, DiagnosticError> {
    match argument_value(args, "--method").unwrap_or("binary-inv-mask") {
        "binary-inv-mask" | "screen-contour" => Ok(ThresholdPreviewMethod::BinaryInvertedMask),
        "adaptive-gaussian" | "gaussian" => Ok(ThresholdPreviewMethod::AdaptiveGaussian),
        "adaptive-mean" | "mean" => Ok(ThresholdPreviewMethod::AdaptiveMean),
        "otsu" => Ok(ThresholdPreviewMethod::Otsu),
        _ => Err(DiagnosticError::InvalidArgument),
    }
}

fn wake_auth_source(args: &[String]) -> Result<AuthSource, DiagnosticError> {
    match argument_value(args, "--source").unwrap_or("manual-test") {
        "manual-test" => Ok(AuthSource::ManualTest),
        "local-camera" => Ok(AuthSource::LocalCamera),
        "vehicle-camera" => Ok(AuthSource::VehicleCamera),
        _ => Err(DiagnosticError::InvalidArgument),
    }
}

fn current_time_unix_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    millis.min(i64::MAX as u128) as i64
}

fn default_template_ref() -> String {
    format!("face-template-{}", current_time_unix_ms())
}

fn send_request(pipe_name: &str, request: ServiceRequest) -> Result<ServiceEvent, ProtocolError> {
    let mut client = NamedPipeClient::new(pipe_name);
    client.connect()?;
    let event = client.request(request)?;
    client.disconnect();
    Ok(event)
}

fn print_wake_auth_event(event: &ServiceEvent) {
    println!("{SERVICE_NAME} wake-auth: {event:?}");
    if let ServiceEvent::AuthSucceeded { grant } = event {
        print_grant_summary(grant);
    }
}

fn print_grant_summary(grant: &AuthGrant) {
    println!("grant_id: {}", grant.grant_id.0);
    println!("nonce: {}", grant.nonce.0);
    println!("session_id: {}", grant.session_id.0);
}

fn print_credential_material_event(event: &ServiceEvent) {
    match event {
        ServiceEvent::CredentialMaterialReady {
            grant_id,
            protected_credential_material,
        } => {
            println!("{SERVICE_NAME} fetch-credential-material: CredentialMaterialReady");
            println!("grant_id: {}", grant_id.0);
            println!("user_id: {}", protected_credential_material.user_id.0);
            println!("domain: {}", protected_credential_material.domain);
            println!("username: {}", protected_credential_material.username);
            println!("protection: {:?}", protected_credential_material.protection);
            println!(
                "protected_password_bytes: {}",
                protected_credential_material.protected_password.len()
            );
        }
        _ => println!("{SERVICE_NAME} fetch-credential-material: {event:?}"),
    }
}

fn print_usage() {
    println!("{SERVICE_NAME} diagnostics");
    println!("Usage: diagnostics_cli health-check [--pipe-name <pipe>]");
    println!(
        "Usage: diagnostics_cli wake-auth [--session-id <id>] [--source manual-test|local-camera] [--pipe-name <pipe>]"
    );
    println!(
        "Usage: diagnostics_cli fetch-credential --grant-id <id> --nonce <nonce> [--session-id <id>] [--pipe-name <pipe>]"
    );
    println!(
        "Usage: diagnostics_cli fetch-credential-material --grant-id <id> --nonce <nonce> [--session-id <id>] [--pipe-name <pipe>]"
    );
    println!(
        "Usage: diagnostics_cli enroll-windows-credential --username <name> [--user-id dev-user] [--account-type local|microsoft|domain] [--store-dir <path>]"
    );
    println!("Usage: diagnostics_cli service-camera-auth [--session-id <id>] [--pipe-name <pipe>]");
    println!("Usage: diagnostics_cli list-cameras");
    println!(
        "Usage: diagnostics_cli test-camera [--camera-id opencv-index:0] [--frame-width 640 --frame-height 480]"
    );
    println!(
        "Usage: diagnostics_cli camera-open-benchmark [--camera-id opencv-index:0] [--backend msmf|dshow|any] [--frame-width 640 --frame-height 480]"
    );
    println!(
        "Usage: diagnostics_cli test-face --image <path> [--yunet-model <onnx>] [--sface-model <onnx>]"
    );
    println!(
        "Usage: diagnostics_cli enroll-face --image <path> --template-out <path> [--user-id <id>]"
    );
    println!(
        "Usage: diagnostics_cli enroll-camera --template-out <path> [--camera-id opencv-index:0] [--frame-width 640 --frame-height 480] [--user-id <id>]"
    );
    println!(
        "Usage: diagnostics_cli guided-enroll --output-dir <dir> [--camera-id opencv-index:0] [--accepted-frames-per-step 6] [--max-wait-frames-per-step 180] [--max-frames-per-step 180] [--pose-ready-consecutive 3] [--pose-ready-min-fit 0.25] [--allow-partial-enrollment] [--save-debug-images] [--pose-provider landmark|mediapipe] [--mediapipe-bridge <dll>] [--mediapipe-model <task>] [--user-id <id>]"
    );
    println!("Usage: diagnostics_cli enrollment-report --template <selected_templates.json>");
    println!(
        "Usage: diagnostics_cli face-debug-snapshot --output-dir <dir> [--scenario front|yaw-left-30|yaw-right-30|pitch-up-15|pitch-down-15] [--start-delay-seconds 3] [--camera-id opencv-index:0] [--frames 30] [--frame-width 640 --frame-height 480] [--save-aligned-face]"
    );
    println!(
        "Usage: diagnostics_cli face-calibrate --template <path> --output-dir <dir> [--scenario front|yaw-left-30|yaw-right-30|pitch-up-15|pitch-down-15] [--start-delay-seconds 3] [--camera-id opencv-index:0] [--frames 100] [--threshold-min 0.40 --threshold-max 0.80 --threshold-step 0.05] [--required-consecutive 3]"
    );
    println!(
        "Usage: diagnostics_cli liveness-screen-debug --output-dir <dir> [--camera-id opencv-index:0] [--frames 60] [--frame-width 640 --frame-height 480] [--save-debug-images] [--save-minifasnet-crops] [--minifasnet-model models/minifasnet_v2.onnx] [--minifasnet-diagnostic-only] [--disable-minifasnet] [--enable-screen-geometry-diagnostics]"
    );
    println!(
        "Usage: diagnostics_cli threshold-preview [--camera-id opencv-index:0] [--frame-width 640 --frame-height 480] [--method binary-inv-mask|adaptive-gaussian|adaptive-mean|otsu] [--binary-threshold 150 --binary-mask-upper-threshold 50] [--adaptive-block-size 31] [--adaptive-c 5.0]"
    );
    println!(
        "Usage: diagnostics_cli presence-check-once --template <selected_templates.json> [--camera-id opencv-index:0] [--threshold 0.50] [--frame-width 640 --frame-height 480] [--audit-dir <dir>] [--disable-screen-snapshot]"
    );
    println!(
        "Usage: diagnostics_cli presence-policy-simulate --events owner,no-face,unknown,person,person-left,person-absent,camera-unavailable [--threshold 0.50]"
    );
    println!(
        "Usage: diagnostics_cli presence-monitor-simulate --events owner,no-face,unknown,person,person-left,person-absent,camera-unavailable [--threshold 0.50] [--max-iterations 10]"
    );
    println!(
        "Usage: diagnostics_cli presence-monitor-camera-debug --template <selected_templates.json> [--camera-id opencv-index:0] [--threshold 0.50] [--iterations 3] [--frame-width 640 --frame-height 480]"
    );
    println!(
        "Usage: diagnostics_cli presence-person-benchmark [--detector mobilenet-ssd|ssdlite-onnx|yolov8-onnx] [--model <path>] [--config <path>] [--fps 2] [--duration-seconds 120] [--confidence 0.50] [--camera-id opencv-index:0] [--frame-width 640 --frame-height 480] [--output-dir <dir>]"
    );
    println!(
        "Usage: diagnostics_cli presence-person-lock-debug [--detector mobilenet-ssd|ssdlite-onnx|yolov8-onnx|ort-yolov8-onnx] [--model <path>] [--camera-id opencv-index:0] [--frame-width 640 --frame-height 480] [--iterations 8] [--absent-frames 6] [--real-lock] [--output-dir <dir>]"
    );
    println!("Usage: diagnostics_cli screen-snapshot-debug --output <path.bmp>");
    println!(
        "Usage: diagnostics_cli face-auth-debug --template <path> [--frames 30] [--camera-id opencv-index:0]"
    );
    println!("Usage: diagnostics_cli verify-face --image <path> --template <path>");
    println!(
        "Usage: diagnostics_cli camera-auth --template <path> [--camera-id opencv-index:0] [--frame-width 640 --frame-height 480]"
    );
    println!(
        "Usage: diagnostics_cli calibrate-threshold --template <path> [--samples 20] [--max-frames 120] [--camera-id opencv-index:0]"
    );
}

fn argument_value<'args>(args: &'args [String], name: &str) -> Option<&'args str> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argument_value_reads_pipe_name() {
        let args = vec![
            "diagnostics_cli".to_owned(),
            "health-check".to_owned(),
            "--pipe-name".to_owned(),
            r"\\.\pipe\test".to_owned(),
        ];

        assert_eq!(argument_value(&args, "--pipe-name"), Some(r"\\.\pipe\test"));
    }

    #[test]
    fn argument_value_reads_grant_id() {
        let args = vec![
            "diagnostics_cli".to_owned(),
            "fetch-credential".to_owned(),
            "--grant-id".to_owned(),
            "grant-1".to_owned(),
        ];

        assert_eq!(argument_value(&args, "--grant-id"), Some("grant-1"));
    }

    #[test]
    fn optional_threshold_uses_explicit_argument_name() -> Result<(), DiagnosticError> {
        let args = vec![
            "diagnostics_cli".to_owned(),
            "verify-face".to_owned(),
            "--threshold".to_owned(),
            "0.91".to_owned(),
        ];

        assert_eq!(optional_f32(&args, "--threshold")?, Some(0.91));
        Ok(())
    }

    #[test]
    fn wake_auth_source_defaults_to_manual_test() -> Result<(), DiagnosticError> {
        let args = vec!["diagnostics_cli".to_owned(), "wake-auth".to_owned()];

        assert_eq!(wake_auth_source(&args)?, AuthSource::ManualTest);
        Ok(())
    }

    #[test]
    fn wake_auth_source_reads_local_camera() -> Result<(), DiagnosticError> {
        let args = vec![
            "diagnostics_cli".to_owned(),
            "wake-auth".to_owned(),
            "--source".to_owned(),
            "local-camera".to_owned(),
        ];

        assert_eq!(wake_auth_source(&args)?, AuthSource::LocalCamera);
        Ok(())
    }

    #[test]
    fn screen_geometry_diagnostics_are_disabled_by_default() -> Result<(), DiagnosticError> {
        let args = vec![
            "diagnostics_cli".to_owned(),
            "liveness-screen-debug".to_owned(),
        ];

        assert_eq!(screen_replay_geometry_config(&args)?, None);
        Ok(())
    }

    #[test]
    fn screen_geometry_diagnostics_require_explicit_opt_in() -> Result<(), DiagnosticError> {
        let args = vec![
            "diagnostics_cli".to_owned(),
            "liveness-screen-debug".to_owned(),
            "--enable-screen-geometry-diagnostics".to_owned(),
        ];

        assert!(screen_replay_geometry_config(&args)?.is_some());
        Ok(())
    }

    #[test]
    fn auth_grant_from_event_accepts_auth_succeeded() -> Result<(), DiagnosticError> {
        let grant = AuthGrant {
            grant_id: GrantId("grant-1".to_owned()),
            nonce: Nonce("nonce-1".to_owned()),
            session_id: SessionId("session-1".to_owned()),
            user_id: UserId("user-1".to_owned()),
            source: AuthSource::LocalCamera,
            score: common_protocol::AuthScore {
                match_score: 0.8,
                liveness_score: None,
            },
            issued_at_unix_ms: 1,
            expires_at_unix_ms: 2,
        };

        let parsed = auth_grant_from_event(ServiceEvent::AuthSucceeded {
            grant: grant.clone(),
        })?;

        assert_eq!(parsed, grant);
        Ok(())
    }

    #[test]
    fn auth_grant_from_event_preserves_auth_failure_reason() {
        let result = auth_grant_from_event(ServiceEvent::AuthFailed {
            session_id: SessionId("session-1".to_owned()),
            reason: AuthFailureReason::NoFaceDetected,
        });

        assert!(matches!(
            result,
            Err(DiagnosticError::AuthRejected(
                AuthFailureReason::NoFaceDetected
            ))
        ));
    }

    #[test]
    fn percentile_sorted_returns_nearest_rank_score() {
        let scores = vec![0.1, 0.2, 0.3, 0.4, 0.5];

        assert_eq!(percentile_sorted(&scores, 0.0), 0.1);
        assert_eq!(percentile_sorted(&scores, 0.5), 0.3);
        assert_eq!(percentile_sorted(&scores, 1.0), 0.5);
    }

    #[test]
    fn latency_summary_reports_distribution() {
        let summary = LatencySummary::from_samples(&[10.0, 30.0, 20.0]);

        assert_eq!(
            summary,
            LatencySummary {
                count: 3,
                avg_ms: 20.0,
                p50_ms: 20.0,
                p90_ms: 30.0,
                max_ms: 30.0,
            }
        );
    }

    #[test]
    fn guided_enrollment_pitch_steps_are_optional() {
        assert!(!guided_enrollment_step_is_optional(
            GuidedEnrollmentStep::FrontalPrimary
        ));
        assert!(!guided_enrollment_step_is_optional(
            GuidedEnrollmentStep::YawLeftMild
        ));
        assert!(!guided_enrollment_step_is_optional(
            GuidedEnrollmentStep::YawRightMild
        ));
        assert!(guided_enrollment_step_is_optional(
            GuidedEnrollmentStep::PitchDownMild
        ));
        assert!(guided_enrollment_step_is_optional(
            GuidedEnrollmentStep::PitchUpMild
        ));
    }

    #[test]
    fn profiled_camera_provider_config_reads_stored_backend() -> Result<(), String> {
        let config_path = std::env::current_exe()
            .map_err(|error| error.to_string())?
            .parent()
            .ok_or_else(|| "test executable parent directory is unavailable".to_owned())?
            .join("runtime")
            .join("camera_backend_profiles.json");
        let original_config = fs::read(&config_path).ok();
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(
            &config_path,
            r#"{
  "profiles": [
    {
      "camera_id": "opencv-index:77",
      "display_name": "Profiled camera",
      "preferred_backend": "dshow",
      "open_ms": 10,
      "read_ms": 5,
      "frame_width": 640,
      "frame_height": 480,
      "measured_at_unix_ms": 1
    }
  ]
}"#,
        )
        .map_err(|error| error.to_string())?;

        let args = vec!["diagnostics_cli".to_owned(), "guided-enroll".to_owned()];
        let config =
            camera_provider_config_for_camera(&args, &CameraId("opencv-index:77".to_owned()))
                .map_err(|error| format!("{error:?}"))?;

        if let Some(original_config) = original_config {
            fs::write(config_path, original_config).map_err(|error| error.to_string())?;
        } else {
            let _ = fs::remove_file(config_path);
        }

        assert_eq!(
            config.preferred_backend,
            Some(video_provider::OpenCvCameraBackend::Dshow)
        );
        Ok(())
    }

    #[test]
    fn score_summary_rejects_empty_sample_set() {
        let result = print_score_summary(&[]);

        assert!(matches!(
            result,
            Err(DiagnosticError::AuthRejected(
                AuthFailureReason::NoFaceDetected
            ))
        ));
    }
}
