use std::{
    fmt, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use common_protocol::{
    AccountType, AuthFailureReason, AuthGrant, AuthSource, CredentialRef, GrantId, Nonce,
    PIPE_NAME, ProtocolError, SERVICE_NAME, ServiceEvent, ServiceRequest, SessionId, UserId,
};
use face_auth::{
    AttemptPolicy, AttemptPolicyConfig, FaceAuthenticator, FaceEnrollmentService,
    RecognitionTemplates,
};
use face_engine::{
    FaceEngineError, FaceModelProvider, FaceTemplate, FaceTemplateCodecError, FaceTemplateMatcher,
    FaceTemplateRef, OpenCvFaceModelConfig, OpenCvFaceModelProvider, SFACE_COSINE_MATCH_THRESHOLD,
};
use ipc::{IpcClient, NamedPipeClient};
use video_provider::OpenCvCameraProviderConfig;
use video_provider::{CameraId, OpenCvCameraProvider, VideoError, VideoFrameProvider};
use win_service::credential_store_config::{
    ServiceCredentialStorePaths, WindowsCredentialEnrollment, enroll_windows_credential,
};

const DEFAULT_YUNET_MODEL_PATH: &str = "models/face_detection_yunet_2023mar.onnx";
const DEFAULT_SFACE_MODEL_PATH: &str = "models/face_recognition_sface_2021dec.onnx";

#[derive(Debug)]
pub enum DiagnosticError {
    Protocol(ProtocolError),
    Video(VideoError),
    Face(FaceEngineError),
    TemplateCodec(FaceTemplateCodecError),
    IoFailed,
    InvalidArgument,
    AuthRejected(AuthFailureReason),
    PasswordPromptFailed,
    PasswordConfirmationMismatch,
}

impl fmt::Display for DiagnosticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => write!(formatter, "protocol error: {error:?}"),
            Self::Video(error) => write!(formatter, "video error: {error:?}"),
            Self::Face(error) => write!(formatter, "face engine error: {error:?}"),
            Self::TemplateCodec(error) => write!(formatter, "template codec error: {error:?}"),
            Self::IoFailed => write!(formatter, "I/O operation failed"),
            Self::InvalidArgument => write!(formatter, "invalid or missing argument"),
            Self::AuthRejected(reason) => write!(formatter, "authentication rejected: {reason:?}"),
            Self::PasswordPromptFailed => write!(formatter, "password prompt failed"),
            Self::PasswordConfirmationMismatch => {
                write!(formatter, "password confirmation mismatch")
            }
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
    } else if args.iter().any(|arg| arg == "test-camera") {
        run_test_camera(&args)?;
    } else if args.iter().any(|arg| arg == "test-face") {
        run_test_face(&args)?;
    } else if args.iter().any(|arg| arg == "enroll-face") {
        run_enroll_face(&args)?;
    } else if args.iter().any(|arg| arg == "enroll-camera") {
        run_enroll_camera(&args)?;
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
    send_request(pipe_name, ServiceRequest::WakeAuth { session_id, source })
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
    let grant = auth_grant_from_event(wake_event)?;

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
    let sources = provider.list_sources()?;
    let camera_id = selected_camera_id(args, &sources)?;
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

fn run_verify_face(args: &[String]) -> Result<(), DiagnosticError> {
    let image_path = argument_value(args, "--image").ok_or(DiagnosticError::InvalidArgument)?;
    let template_path =
        argument_value(args, "--template").ok_or(DiagnosticError::InvalidArgument)?;
    let threshold = optional_f32(args, "--threshold")?.unwrap_or(SFACE_COSINE_MATCH_THRESHOLD);
    let templates = RecognitionTemplates::new(vec![read_template(template_path)?]);
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
    Ok(())
}

fn run_camera_auth(args: &[String]) -> Result<(), DiagnosticError> {
    let template_path =
        argument_value(args, "--template").ok_or(DiagnosticError::InvalidArgument)?;
    let threshold = optional_f32(args, "--threshold")?.unwrap_or(SFACE_COSINE_MATCH_THRESHOLD);
    let required_consecutive_match_count =
        optional_u32(args, "--required-consecutive")?.unwrap_or(2);
    let max_frames = optional_u32(args, "--max-frames")?.unwrap_or(30);
    let templates = RecognitionTemplates::new(vec![read_template(template_path)?]);

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

fn build_loaded_model_provider(
    args: &[String],
) -> Result<OpenCvFaceModelProvider, DiagnosticError> {
    let yunet_model_path = model_path(args, "--yunet-model", DEFAULT_YUNET_MODEL_PATH);
    let sface_model_path = model_path(args, "--sface-model", DEFAULT_SFACE_MODEL_PATH);
    let threshold = optional_f32(args, "--threshold")?.unwrap_or(SFACE_COSINE_MATCH_THRESHOLD);

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

fn build_camera_provider(args: &[String]) -> Result<OpenCvCameraProvider, DiagnosticError> {
    Ok(OpenCvCameraProvider::new(OpenCvCameraProviderConfig {
        max_camera_index: 8,
        requested_frame_width: optional_u32(args, "--frame-width")?,
        requested_frame_height: optional_u32(args, "--frame-height")?,
    }))
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
        "Usage: diagnostics_cli test-face --image <path> [--yunet-model <onnx>] [--sface-model <onnx>]"
    );
    println!(
        "Usage: diagnostics_cli enroll-face --image <path> --template-out <path> [--user-id <id>]"
    );
    println!(
        "Usage: diagnostics_cli enroll-camera --template-out <path> [--camera-id opencv-index:0] [--frame-width 640 --frame-height 480] [--user-id <id>]"
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
