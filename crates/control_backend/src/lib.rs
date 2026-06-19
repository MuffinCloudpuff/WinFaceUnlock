use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, MutexGuard},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use common_protocol::{
    AuthFailureReason, AuthSource, PIPE_NAME, ProtocolError, ServiceEvent, ServiceRequest,
    SessionId,
};
use control_protocol::{
    CONTROL_PROTOCOL_VERSION, CameraDeviceList, CameraDeviceSummary, ControlErrorCode,
    ControlOperation, ControlOperationStatus, ControlRequestEnvelope, ControlResponseEnvelope,
    ControlSettingsPatch, ControlSettingsSnapshot, DashboardStatus, DeleteFaceTemplateOutcome,
    DeleteFaceTemplatePayload, FaceAuthSelfTestOutcome, FaceAuthSelfTestPayload,
    FaceEnrollmentFinishOutcome, FaceEnrollmentFrameResult, FaceEnrollmentPreviewFrame,
    FaceEnrollmentSessionPayload, FaceEnrollmentSessionState, FaceEnrollmentSessionStatus,
    FaceEnrollmentStartPayload, FaceTemplateEnrollmentSummary, FaceTemplateList,
    FaceTemplateSourceState, WindowsCredentialAccountProfile, WindowsCredentialEnrollmentOutcome,
    WindowsCredentialEnrollmentPayload,
};
use control_status::{
    ACTIVE_SERVICE_FACE_TEMPLATE_REF, ControlStatusError, FaceTemplateStatusError,
    WindowsControlSettingsStore, WindowsDashboardStatusProvider, WindowsFaceTemplateStatusStore,
    summarize_selected_face_template_file,
};
use ipc::IpcClient;
use serde::Deserialize;
use serde_json::{Value, json};

const ENV_CONTROL_INSTALL_DIR: &str = "WINFACEUNLOCK_INSTALL_DIR";
const ENV_CONTROL_DIAGNOSTICS_CLI: &str = "WINFACEUNLOCK_DIAGNOSTICS_CLI";
const ENV_CONTROL_FACE_ENROLLMENT_OUTPUT_DIR: &str = "WINFACEUNLOCK_FACE_ENROLLMENT_OUTPUT_DIR";
const ENV_YUNET_MODEL_PATH: &str = "WINFACEUNLOCK_YUNET_MODEL_PATH";
const ENV_SFACE_MODEL_PATH: &str = "WINFACEUNLOCK_SFACE_MODEL_PATH";
const DIAGNOSTICS_CLI_FILE_NAME: &str = "diagnostics_cli.exe";
const FACE_ENROLLMENT_OUTPUT_DIR_NAME: &str = "face-enrollment";
const DEFAULT_YUNET_MODEL_PATH: &str = "models/face_detection_yunet_2023mar.onnx";
const DEFAULT_SFACE_MODEL_PATH: &str = "models/face_recognition_sface_2021dec.onnx";
const SELECTED_TEMPLATES_FILE_NAME: &str = "selected_templates.json";
const ENROLLMENT_STATUS_FILE_NAME: &str = "enrollment_status.json";
const ENROLLMENT_PREVIEW_FRAME_FILE_NAME: &str = "preview_frame.jpg";
const FACE_ENROLLMENT_STARTUP_TIMEOUT: Duration = Duration::from_secs(12);
const PREVIEW_EVENT_PREFIX: &str = "WINFACEUNLOCK_PREVIEW_FRAME ";

pub trait DashboardStatusProvider {
    fn load_dashboard_status(&self) -> Result<DashboardStatus, ControlBackendError>;
}

pub trait ControlSettingsStore {
    fn load_settings(&self) -> Result<ControlSettingsSnapshot, ControlBackendError>;

    fn update_settings(
        &self,
        patch: &ControlSettingsPatch,
    ) -> Result<ControlSettingsSnapshot, ControlBackendError>;
}

pub trait WindowsCredentialEnrollmentStore {
    fn load_windows_credential_account(
        &self,
    ) -> Result<WindowsCredentialAccountProfile, ControlBackendError>;

    fn enroll_windows_credential(
        &self,
        payload: &WindowsCredentialEnrollmentPayload,
        password_secret: WindowsCredentialSecret,
    ) -> Result<WindowsCredentialEnrollmentOutcome, ControlBackendError>;
}

pub trait FaceTemplateManagementStore {
    fn list_face_templates(&self) -> Result<FaceTemplateList, ControlBackendError>;

    fn delete_face_template(
        &self,
        payload: &DeleteFaceTemplatePayload,
    ) -> Result<DeleteFaceTemplateOutcome, ControlBackendError>;
}

pub trait CameraDiscoveryProvider {
    fn list_cameras(&self) -> Result<CameraDeviceList, ControlBackendError>;
}

pub trait FaceEnrollmentRuntime {
    fn start_face_enrollment(
        &self,
        payload: &FaceEnrollmentStartPayload,
    ) -> Result<FaceEnrollmentSessionStatus, ControlBackendError>;

    fn get_face_enrollment_status(
        &self,
        payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentSessionStatus, ControlBackendError>;

    fn get_face_enrollment_preview(
        &self,
        payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentPreviewFrame, ControlBackendError>;

    fn cancel_face_enrollment(
        &self,
        payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentSessionStatus, ControlBackendError>;

    fn finish_face_enrollment(
        &self,
        payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentFinishOutcome, ControlBackendError>;
}

pub trait FaceEnrollmentTemplateApplier {
    fn apply_face_enrollment_template(
        &self,
        template_path: &Path,
    ) -> Result<(), ControlBackendError>;
}

pub trait FaceAuthSelfTestRunner {
    fn run_face_auth_self_test(
        &self,
        payload: &FaceAuthSelfTestPayload,
    ) -> Result<FaceAuthSelfTestOutcome, ControlBackendError>;
}

pub trait FaceAuthServiceClient {
    fn send_service_request(&self, request: ServiceRequest) -> Result<ServiceEvent, ProtocolError>;
}

#[derive(Clone, Debug)]
pub struct ServiceFaceAuthSelfTestRunner<C> {
    service_client: C,
}

impl<C> ServiceFaceAuthSelfTestRunner<C> {
    pub fn new(service_client: C) -> Self {
        Self { service_client }
    }
}

impl<C> FaceAuthSelfTestRunner for ServiceFaceAuthSelfTestRunner<C>
where
    C: FaceAuthServiceClient,
{
    fn run_face_auth_self_test(
        &self,
        payload: &FaceAuthSelfTestPayload,
    ) -> Result<FaceAuthSelfTestOutcome, ControlBackendError> {
        let session_id = SessionId(payload.session_id.clone());
        let wake_event = self
            .service_client
            .send_service_request(ServiceRequest::WakeAuth {
                session_id: session_id.clone(),
                source: AuthSource::LocalCamera,
            })
            .map_err(auth_wake_protocol_error)?;

        let grant = match wake_event {
            ServiceEvent::AuthSucceeded { grant } => grant,
            ServiceEvent::AuthFailed { reason, .. } => {
                return Err(auth_failure_reason_to_control_error(reason));
            }
            ServiceEvent::RequestRejected { reason } => {
                return Err(auth_wake_protocol_error(reason));
            }
            _ => {
                return Err(ControlBackendError::auth_self_test_failed(
                    "service returned an invalid wake-auth response",
                ));
            }
        };

        if !payload.require_credential_ready {
            return Ok(FaceAuthSelfTestOutcome {
                session_id: payload.session_id.clone(),
                auth_match_passed: true,
                grant_issued: true,
                credential_material_ready: false,
                credential_decryption_succeeded: false,
                pipe_delivery_confirmed: true,
                best_match_score: Some(grant.score.match_score),
                matched_face_template_ref: None,
            });
        }

        let credential_event = self
            .service_client
            .send_service_request(ServiceRequest::FetchCredentialMaterial {
                session_id,
                grant_id: grant.grant_id.clone(),
                nonce: grant.nonce.clone(),
            })
            .map_err(credential_material_protocol_error)?;

        match credential_event {
            ServiceEvent::CredentialMaterialReady { .. } => Ok(FaceAuthSelfTestOutcome {
                session_id: payload.session_id.clone(),
                auth_match_passed: true,
                grant_issued: true,
                credential_material_ready: true,
                credential_decryption_succeeded: true,
                pipe_delivery_confirmed: true,
                best_match_score: Some(grant.score.match_score),
                matched_face_template_ref: None,
            }),
            ServiceEvent::RequestRejected { reason } => {
                Err(credential_material_protocol_error(reason))
            }
            _ => Err(ControlBackendError::credential_material_unavailable(
                "service returned an invalid credential material response",
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServiceIpcFaceEnrollmentTemplateApplier<C = NamedPipeFaceAuthServiceClient> {
    service_client: C,
}

impl ServiceIpcFaceEnrollmentTemplateApplier<NamedPipeFaceAuthServiceClient> {
    pub fn default_named_pipe() -> Self {
        Self::new(NamedPipeFaceAuthServiceClient::default())
    }
}

impl<C> ServiceIpcFaceEnrollmentTemplateApplier<C> {
    pub fn new(service_client: C) -> Self {
        Self { service_client }
    }
}

impl<C> FaceEnrollmentTemplateApplier for ServiceIpcFaceEnrollmentTemplateApplier<C>
where
    C: FaceAuthServiceClient,
{
    fn apply_face_enrollment_template(
        &self,
        template_path: &Path,
    ) -> Result<(), ControlBackendError> {
        let event = self
            .service_client
            .send_service_request(ServiceRequest::ApplyFaceTemplate {
                template_path: template_path.to_path_buf(),
            })
            .map_err(face_template_apply_protocol_error)?;

        match event {
            ServiceEvent::FaceTemplateApplied { .. } => Ok(()),
            ServiceEvent::RequestRejected { reason } => {
                Err(face_template_apply_protocol_error(reason))
            }
            _ => Err(ControlBackendError::face_enrollment_failed(
                "service returned an invalid face template apply response",
            )),
        }
    }
}

#[derive(Clone)]
pub struct CommandFaceEnrollmentRuntime<
    P = DiagnosticsCliEnrollmentProcessFactory,
    A = ServiceIpcFaceEnrollmentTemplateApplier,
> {
    process_factory: P,
    template_applier: A,
    sessions: Arc<Mutex<HashMap<String, CommandFaceEnrollmentSession>>>,
}

impl
    CommandFaceEnrollmentRuntime<
        DiagnosticsCliEnrollmentProcessFactory,
        ServiceIpcFaceEnrollmentTemplateApplier,
    >
{
    pub fn from_environment_or_default() -> Self {
        Self::new(DiagnosticsCliEnrollmentProcessFactory::from_environment_or_default())
    }
}

impl<P> CommandFaceEnrollmentRuntime<P, ServiceIpcFaceEnrollmentTemplateApplier> {
    pub fn new(process_factory: P) -> Self {
        Self::with_template_applier(
            process_factory,
            ServiceIpcFaceEnrollmentTemplateApplier::default_named_pipe(),
        )
    }
}

impl<P, A> CommandFaceEnrollmentRuntime<P, A> {
    pub fn with_template_applier(process_factory: P, template_applier: A) -> Self {
        Self {
            process_factory,
            template_applier,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<P, A> FaceEnrollmentRuntime for CommandFaceEnrollmentRuntime<P, A>
where
    P: FaceEnrollmentProcessFactory,
    A: FaceEnrollmentTemplateApplier,
{
    fn start_face_enrollment(
        &self,
        payload: &FaceEnrollmentStartPayload,
    ) -> Result<FaceEnrollmentSessionStatus, ControlBackendError> {
        let mut sessions = lock_enrollment_sessions(&self.sessions)?;
        let has_running_session =
            sessions
                .values_mut()
                .try_fold(false, |has_running, session| {
                    refresh_command_face_enrollment_session(session)?;
                    Ok::<bool, ControlBackendError>(has_running || session.is_running())
                })?;
        if has_running_session {
            return Err(ControlBackendError::face_enrollment_already_running(
                "another face enrollment session is already running",
            ));
        }

        let session_id = next_face_enrollment_session_id(&sessions);
        let output_dir = self
            .process_factory
            .prepare_output_dir(&session_id)
            .map_err(face_enrollment_process_error_to_control_error)?;
        let process = self
            .process_factory
            .start_guided_enrollment(&session_id, payload, &output_dir)
            .map_err(face_enrollment_process_error_to_control_error)?;
        let session = CommandFaceEnrollmentSession::running(
            session_id.clone(),
            payload.clone(),
            output_dir,
            process,
        );
        let status = session.status();
        sessions.insert(session_id, session);
        Ok(status)
    }

    fn get_face_enrollment_status(
        &self,
        payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentSessionStatus, ControlBackendError> {
        let mut sessions = lock_enrollment_sessions(&self.sessions)?;
        let session = sessions
            .get_mut(&payload.enrollment_session_id)
            .ok_or_else(|| {
                ControlBackendError::face_enrollment_session_not_found(format!(
                    "face enrollment session {} was not found",
                    payload.enrollment_session_id
                ))
            })?;
        refresh_command_face_enrollment_session(session)?;
        Ok(session.status())
    }

    fn get_face_enrollment_preview(
        &self,
        payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentPreviewFrame, ControlBackendError> {
        let mut sessions = lock_enrollment_sessions(&self.sessions)?;
        let session = sessions
            .get_mut(&payload.enrollment_session_id)
            .ok_or_else(|| {
                ControlBackendError::face_enrollment_session_not_found(format!(
                    "face enrollment session {} was not found",
                    payload.enrollment_session_id
                ))
            })?;
        refresh_command_face_enrollment_session(session)?;
        Ok(read_enrollment_preview_frame(session))
    }

    fn cancel_face_enrollment(
        &self,
        payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentSessionStatus, ControlBackendError> {
        let mut sessions = lock_enrollment_sessions(&self.sessions)?;
        let session = sessions
            .get_mut(&payload.enrollment_session_id)
            .ok_or_else(|| {
                ControlBackendError::face_enrollment_session_not_found(format!(
                    "face enrollment session {} was not found",
                    payload.enrollment_session_id
                ))
            })?;
        refresh_command_face_enrollment_session(session)?;
        if !session.is_running() {
            return Ok(session.status());
        }

        if let Some(process) = session.process.as_mut() {
            process
                .cancel()
                .map_err(face_enrollment_process_error_to_control_error)?;
        }
        session.process = None;
        session.session_state = FaceEnrollmentSessionState::Cancelled;
        session.failure_message = None;
        Ok(session.status())
    }

    fn finish_face_enrollment(
        &self,
        payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentFinishOutcome, ControlBackendError> {
        let mut sessions = lock_enrollment_sessions(&self.sessions)?;
        let session = sessions
            .get_mut(&payload.enrollment_session_id)
            .ok_or_else(|| {
                ControlBackendError::face_enrollment_session_not_found(format!(
                    "face enrollment session {} was not found",
                    payload.enrollment_session_id
                ))
            })?;
        refresh_command_face_enrollment_session(session)?;

        match session.session_state {
            FaceEnrollmentSessionState::Completed => {
                let template_summary = session.template_summary.clone().ok_or_else(|| {
                    ControlBackendError::face_enrollment_failed(
                        "completed face enrollment session is missing template summary",
                    )
                })?;
                let template_path = session.output_dir.join(SELECTED_TEMPLATES_FILE_NAME);
                self.template_applier
                    .apply_face_enrollment_template(&template_path)?;
                Ok(FaceEnrollmentFinishOutcome {
                    enrollment_session_id: session.enrollment_session_id.clone(),
                    session_state: FaceEnrollmentSessionState::Completed,
                    face_template_ref: enrollment_face_template_ref(&session.enrollment_session_id),
                    user_id: session.start_payload.user_id.clone(),
                    template_summary,
                    service_auth_configured: true,
                    service_auth_configuration_error: None,
                })
            }
            FaceEnrollmentSessionState::Failed => Err(ControlBackendError::face_enrollment_failed(
                session.failure_message.clone().unwrap_or_else(|| {
                    "face enrollment session failed before a template was generated".to_owned()
                }),
            )),
            FaceEnrollmentSessionState::Cancelled => {
                Err(ControlBackendError::face_enrollment_cancelled(
                    "face enrollment session was cancelled",
                ))
            }
            _ => Err(ControlBackendError::face_enrollment_failed(
                "face enrollment session is still running",
            )),
        }
    }
}

pub trait FaceEnrollmentProcessFactory: Clone + Send + Sync + 'static {
    type Process: FaceEnrollmentProcess;

    fn prepare_output_dir(&self, session_id: &str) -> Result<PathBuf, FaceEnrollmentProcessError>;

    fn start_guided_enrollment(
        &self,
        session_id: &str,
        payload: &FaceEnrollmentStartPayload,
        output_dir: &Path,
    ) -> Result<Self::Process, FaceEnrollmentProcessError>;
}

pub trait FaceEnrollmentPreviewEventSink: Clone + Send + Sync + 'static {
    fn emit_preview_frame(&self, event_json: &str);
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoopFaceEnrollmentPreviewEventSink;

impl FaceEnrollmentPreviewEventSink for NoopFaceEnrollmentPreviewEventSink {
    fn emit_preview_frame(&self, _event_json: &str) {}
}

pub trait FaceEnrollmentProcess: Send + 'static {
    fn try_wait(&mut self) -> Result<Option<FaceEnrollmentExit>, FaceEnrollmentProcessError>;

    fn cancel(&mut self) -> Result<(), FaceEnrollmentProcessError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FaceEnrollmentExit {
    pub exit_success: bool,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FaceEnrollmentProcessError {
    MissingDiagnosticsCli(String),
    OutputDirectoryUnavailable(String),
    CameraUnavailable(String),
    ModelUnavailable(String),
    ProcessStartFailed(String),
    ProcessStatusFailed(String),
    ProcessCancelFailed(String),
    TemplateFileMissing(String),
    TemplateParseFailed(String),
    TemplateEmpty(String),
    PermissionDenied(String),
}

#[derive(Clone, Debug)]
pub struct DiagnosticsCliEnrollmentProcessFactory<S = NoopFaceEnrollmentPreviewEventSink> {
    diagnostics_cli_path: PathBuf,
    enrollment_root_dir: PathBuf,
    preview_event_sink: S,
}

impl DiagnosticsCliEnrollmentProcessFactory<NoopFaceEnrollmentPreviewEventSink> {
    pub fn from_environment_or_default() -> Self {
        let diagnostics_cli_path = std::env::var_os(ENV_CONTROL_DIAGNOSTICS_CLI)
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os(ENV_CONTROL_INSTALL_DIR)
                    .map(PathBuf::from)
                    .unwrap_or_else(default_install_dir)
                    .join(DIAGNOSTICS_CLI_FILE_NAME)
            });
        let enrollment_root_dir = std::env::var_os(ENV_CONTROL_FACE_ENROLLMENT_OUTPUT_DIR)
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os(ENV_CONTROL_INSTALL_DIR)
                    .map(PathBuf::from)
                    .unwrap_or_else(default_install_dir)
                    .join(FACE_ENROLLMENT_OUTPUT_DIR_NAME)
            });
        Self::new(diagnostics_cli_path, enrollment_root_dir)
    }

    pub fn new(diagnostics_cli_path: PathBuf, enrollment_root_dir: PathBuf) -> Self {
        Self::with_preview_event_sink(
            diagnostics_cli_path,
            enrollment_root_dir,
            NoopFaceEnrollmentPreviewEventSink,
        )
    }
}

impl<S> DiagnosticsCliEnrollmentProcessFactory<S>
where
    S: FaceEnrollmentPreviewEventSink,
{
    pub fn with_preview_event_sink(
        diagnostics_cli_path: PathBuf,
        enrollment_root_dir: PathBuf,
        preview_event_sink: S,
    ) -> Self {
        Self {
            diagnostics_cli_path,
            enrollment_root_dir,
            preview_event_sink,
        }
    }

    pub fn diagnostics_cli_path(&self) -> &Path {
        &self.diagnostics_cli_path
    }

    pub fn enrollment_root_dir(&self) -> &Path {
        &self.enrollment_root_dir
    }

    pub fn list_cameras(&self) -> Result<CameraDeviceList, FaceEnrollmentProcessError> {
        if !self.diagnostics_cli_path.exists() {
            return Err(FaceEnrollmentProcessError::MissingDiagnosticsCli(
                "camera discovery runtime executable is unavailable".to_owned(),
            ));
        }

        let mut command = Command::new(&self.diagnostics_cli_path);
        if let Some(parent) = self.diagnostics_cli_path.parent() {
            command.current_dir(parent);
        }
        let output = command
            .arg("list-cameras")
            .stdin(Stdio::null())
            .output()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => FaceEnrollmentProcessError::MissingDiagnosticsCli(
                    "camera discovery runtime executable is unavailable".to_owned(),
                ),
                std::io::ErrorKind::PermissionDenied => {
                    FaceEnrollmentProcessError::PermissionDenied(
                        "permission denied while listing cameras".to_owned(),
                    )
                }
                _ => FaceEnrollmentProcessError::ProcessStartFailed(format!(
                    "failed to list cameras: {error}"
                )),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(FaceEnrollmentProcessError::CameraUnavailable(format!(
                "camera discovery failed: {}",
                stderr.trim()
            )));
        }

        parse_diagnostics_camera_list(&String::from_utf8_lossy(&output.stdout))
            .map_err(FaceEnrollmentProcessError::CameraUnavailable)
    }

    fn resolve_model_path(&self, env_key: &str, default_relative_path: &str) -> PathBuf {
        if let Some(explicit_path) = std::env::var_os(env_key) {
            return PathBuf::from(explicit_path);
        }

        if let Some(path) = self.find_model_path_in_ancestors(default_relative_path) {
            return path;
        }

        std::env::var_os(ENV_CONTROL_INSTALL_DIR)
            .map(PathBuf::from)
            .unwrap_or_else(default_install_dir)
            .join(default_relative_path)
    }

    fn find_model_path_in_ancestors(&self, default_relative_path: &str) -> Option<PathBuf> {
        let mut current_dir = self.diagnostics_cli_path.parent();
        while let Some(dir) = current_dir {
            let candidate = dir.join(default_relative_path);
            if candidate.exists() {
                return Some(candidate);
            }
            current_dir = dir.parent();
        }
        None
    }
}

impl<S> CameraDiscoveryProvider for DiagnosticsCliEnrollmentProcessFactory<S>
where
    S: FaceEnrollmentPreviewEventSink,
{
    fn list_cameras(&self) -> Result<CameraDeviceList, ControlBackendError> {
        DiagnosticsCliEnrollmentProcessFactory::list_cameras(self)
            .map_err(face_enrollment_process_error_to_control_error)
    }
}

impl<S> FaceEnrollmentProcessFactory for DiagnosticsCliEnrollmentProcessFactory<S>
where
    S: FaceEnrollmentPreviewEventSink,
{
    type Process = ChildFaceEnrollmentProcess;

    fn prepare_output_dir(&self, session_id: &str) -> Result<PathBuf, FaceEnrollmentProcessError> {
        let output_dir = self
            .enrollment_root_dir
            .join(sanitize_session_path_segment(session_id));
        fs::create_dir_all(&output_dir).map_err(|error| {
            FaceEnrollmentProcessError::OutputDirectoryUnavailable(format!(
                "face enrollment output directory is unavailable: {error}"
            ))
        })?;
        Ok(output_dir)
    }

    fn start_guided_enrollment(
        &self,
        session_id: &str,
        payload: &FaceEnrollmentStartPayload,
        output_dir: &Path,
    ) -> Result<Self::Process, FaceEnrollmentProcessError> {
        if !self.diagnostics_cli_path.exists() {
            return Err(FaceEnrollmentProcessError::MissingDiagnosticsCli(
                "face enrollment runtime executable is unavailable".to_owned(),
            ));
        }

        let mut command = Command::new(&self.diagnostics_cli_path);
        if let Some(parent) = self.diagnostics_cli_path.parent() {
            command.current_dir(parent);
        }
        command
            .arg("guided-enroll")
            .arg("--output-dir")
            .arg(output_dir)
            .arg("--camera-id")
            .arg(&payload.camera_id)
            .arg("--yunet-model")
            .arg(self.resolve_model_path(ENV_YUNET_MODEL_PATH, DEFAULT_YUNET_MODEL_PATH))
            .arg("--sface-model")
            .arg(self.resolve_model_path(ENV_SFACE_MODEL_PATH, DEFAULT_SFACE_MODEL_PATH))
            .arg("--user-id")
            .arg(&payload.user_id)
            .arg("--enrollment-id")
            .arg(session_id)
            .arg("--accepted-frames-per-step")
            .arg("6")
            .arg("--max-wait-frames-per-step")
            .arg("180")
            .arg("--max-frames-per-step")
            .arg("180")
            .arg("--pose-ready-consecutive")
            .arg("3")
            .arg("--pose-ready-min-fit")
            .arg("0.25")
            .arg("--frame-delay-ms")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if payload.allow_partial_enrollment {
            command.arg("--allow-partial-enrollment");
        }

        let mut child = command.spawn().map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => FaceEnrollmentProcessError::MissingDiagnosticsCli(
                "face enrollment runtime executable is unavailable".to_owned(),
            ),
            std::io::ErrorKind::PermissionDenied => FaceEnrollmentProcessError::PermissionDenied(
                "permission denied while starting face enrollment".to_owned(),
            ),
            _ => FaceEnrollmentProcessError::ProcessStartFailed(format!(
                "failed to start face enrollment process: {error}"
            )),
        })?;
        if let Some(stdout) = child.stdout.take() {
            spawn_preview_event_forwarder(stdout, self.preview_event_sink.clone());
        }
        Ok(ChildFaceEnrollmentProcess { child })
    }
}

fn spawn_preview_event_forwarder<S>(stdout: impl std::io::Read + Send + 'static, sink: S)
where
    S: FaceEnrollmentPreviewEventSink,
{
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(event_json) = line.strip_prefix(PREVIEW_EVENT_PREFIX) {
                sink.emit_preview_frame(event_json);
            }
        }
    });
}

#[derive(Debug)]
pub struct ChildFaceEnrollmentProcess {
    child: Child,
}

impl FaceEnrollmentProcess for ChildFaceEnrollmentProcess {
    fn try_wait(&mut self) -> Result<Option<FaceEnrollmentExit>, FaceEnrollmentProcessError> {
        self.child
            .try_wait()
            .map(|status| {
                status.map(|status| FaceEnrollmentExit {
                    exit_success: status.success(),
                    exit_code: status.code(),
                })
            })
            .map_err(|error| {
                FaceEnrollmentProcessError::ProcessStatusFailed(format!(
                    "failed to query face enrollment process: {error}"
                ))
            })
    }

    fn cancel(&mut self) -> Result<(), FaceEnrollmentProcessError> {
        match self.child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => {
                self.child.kill().map_err(|error| {
                    FaceEnrollmentProcessError::ProcessCancelFailed(format!(
                        "failed to cancel face enrollment process: {error}"
                    ))
                })?;
                let _ = self.child.wait();
                Ok(())
            }
            Err(error) => Err(FaceEnrollmentProcessError::ProcessStatusFailed(format!(
                "failed to query face enrollment process before cancellation: {error}"
            ))),
        }
    }
}

struct CommandFaceEnrollmentSession {
    enrollment_session_id: String,
    start_payload: FaceEnrollmentStartPayload,
    output_dir: PathBuf,
    started_at: SystemTime,
    session_state: FaceEnrollmentSessionState,
    process: Option<Box<dyn FaceEnrollmentProcess>>,
    current_step: Option<String>,
    current_instruction_code: Option<String>,
    accepted_sample_count: u32,
    required_sample_count: Option<u32>,
    last_frame_result: Option<FaceEnrollmentFrameResult>,
    template_summary: Option<FaceTemplateEnrollmentSummary>,
    failure_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct RuntimeFaceEnrollmentStatus {
    session_state: FaceEnrollmentSessionState,
    current_step: Option<String>,
    current_instruction_code: Option<String>,
    accepted_sample_count: u32,
    required_sample_count: Option<u32>,
    last_frame_result: Option<FaceEnrollmentFrameResult>,
}

impl CommandFaceEnrollmentSession {
    fn running(
        enrollment_session_id: String,
        start_payload: FaceEnrollmentStartPayload,
        output_dir: PathBuf,
        process: impl FaceEnrollmentProcess,
    ) -> Self {
        Self {
            enrollment_session_id,
            start_payload,
            output_dir,
            started_at: SystemTime::now(),
            session_state: FaceEnrollmentSessionState::Running,
            process: Some(Box::new(process)),
            current_step: None,
            current_instruction_code: None,
            accepted_sample_count: 0,
            required_sample_count: None,
            last_frame_result: None,
            template_summary: None,
            failure_message: None,
        }
    }

    fn is_running(&self) -> bool {
        matches!(
            self.session_state,
            FaceEnrollmentSessionState::Starting
                | FaceEnrollmentSessionState::Running
                | FaceEnrollmentSessionState::WaitingForFace
                | FaceEnrollmentSessionState::WaitingForPose
                | FaceEnrollmentSessionState::Capturing
                | FaceEnrollmentSessionState::Finishing
        )
    }

    fn status(&self) -> FaceEnrollmentSessionStatus {
        FaceEnrollmentSessionStatus {
            enrollment_session_id: self.enrollment_session_id.clone(),
            session_state: self.session_state,
            user_id: self.start_payload.user_id.clone(),
            camera_id: self.start_payload.camera_id.clone(),
            current_step: self.current_step.clone(),
            current_instruction_code: self.current_instruction_code.clone(),
            accepted_sample_count: self.accepted_sample_count,
            required_sample_count: self.required_sample_count,
            last_frame_result: self.last_frame_result,
            template_summary: self.template_summary.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamedPipeFaceAuthServiceClient {
    pipe_name: String,
}

impl NamedPipeFaceAuthServiceClient {
    pub fn new(pipe_name: impl Into<String>) -> Self {
        Self {
            pipe_name: pipe_name.into(),
        }
    }
}

impl Default for NamedPipeFaceAuthServiceClient {
    fn default() -> Self {
        Self::new(PIPE_NAME)
    }
}

#[cfg(windows)]
impl FaceAuthServiceClient for NamedPipeFaceAuthServiceClient {
    fn send_service_request(&self, request: ServiceRequest) -> Result<ServiceEvent, ProtocolError> {
        let mut client = ipc::NamedPipeClient::new(&self.pipe_name);
        client.connect()?;
        let event = client.request(request)?;
        client.disconnect();
        Ok(event)
    }
}

#[cfg(not(windows))]
impl FaceAuthServiceClient for NamedPipeFaceAuthServiceClient {
    fn send_service_request(
        &self,
        _request: ServiceRequest,
    ) -> Result<ServiceEvent, ProtocolError> {
        Err(ProtocolError::TransportUnavailable)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailableFaceTemplateManagementStore;

impl FaceTemplateManagementStore for UnavailableFaceTemplateManagementStore {
    fn list_face_templates(&self) -> Result<FaceTemplateList, ControlBackendError> {
        Err(ControlBackendError::face_template_store_unavailable(
            "face template management is not connected to a runtime backend",
        ))
    }

    fn delete_face_template(
        &self,
        _payload: &DeleteFaceTemplatePayload,
    ) -> Result<DeleteFaceTemplateOutcome, ControlBackendError> {
        Err(ControlBackendError::face_template_delete_failed(
            "face template deletion is not connected to a runtime backend",
        ))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailableFaceEnrollmentRuntime;

#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailableCameraDiscoveryProvider;

impl CameraDiscoveryProvider for UnavailableCameraDiscoveryProvider {
    fn list_cameras(&self) -> Result<CameraDeviceList, ControlBackendError> {
        Err(ControlBackendError::camera_discovery_unavailable(
            "camera discovery runtime is not connected",
        ))
    }
}

impl FaceEnrollmentRuntime for UnavailableFaceEnrollmentRuntime {
    fn start_face_enrollment(
        &self,
        _payload: &FaceEnrollmentStartPayload,
    ) -> Result<FaceEnrollmentSessionStatus, ControlBackendError> {
        Err(ControlBackendError::face_enrollment_unavailable(
            "face enrollment runtime is not connected",
        ))
    }

    fn get_face_enrollment_status(
        &self,
        _payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentSessionStatus, ControlBackendError> {
        Err(ControlBackendError::face_enrollment_session_not_found(
            "face enrollment session was not found",
        ))
    }

    fn get_face_enrollment_preview(
        &self,
        _payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentPreviewFrame, ControlBackendError> {
        Err(ControlBackendError::face_enrollment_session_not_found(
            "face enrollment session was not found",
        ))
    }

    fn cancel_face_enrollment(
        &self,
        _payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentSessionStatus, ControlBackendError> {
        Err(ControlBackendError::face_enrollment_session_not_found(
            "face enrollment session was not found",
        ))
    }

    fn finish_face_enrollment(
        &self,
        _payload: &FaceEnrollmentSessionPayload,
    ) -> Result<FaceEnrollmentFinishOutcome, ControlBackendError> {
        Err(ControlBackendError::face_enrollment_session_not_found(
            "face enrollment session was not found",
        ))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailableFaceAuthSelfTestRunner;

impl FaceAuthSelfTestRunner for UnavailableFaceAuthSelfTestRunner {
    fn run_face_auth_self_test(
        &self,
        _payload: &FaceAuthSelfTestPayload,
    ) -> Result<FaceAuthSelfTestOutcome, ControlBackendError> {
        Err(ControlBackendError::auth_self_test_failed(
            "face auth self-test runner is not connected",
        ))
    }
}

#[derive(Eq, PartialEq)]
pub struct WindowsCredentialSecret {
    password: String,
}

impl WindowsCredentialSecret {
    pub fn from_password(password: String) -> Self {
        Self { password }
    }

    pub fn is_empty(&self) -> bool {
        self.password.is_empty()
    }

    pub fn into_password(self) -> String {
        self.password
    }
}

impl std::fmt::Debug for WindowsCredentialSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowsCredentialSecret")
            .field("password", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlBackendError {
    operation_status: ControlOperationStatus,
    control_error_code: ControlErrorCode,
    message: String,
    next_recommended_action: Option<String>,
}

impl ControlBackendError {
    fn new(
        operation_status: ControlOperationStatus,
        control_error_code: ControlErrorCode,
        message: impl Into<String>,
        next_recommended_action: impl Into<String>,
    ) -> Self {
        Self {
            operation_status,
            control_error_code,
            message: message.into(),
            next_recommended_action: Some(next_recommended_action.into()),
        }
    }

    pub fn dashboard_status_unavailable(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::ServiceUnavailable,
            control_error_code: ControlErrorCode::DashboardStatusUnavailable,
            message: message.into(),
            next_recommended_action: Some(
                "Check whether WinFaceUnlock service and configuration are available.".to_owned(),
            ),
        }
    }

    pub fn settings_unavailable(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::ServiceUnavailable,
            control_error_code: ControlErrorCode::SettingsUnavailable,
            message: message.into(),
            next_recommended_action: Some(
                "Check whether WinFaceUnlock service configuration is available.".to_owned(),
            ),
        }
    }

    pub fn settings_persistence_failed(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::Failed,
            control_error_code: ControlErrorCode::SettingsPersistenceFailed,
            message: message.into(),
            next_recommended_action: Some(
                "Retry after confirming the service registry configuration can be updated."
                    .to_owned(),
            ),
        }
    }

    pub fn credential_enrollment_unavailable(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::ServiceUnavailable,
            control_error_code: ControlErrorCode::CredentialEnrollmentUnavailable,
            message: message.into(),
            next_recommended_action: Some(
                "Check whether the WinFaceUnlock credential store can be opened.".to_owned(),
            ),
        }
    }

    pub fn credential_account_unavailable(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::ServiceUnavailable,
            control_error_code: ControlErrorCode::CredentialAccountUnavailable,
            message: message.into(),
            next_recommended_action: Some(
                "Check whether the current Windows account identity is available.".to_owned(),
            ),
        }
    }

    pub fn credential_enrollment_failed(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::Failed,
            control_error_code: ControlErrorCode::CredentialEnrollmentFailed,
            message: message.into(),
            next_recommended_action: Some(
                "Confirm the Windows credential and retry enrollment.".to_owned(),
            ),
        }
    }

    pub fn face_template_store_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::ServiceUnavailable,
            ControlErrorCode::FaceTemplateStoreUnavailable,
            message,
            "Check whether WinFaceUnlock service configuration and template storage are available.",
        )
    }

    pub fn face_template_config_missing(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::FaceTemplateConfigMissing,
            message,
            "Enroll or configure a face template before listing enrolled faces.",
        )
    }

    pub fn face_template_file_missing(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::FaceTemplateFileMissing,
            message,
            "Run face enrollment again or repair the configured face template path.",
        )
    }

    pub fn face_template_parse_failed(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::FaceTemplateParseFailed,
            message,
            "Regenerate the face template through guided enrollment.",
        )
    }

    pub fn face_template_empty(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::FaceTemplateEmpty,
            message,
            "Run face enrollment again so at least one unlock template is selected.",
        )
    }

    pub fn face_template_not_found(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::FaceTemplateNotFound,
            message,
            "Refresh face template management and retry with an existing template reference.",
        )
    }

    pub fn active_template_delete_blocked(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::ActiveTemplateDeleteBlocked,
            message,
            "Reconfigure service authentication before deleting the active face template.",
        )
    }

    pub fn face_template_delete_failed(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::FaceTemplateDeleteFailed,
            message,
            "Retry face template deletion after checking template storage permissions.",
        )
    }

    pub fn camera_discovery_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::ServiceUnavailable,
            ControlErrorCode::CameraUnavailable,
            message,
            "Check camera permissions and camera device availability.",
        )
    }

    pub fn face_enrollment_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::ServiceUnavailable,
            ControlErrorCode::FaceEnrollmentUnavailable,
            message,
            "Check camera access, model files, and the face enrollment runtime.",
        )
    }

    pub fn face_enrollment_already_running(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::FaceEnrollmentAlreadyRunning,
            message,
            "Finish or cancel the current face enrollment session before starting another.",
        )
    }

    pub fn face_enrollment_session_not_found(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::FaceEnrollmentSessionNotFound,
            message,
            "Start a new face enrollment session and retry.",
        )
    }

    pub fn camera_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::ServiceUnavailable,
            ControlErrorCode::CameraUnavailable,
            message,
            "Check camera permissions and camera device availability.",
        )
    }

    pub fn face_model_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::ServiceUnavailable,
            ControlErrorCode::FaceModelUnavailable,
            message,
            "Check whether WinFaceUnlock face model files are installed.",
        )
    }

    pub fn face_enrollment_failed(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::FaceEnrollmentFailed,
            message,
            "Retry guided face enrollment after checking camera positioning and model files.",
        )
    }

    pub fn face_enrollment_cancelled(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Cancelled,
            ControlErrorCode::FaceEnrollmentCancelled,
            message,
            "Start face enrollment again when ready.",
        )
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::ServiceUnavailable,
            ControlErrorCode::ServiceStatusUnavailable,
            message,
            "Check whether WinFaceUnlockService is installed and running.",
        )
    }

    pub fn auth_match_failed(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::AuthMatchFailed,
            message,
            "Check camera access, face positioning, enrolled templates, and matching threshold.",
        )
    }

    pub fn grant_issue_failed(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::GrantIssueFailed,
            message,
            "Retry authentication after checking service grant issuance state.",
        )
    }

    pub fn credential_missing(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::CredentialMissing,
            message,
            "Enroll the Windows credential before running face auth self-test.",
        )
    }

    pub fn credential_material_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::CredentialMaterialUnavailable,
            message,
            "Check credential store access and credential material protection.",
        )
    }

    pub fn auth_self_test_failed(message: impl Into<String>) -> Self {
        Self::new(
            ControlOperationStatus::Failed,
            ControlErrorCode::AuthSelfTestFailed,
            message,
            "Check that face templates, credentials, service auth, and camera access are ready.",
        )
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::PermissionDenied,
            control_error_code: ControlErrorCode::PermissionDenied,
            message: message.into(),
            next_recommended_action: Some(
                "Run the control frontend with sufficient local privileges.".to_owned(),
            ),
        }
    }

    pub fn requires_elevation(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::RequiresElevation,
            control_error_code: ControlErrorCode::ElevationRequired,
            message: message.into(),
            next_recommended_action: Some(
                "Retry from an elevated local control process or service-mediated settings path."
                    .to_owned(),
            ),
        }
    }

    fn status_reader_error(error: ControlStatusError) -> Self {
        match error {
            ControlStatusError::PermissionDenied(message) => Self::permission_denied(message),
            ControlStatusError::ElevationRequired(message) => Self::requires_elevation(message),
            ControlStatusError::ServiceStatusUnavailable(message) => Self {
                operation_status: ControlOperationStatus::ServiceUnavailable,
                control_error_code: ControlErrorCode::ServiceStatusUnavailable,
                message,
                next_recommended_action: Some(
                    "Check whether the Windows Service Control Manager is reachable.".to_owned(),
                ),
            },
            ControlStatusError::ProviderStatusUnavailable(message) => Self {
                operation_status: ControlOperationStatus::Failed,
                control_error_code: ControlErrorCode::ProviderStatusUnavailable,
                message,
                next_recommended_action: Some(
                    "Check WinFaceUnlock credential provider registry entries.".to_owned(),
                ),
            },
            ControlStatusError::ServiceConfigUnavailable(message) => Self {
                operation_status: ControlOperationStatus::Failed,
                control_error_code: ControlErrorCode::ServiceConfigUnavailable,
                message,
                next_recommended_action: Some(
                    "Check WinFaceUnlock service configuration registry entries.".to_owned(),
                ),
            },
            ControlStatusError::SettingsUnavailable(message) => Self::settings_unavailable(message),
            ControlStatusError::SettingsPersistenceFailed(message) => {
                Self::settings_persistence_failed(message)
            }
            ControlStatusError::DataDirectoryStatusUnavailable(message) => Self {
                operation_status: ControlOperationStatus::Failed,
                control_error_code: ControlErrorCode::DataDirectoryStatusUnavailable,
                message,
                next_recommended_action: Some(
                    "Check WinFaceUnlock ProgramData directory access.".to_owned(),
                ),
            },
            ControlStatusError::PresenceRuntimeStatusUnavailable(message) => Self {
                operation_status: ControlOperationStatus::Failed,
                control_error_code: ControlErrorCode::PresenceRuntimeStatusUnavailable,
                message,
                next_recommended_action: Some(
                    "Check the WinFaceUnlock presence runtime status file.".to_owned(),
                ),
            },
        }
    }

    fn face_template_status_error(error: FaceTemplateStatusError) -> Self {
        match error {
            FaceTemplateStatusError::ServiceConfigUnavailable(message) => {
                Self::face_template_store_unavailable(message)
            }
            FaceTemplateStatusError::TemplateConfigMissing(message) => {
                Self::face_template_config_missing(message)
            }
            FaceTemplateStatusError::TemplateFileMissing(message) => {
                Self::face_template_file_missing(message)
            }
            FaceTemplateStatusError::TemplateParseFailed(message) => {
                Self::face_template_parse_failed(message)
            }
            FaceTemplateStatusError::TemplateEmpty(message) => Self::face_template_empty(message),
            FaceTemplateStatusError::PermissionDenied(message) => Self::permission_denied(message),
        }
    }

    fn into_response(self, request: &ControlRequestEnvelope) -> ControlResponseEnvelope {
        ControlResponseEnvelope {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: self.operation_status,
            message: self.message,
            safe_details: json!({
                "control_error_code": self.control_error_code,
            }),
            next_recommended_action: self.next_recommended_action,
        }
    }
}

impl DashboardStatusProvider for WindowsDashboardStatusProvider {
    fn load_dashboard_status(&self) -> Result<DashboardStatus, ControlBackendError> {
        self.load_dashboard_status()
            .map_err(ControlBackendError::status_reader_error)
    }
}

impl ControlSettingsStore for WindowsControlSettingsStore {
    fn load_settings(&self) -> Result<ControlSettingsSnapshot, ControlBackendError> {
        self.load_settings()
            .map_err(ControlBackendError::status_reader_error)
    }

    fn update_settings(
        &self,
        patch: &ControlSettingsPatch,
    ) -> Result<ControlSettingsSnapshot, ControlBackendError> {
        self.update_settings(patch)
            .map_err(ControlBackendError::status_reader_error)
    }
}

impl FaceTemplateManagementStore for WindowsFaceTemplateStatusStore {
    fn list_face_templates(&self) -> Result<FaceTemplateList, ControlBackendError> {
        self.load_face_templates()
            .map_err(ControlBackendError::face_template_status_error)
    }

    fn delete_face_template(
        &self,
        payload: &DeleteFaceTemplatePayload,
    ) -> Result<DeleteFaceTemplateOutcome, ControlBackendError> {
        if payload.face_template_ref == ACTIVE_SERVICE_FACE_TEMPLATE_REF {
            Err(ControlBackendError::active_template_delete_blocked(
                "active service face template deletion is not enabled until service auth reconfiguration is defined",
            ))
        } else {
            Err(ControlBackendError::face_template_not_found(format!(
                "face template {} was not found",
                payload.face_template_ref
            )))
        }
    }
}

pub struct ControlHandler<
    D,
    S,
    C,
    F = UnavailableFaceTemplateManagementStore,
    E = UnavailableFaceEnrollmentRuntime,
    G = UnavailableCameraDiscoveryProvider,
    A = UnavailableFaceAuthSelfTestRunner,
> {
    dashboard_status_provider: D,
    settings_store: S,
    credential_enrollment_store: C,
    face_template_store: F,
    face_enrollment_runtime: E,
    camera_discovery_provider: G,
    face_auth_self_test_runner: A,
}

impl<D, S, C> ControlHandler<D, S, C>
where
    D: DashboardStatusProvider,
    S: ControlSettingsStore,
    C: WindowsCredentialEnrollmentStore,
{
    pub fn new(
        dashboard_status_provider: D,
        settings_store: S,
        credential_enrollment_store: C,
    ) -> Self {
        Self {
            dashboard_status_provider,
            settings_store,
            credential_enrollment_store,
            face_template_store: UnavailableFaceTemplateManagementStore,
            face_enrollment_runtime: UnavailableFaceEnrollmentRuntime,
            camera_discovery_provider: UnavailableCameraDiscoveryProvider,
            face_auth_self_test_runner: UnavailableFaceAuthSelfTestRunner,
        }
    }
}

impl<D, S, C, F, E, G, A> ControlHandler<D, S, C, F, E, G, A>
where
    D: DashboardStatusProvider,
    S: ControlSettingsStore,
    C: WindowsCredentialEnrollmentStore,
    F: FaceTemplateManagementStore,
    E: FaceEnrollmentRuntime,
    G: CameraDiscoveryProvider,
    A: FaceAuthSelfTestRunner,
{
    pub fn with_face_dependencies(
        dashboard_status_provider: D,
        settings_store: S,
        credential_enrollment_store: C,
        face_template_store: F,
        face_enrollment_runtime: E,
        camera_discovery_provider: G,
        face_auth_self_test_runner: A,
    ) -> Self {
        Self {
            dashboard_status_provider,
            settings_store,
            credential_enrollment_store,
            face_template_store,
            face_enrollment_runtime,
            camera_discovery_provider,
            face_auth_self_test_runner,
        }
    }

    pub fn handle_request(&self, request: ControlRequestEnvelope) -> ControlResponseEnvelope {
        if request.protocol_version != CONTROL_PROTOCOL_VERSION {
            return ControlResponseEnvelope::unsupported_protocol(&request);
        }

        match request.operation {
            ControlOperation::GetDashboardStatus => self.handle_get_dashboard_status(&request),
            ControlOperation::GetSettings => self.handle_get_settings(&request),
            ControlOperation::UpdateSettings => self.handle_update_settings(&request),
            ControlOperation::GetWindowsCredentialAccount => {
                self.handle_get_windows_credential_account(&request)
            }
            ControlOperation::EnrollWindowsCredential => ControlResponseEnvelope::invalid_request(
                &request,
                "enroll_windows_credential requires a credential secret side channel.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidCredentialEnrollmentRequest,
                }),
            ),
            ControlOperation::ListFaceTemplates => self.handle_list_face_templates(&request),
            ControlOperation::DeleteFaceTemplate => self.handle_delete_face_template(&request),
            ControlOperation::ListCameras => self.handle_list_cameras(&request),
            ControlOperation::StartFaceEnrollment => self.handle_start_face_enrollment(&request),
            ControlOperation::GetFaceEnrollmentStatus => {
                self.handle_get_face_enrollment_status(&request)
            }
            ControlOperation::GetFaceEnrollmentPreview => {
                self.handle_get_face_enrollment_preview(&request)
            }
            ControlOperation::CancelFaceEnrollment => self.handle_cancel_face_enrollment(&request),
            ControlOperation::FinishFaceEnrollment => self.handle_finish_face_enrollment(&request),
            ControlOperation::RunFaceAuthSelfTest => self.handle_run_face_auth_self_test(&request),
        }
    }

    fn handle_list_face_templates(
        &self,
        request: &ControlRequestEnvelope,
    ) -> ControlResponseEnvelope {
        if !payload_is_empty(&request.payload) {
            return ControlResponseEnvelope::invalid_request(
                request,
                "list_face_templates does not accept a payload.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidFaceTemplateRequest,
                }),
            );
        }

        match self.face_template_store.list_face_templates() {
            Ok(list) => {
                ControlResponseEnvelope::completed(request, "Face templates loaded.", json!(list))
            }
            Err(error) => error.into_response(request),
        }
    }

    fn handle_delete_face_template(
        &self,
        request: &ControlRequestEnvelope,
    ) -> ControlResponseEnvelope {
        let payload =
            match serde_json::from_value::<DeleteFaceTemplatePayload>(request.payload.clone()) {
                Ok(payload) if payload.has_valid_ref() => payload,
                Ok(_) => {
                    return ControlResponseEnvelope::invalid_request(
                        request,
                        "delete_face_template requires a non-empty face_template_ref.",
                        json!({
                            "control_error_code": ControlErrorCode::InvalidFaceTemplateRequest,
                        }),
                    );
                }
                Err(error) => {
                    return ControlResponseEnvelope::invalid_request(
                        request,
                        format!("delete_face_template payload is invalid: {error}"),
                        json!({
                            "control_error_code": ControlErrorCode::InvalidFaceTemplateRequest,
                        }),
                    );
                }
            };

        match self.face_template_store.delete_face_template(&payload) {
            Ok(outcome) => ControlResponseEnvelope::completed(
                request,
                "Face template deleted.",
                json!(outcome),
            ),
            Err(error) => error.into_response(request),
        }
    }

    fn handle_list_cameras(&self, request: &ControlRequestEnvelope) -> ControlResponseEnvelope {
        if !payload_is_empty(&request.payload) {
            return ControlResponseEnvelope::invalid_request(
                request,
                "list_cameras does not accept a payload.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidFaceEnrollmentRequest,
                }),
            );
        }

        match self.camera_discovery_provider.list_cameras() {
            Ok(list) => ControlResponseEnvelope::completed(request, "Cameras loaded.", json!(list)),
            Err(error) => error.into_response(request),
        }
    }

    fn handle_start_face_enrollment(
        &self,
        request: &ControlRequestEnvelope,
    ) -> ControlResponseEnvelope {
        let payload =
            match serde_json::from_value::<FaceEnrollmentStartPayload>(request.payload.clone()) {
                Ok(payload) if payload.has_valid_fields() => payload,
                Ok(_) => {
                    return ControlResponseEnvelope::invalid_request(
                        request,
                        "start_face_enrollment requires valid user_id and camera_id fields.",
                        json!({
                            "control_error_code": ControlErrorCode::InvalidFaceEnrollmentRequest,
                        }),
                    );
                }
                Err(error) => {
                    return ControlResponseEnvelope::invalid_request(
                        request,
                        format!("start_face_enrollment payload is invalid: {error}"),
                        json!({
                            "control_error_code": ControlErrorCode::InvalidFaceEnrollmentRequest,
                        }),
                    );
                }
            };

        match self.face_enrollment_runtime.start_face_enrollment(&payload) {
            Ok(status) => ControlResponseEnvelope::completed(
                request,
                "Face enrollment started.",
                json!(status),
            ),
            Err(error) => error.into_response(request),
        }
    }

    fn handle_get_face_enrollment_status(
        &self,
        request: &ControlRequestEnvelope,
    ) -> ControlResponseEnvelope {
        let payload = match enrollment_session_payload(request) {
            Ok(payload) => payload,
            Err(response) => return response,
        };

        match self
            .face_enrollment_runtime
            .get_face_enrollment_status(&payload)
        {
            Ok(status) => ControlResponseEnvelope::completed(
                request,
                "Face enrollment status loaded.",
                json!(status),
            ),
            Err(error) => error.into_response(request),
        }
    }

    fn handle_get_face_enrollment_preview(
        &self,
        request: &ControlRequestEnvelope,
    ) -> ControlResponseEnvelope {
        let payload = match enrollment_session_payload(request) {
            Ok(payload) => payload,
            Err(response) => return response,
        };

        match self
            .face_enrollment_runtime
            .get_face_enrollment_preview(&payload)
        {
            Ok(preview) => ControlResponseEnvelope::completed(
                request,
                "Face enrollment preview loaded.",
                json!(preview),
            ),
            Err(error) => error.into_response(request),
        }
    }

    fn handle_cancel_face_enrollment(
        &self,
        request: &ControlRequestEnvelope,
    ) -> ControlResponseEnvelope {
        let payload = match enrollment_session_payload(request) {
            Ok(payload) => payload,
            Err(response) => return response,
        };

        match self
            .face_enrollment_runtime
            .cancel_face_enrollment(&payload)
        {
            Ok(status) => ControlResponseEnvelope::completed(
                request,
                "Face enrollment cancelled.",
                json!(status),
            ),
            Err(error) => error.into_response(request),
        }
    }

    fn handle_finish_face_enrollment(
        &self,
        request: &ControlRequestEnvelope,
    ) -> ControlResponseEnvelope {
        let payload = match enrollment_session_payload(request) {
            Ok(payload) => payload,
            Err(response) => return response,
        };

        match self
            .face_enrollment_runtime
            .finish_face_enrollment(&payload)
        {
            Ok(outcome) => ControlResponseEnvelope::completed(
                request,
                "Face enrollment finished.",
                json!(outcome),
            ),
            Err(error) => error.into_response(request),
        }
    }

    fn handle_run_face_auth_self_test(
        &self,
        request: &ControlRequestEnvelope,
    ) -> ControlResponseEnvelope {
        let payload =
            match serde_json::from_value::<FaceAuthSelfTestPayload>(request.payload.clone()) {
                Ok(payload) if payload.has_valid_fields() => payload,
                Ok(_) => {
                    return ControlResponseEnvelope::invalid_request(
                        request,
                        "run_face_auth_self_test requires valid session_id and camera_id fields.",
                        json!({
                            "control_error_code": ControlErrorCode::InvalidAuthSelfTestRequest,
                        }),
                    );
                }
                Err(error) => {
                    return ControlResponseEnvelope::invalid_request(
                        request,
                        format!("run_face_auth_self_test payload is invalid: {error}"),
                        json!({
                            "control_error_code": ControlErrorCode::InvalidAuthSelfTestRequest,
                        }),
                    );
                }
            };

        match self
            .face_auth_self_test_runner
            .run_face_auth_self_test(&payload)
        {
            Ok(outcome) => ControlResponseEnvelope::completed(
                request,
                "Face auth self-test completed.",
                json!(outcome),
            ),
            Err(error) => error.into_response(request),
        }
    }

    fn handle_get_windows_credential_account(
        &self,
        request: &ControlRequestEnvelope,
    ) -> ControlResponseEnvelope {
        if !payload_is_empty(&request.payload) {
            return ControlResponseEnvelope::invalid_request(
                request,
                "get_windows_credential_account does not accept a payload.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidCredentialAccountRequest,
                }),
            );
        }

        match self
            .credential_enrollment_store
            .load_windows_credential_account()
        {
            Ok(profile) => ControlResponseEnvelope::completed(
                request,
                "Windows credential account loaded.",
                json!(profile),
            ),
            Err(error) => error.into_response(request),
        }
    }

    pub fn handle_windows_credential_enrollment_request(
        &self,
        request: ControlRequestEnvelope,
        password_secret: WindowsCredentialSecret,
    ) -> ControlResponseEnvelope {
        if request.protocol_version != CONTROL_PROTOCOL_VERSION {
            return ControlResponseEnvelope::unsupported_protocol(&request);
        }

        if request.operation != ControlOperation::EnrollWindowsCredential {
            return ControlResponseEnvelope::invalid_request(
                &request,
                "credential enrollment requests must use enroll_windows_credential.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidCredentialEnrollmentRequest,
                }),
            );
        }

        let payload = match serde_json::from_value::<WindowsCredentialEnrollmentPayload>(
            request.payload.clone(),
        ) {
            Ok(payload) if payload.has_valid_safe_fields() => payload,
            Ok(_) => {
                return ControlResponseEnvelope::invalid_request(
                    &request,
                    "credential enrollment payload contains invalid account fields.",
                    json!({
                        "control_error_code": ControlErrorCode::InvalidCredentialEnrollmentRequest,
                    }),
                );
            }
            Err(error) => {
                return ControlResponseEnvelope::invalid_request(
                    &request,
                    format!("credential enrollment payload is invalid: {error}"),
                    json!({
                        "control_error_code": ControlErrorCode::InvalidCredentialEnrollmentRequest,
                    }),
                );
            }
        };

        if password_secret.is_empty() {
            return ControlResponseEnvelope::invalid_request(
                &request,
                "credential enrollment requires a non-empty credential secret.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidCredentialEnrollmentRequest,
                }),
            );
        }

        match self
            .credential_enrollment_store
            .enroll_windows_credential(&payload, password_secret)
        {
            Ok(outcome) => ControlResponseEnvelope::completed(
                &request,
                "Windows credential enrolled.",
                json!(outcome),
            ),
            Err(error) => error.into_response(&request),
        }
    }

    fn handle_get_dashboard_status(
        &self,
        request: &ControlRequestEnvelope,
    ) -> ControlResponseEnvelope {
        if !payload_is_empty(&request.payload) {
            return ControlResponseEnvelope::invalid_request(
                request,
                "get_dashboard_status does not accept a payload.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidDashboardStatusRequest,
                }),
            );
        }

        match self.dashboard_status_provider.load_dashboard_status() {
            Ok(status) => ControlResponseEnvelope::completed(
                request,
                "Dashboard status loaded.",
                json!(status),
            ),
            Err(error) => error.into_response(request),
        }
    }

    fn handle_get_settings(&self, request: &ControlRequestEnvelope) -> ControlResponseEnvelope {
        if !payload_is_empty(&request.payload) {
            return ControlResponseEnvelope::invalid_request(
                request,
                "get_settings does not accept a payload.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidSettingsRequest,
                }),
            );
        }

        match self.settings_store.load_settings() {
            Ok(settings) => {
                ControlResponseEnvelope::completed(request, "Settings loaded.", json!(settings))
            }
            Err(error) => error.into_response(request),
        }
    }

    fn handle_update_settings(&self, request: &ControlRequestEnvelope) -> ControlResponseEnvelope {
        let patch = match serde_json::from_value::<ControlSettingsPatch>(request.payload.clone()) {
            Ok(patch) if patch.has_updates() => patch,
            Ok(_) => {
                return ControlResponseEnvelope::invalid_request(
                    request,
                    "update_settings requires at least one settings field.",
                    json!({
                        "control_error_code": ControlErrorCode::InvalidSettingsRequest,
                    }),
                );
            }
            Err(error) => {
                return ControlResponseEnvelope::invalid_request(
                    request,
                    format!("update_settings payload is invalid: {error}"),
                    json!({
                        "control_error_code": ControlErrorCode::InvalidSettingsRequest,
                    }),
                );
            }
        };

        match self.settings_store.update_settings(&patch) {
            Ok(settings) => {
                ControlResponseEnvelope::completed(request, "Settings updated.", json!(settings))
            }
            Err(error) => error.into_response(request),
        }
    }
}

fn payload_is_empty(payload: &Value) -> bool {
    match payload {
        Value::Null => true,
        Value::Object(object) => object.is_empty(),
        _ => false,
    }
}

fn parse_diagnostics_camera_list(output: &str) -> Result<CameraDeviceList, String> {
    let cameras = output
        .lines()
        .filter_map(|line| line.strip_prefix("camera: "))
        .filter_map(|camera| {
            let (camera_id, display_name) = camera.split_once(' ')?;
            let camera_id = camera_id.trim();
            if camera_id.is_empty() {
                return None;
            }
            Some(CameraDeviceSummary {
                camera_id: camera_id.to_owned(),
                display_name: display_name.trim().to_owned(),
            })
        })
        .collect();

    Ok(CameraDeviceList { cameras })
}

fn enrollment_session_payload(
    request: &ControlRequestEnvelope,
) -> Result<FaceEnrollmentSessionPayload, ControlResponseEnvelope> {
    match serde_json::from_value::<FaceEnrollmentSessionPayload>(request.payload.clone()) {
        Ok(payload) if payload.has_valid_session_id() => Ok(payload),
        Ok(_) => Err(ControlResponseEnvelope::invalid_request(
            request,
            "face enrollment session operation requires a non-empty enrollment_session_id.",
            json!({
                "control_error_code": ControlErrorCode::InvalidFaceEnrollmentRequest,
            }),
        )),
        Err(error) => Err(ControlResponseEnvelope::invalid_request(
            request,
            format!("face enrollment session payload is invalid: {error}"),
            json!({
                "control_error_code": ControlErrorCode::InvalidFaceEnrollmentRequest,
            }),
        )),
    }
}

fn lock_enrollment_sessions(
    sessions: &Arc<Mutex<HashMap<String, CommandFaceEnrollmentSession>>>,
) -> Result<MutexGuard<'_, HashMap<String, CommandFaceEnrollmentSession>>, ControlBackendError> {
    sessions.lock().map_err(|_| {
        ControlBackendError::face_enrollment_unavailable(
            "face enrollment session state is unavailable",
        )
    })
}

fn refresh_command_face_enrollment_session(
    session: &mut CommandFaceEnrollmentSession,
) -> Result<(), ControlBackendError> {
    if !session.is_running() {
        return Ok(());
    }

    apply_runtime_face_enrollment_status(session);

    if session.session_state == FaceEnrollmentSessionState::Starting
        && session
            .started_at
            .elapsed()
            .map(|elapsed| elapsed > FACE_ENROLLMENT_STARTUP_TIMEOUT)
            .unwrap_or(false)
    {
        if let Some(process) = session.process.as_mut() {
            process
                .cancel()
                .map_err(face_enrollment_process_error_to_control_error)?;
        }
        session.process = None;
        session.session_state = FaceEnrollmentSessionState::Failed;
        session.failure_message = Some(format!(
            "camera startup did not produce a readable frame within {} seconds",
            FACE_ENROLLMENT_STARTUP_TIMEOUT.as_secs()
        ));
        return Ok(());
    }

    let Some(process) = session.process.as_mut() else {
        return Ok(());
    };

    let Some(exit) = process
        .try_wait()
        .map_err(face_enrollment_process_error_to_control_error)?
    else {
        return Ok(());
    };

    session.process = None;
    if exit.exit_success {
        session.session_state = FaceEnrollmentSessionState::Finishing;
        match read_enrollment_template_summary(session) {
            Ok(summary) => {
                session.template_summary = Some(summary);
                session.session_state = FaceEnrollmentSessionState::Completed;
                session.failure_message = None;
            }
            Err(error) => {
                session.session_state = FaceEnrollmentSessionState::Failed;
                session.failure_message = Some(error.message.clone());
                return Err(error);
            }
        }
    } else {
        session.session_state = FaceEnrollmentSessionState::Failed;
        let code = exit
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_owned());
        session.failure_message = Some(format!(
            "face enrollment command failed with exit code {code}"
        ));
    }

    Ok(())
}

fn apply_runtime_face_enrollment_status(session: &mut CommandFaceEnrollmentSession) {
    let Some(runtime_status) = read_runtime_face_enrollment_status(&session.output_dir) else {
        return;
    };

    if matches!(
        runtime_status.session_state,
        FaceEnrollmentSessionState::Starting
            | FaceEnrollmentSessionState::Running
            | FaceEnrollmentSessionState::WaitingForFace
            | FaceEnrollmentSessionState::WaitingForPose
            | FaceEnrollmentSessionState::Capturing
            | FaceEnrollmentSessionState::Finishing
    ) {
        session.session_state = runtime_status.session_state;
    }
    session.current_step = runtime_status.current_step;
    session.current_instruction_code = runtime_status.current_instruction_code;
    session.accepted_sample_count = runtime_status.accepted_sample_count;
    session.required_sample_count = runtime_status.required_sample_count;
    session.last_frame_result = runtime_status.last_frame_result;
}

fn read_runtime_face_enrollment_status(output_dir: &Path) -> Option<RuntimeFaceEnrollmentStatus> {
    let status_path = output_dir.join(ENROLLMENT_STATUS_FILE_NAME);
    let status_bytes = fs::read(status_path).ok()?;
    serde_json::from_slice(&status_bytes).ok()
}

fn read_enrollment_preview_frame(
    session: &CommandFaceEnrollmentSession,
) -> FaceEnrollmentPreviewFrame {
    let preview_frame_path = session.output_dir.join(ENROLLMENT_PREVIEW_FRAME_FILE_NAME);
    let Ok(image_bytes) = fs::read(&preview_frame_path) else {
        return FaceEnrollmentPreviewFrame {
            enrollment_session_id: session.enrollment_session_id.clone(),
            preview_available: false,
            mime_type: None,
            image_base64: None,
            frame_updated_at_unix_ms: None,
        };
    };

    FaceEnrollmentPreviewFrame {
        enrollment_session_id: session.enrollment_session_id.clone(),
        preview_available: true,
        mime_type: Some("image/jpeg".to_owned()),
        image_base64: Some(BASE64_STANDARD.encode(image_bytes)),
        frame_updated_at_unix_ms: preview_frame_updated_at_unix_ms(&preview_frame_path),
    }
}

fn preview_frame_updated_at_unix_ms(preview_frame_path: &Path) -> Option<i64> {
    fs::metadata(preview_frame_path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
}

fn read_enrollment_template_summary(
    session: &CommandFaceEnrollmentSession,
) -> Result<FaceTemplateEnrollmentSummary, ControlBackendError> {
    let template_path = session.output_dir.join(SELECTED_TEMPLATES_FILE_NAME);
    let summary = summarize_selected_face_template_file(
        &template_path,
        enrollment_face_template_ref(&session.enrollment_session_id),
        FaceTemplateSourceState::RepositoryTemplate,
    )
    .map_err(face_enrollment_template_status_error_to_control_error)?;

    Ok(FaceTemplateEnrollmentSummary {
        selected_template_count: summary.selected_template_count,
        rejected_sample_count: summary.rejected_sample_count.unwrap_or(0),
    })
}

fn enrollment_face_template_ref(enrollment_session_id: &str) -> String {
    format!("face-enrollment-template-{enrollment_session_id}")
}

fn next_face_enrollment_session_id(
    sessions: &HashMap<String, CommandFaceEnrollmentSession>,
) -> String {
    let base = format!("face-enrollment-{}", current_unix_ms());
    if !sessions.contains_key(&base) {
        return base;
    }

    for suffix in 1_u32..=u32::MAX {
        let candidate = format!("{base}-{suffix}");
        if !sessions.contains_key(&candidate) {
            return candidate;
        }
    }

    format!("face-enrollment-{}-fallback", current_unix_ms())
}

fn face_enrollment_template_status_error_to_control_error(
    error: FaceTemplateStatusError,
) -> ControlBackendError {
    match error {
        FaceTemplateStatusError::TemplateFileMissing(message) => {
            ControlBackendError::face_enrollment_failed(message)
        }
        FaceTemplateStatusError::TemplateParseFailed(message) => {
            ControlBackendError::face_enrollment_failed(message)
        }
        FaceTemplateStatusError::TemplateEmpty(message) => {
            ControlBackendError::face_enrollment_failed(message)
        }
        FaceTemplateStatusError::PermissionDenied(message) => {
            ControlBackendError::permission_denied(message)
        }
        FaceTemplateStatusError::ServiceConfigUnavailable(message)
        | FaceTemplateStatusError::TemplateConfigMissing(message) => {
            ControlBackendError::face_enrollment_failed(message)
        }
    }
}

fn face_enrollment_process_error_to_control_error(
    error: FaceEnrollmentProcessError,
) -> ControlBackendError {
    match error {
        FaceEnrollmentProcessError::MissingDiagnosticsCli(message) => {
            ControlBackendError::face_enrollment_unavailable(message)
        }
        FaceEnrollmentProcessError::OutputDirectoryUnavailable(message) => {
            ControlBackendError::face_enrollment_unavailable(message)
        }
        FaceEnrollmentProcessError::CameraUnavailable(message) => {
            ControlBackendError::camera_unavailable(message)
        }
        FaceEnrollmentProcessError::ModelUnavailable(message) => {
            ControlBackendError::face_model_unavailable(message)
        }
        FaceEnrollmentProcessError::ProcessStartFailed(message)
        | FaceEnrollmentProcessError::ProcessStatusFailed(message)
        | FaceEnrollmentProcessError::TemplateFileMissing(message)
        | FaceEnrollmentProcessError::TemplateParseFailed(message)
        | FaceEnrollmentProcessError::TemplateEmpty(message) => {
            ControlBackendError::face_enrollment_failed(message)
        }
        FaceEnrollmentProcessError::ProcessCancelFailed(message) => {
            ControlBackendError::face_enrollment_failed(message)
        }
        FaceEnrollmentProcessError::PermissionDenied(message) => {
            ControlBackendError::permission_denied(message)
        }
    }
}

fn current_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn sanitize_session_path_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "face-enrollment-session".to_owned()
    } else {
        sanitized
    }
}

#[cfg(windows)]
fn default_install_dir() -> PathBuf {
    PathBuf::from(r"C:\WinFaceUnlock")
}

#[cfg(not(windows))]
fn default_install_dir() -> PathBuf {
    std::env::temp_dir().join("WinFaceUnlock")
}

fn auth_wake_protocol_error(error: ProtocolError) -> ControlBackendError {
    match error {
        ProtocolError::TransportUnavailable => {
            ControlBackendError::service_unavailable("service auth IPC is unavailable")
        }
        ProtocolError::Unauthorized => {
            ControlBackendError::grant_issue_failed("service rejected wake-auth authorization")
        }
        ProtocolError::InvalidMessage
        | ProtocolError::ExpiredGrant
        | ProtocolError::UsedGrant
        | ProtocolError::SessionMismatch => ControlBackendError::auth_self_test_failed(format!(
            "service wake-auth request failed: {error:?}"
        )),
    }
}

fn credential_material_protocol_error(error: ProtocolError) -> ControlBackendError {
    match error {
        ProtocolError::TransportUnavailable => ControlBackendError::service_unavailable(
            "service credential material IPC is unavailable",
        ),
        ProtocolError::Unauthorized => ControlBackendError::credential_missing(
            "service could not resolve enrolled credential material",
        ),
        ProtocolError::InvalidMessage => ControlBackendError::credential_material_unavailable(
            "service returned invalid credential material state",
        ),
        ProtocolError::ExpiredGrant | ProtocolError::UsedGrant | ProtocolError::SessionMismatch => {
            ControlBackendError::grant_issue_failed(format!(
                "service rejected credential material grant: {error:?}"
            ))
        }
    }
}

fn face_template_apply_protocol_error(error: ProtocolError) -> ControlBackendError {
    match error {
        ProtocolError::TransportUnavailable => ControlBackendError::service_unavailable(
            "service face template configuration IPC is unavailable",
        ),
        ProtocolError::Unauthorized => ControlBackendError::permission_denied(
            "service rejected face template configuration authorization",
        ),
        ProtocolError::InvalidMessage => ControlBackendError::face_enrollment_failed(
            "service rejected face template configuration payload",
        ),
        ProtocolError::ExpiredGrant | ProtocolError::UsedGrant | ProtocolError::SessionMismatch => {
            ControlBackendError::face_enrollment_failed(format!(
                "service rejected face template configuration request: {error:?}"
            ))
        }
    }
}

fn auth_failure_reason_to_control_error(reason: AuthFailureReason) -> ControlBackendError {
    match reason {
        AuthFailureReason::NoFaceDetected
        | AuthFailureReason::MultipleFacesDetected
        | AuthFailureReason::MatchBelowThreshold
        | AuthFailureReason::TemplateModelMismatch
        | AuthFailureReason::LivenessFailed
        | AuthFailureReason::CooldownActive
        | AuthFailureReason::Timeout => ControlBackendError::auth_match_failed(format!(
            "face authentication failed: {reason:?}"
        )),
        AuthFailureReason::Cancelled => {
            ControlBackendError::auth_self_test_failed("face authentication was cancelled")
        }
        AuthFailureReason::InternalError => {
            ControlBackendError::auth_self_test_failed("service reported an internal auth error")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::VecDeque,
        fs,
        path::{Path, PathBuf},
        rc::Rc,
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    use common_protocol::{
        AuthGrant, AuthScore, CredentialMaterialProtection, GrantId, Nonce,
        ProtectedCredentialMaterial, UserId,
    };
    use control_protocol::{
        DataDirectorySummary, FaceRecognitionModelSummary, FaceTemplateKind,
        FaceTemplateSourceState, FaceTemplateSummary, LogonWakeMode, PathPresence,
        PresenceMonitorState, PresenceRuntimeSummary, ProviderRegistrationState,
        ProviderStatusSummary, RegistryConfigState, ServiceConfigSummary, ServiceInstallationState,
        ServiceRuntimeState, ServiceStatusSummary, WindowsCredentialAccountType,
    };

    use super::*;

    #[derive(Clone)]
    struct FixedDashboardStatusProvider {
        result: Result<DashboardStatus, ControlBackendError>,
    }

    impl DashboardStatusProvider for FixedDashboardStatusProvider {
        fn load_dashboard_status(&self) -> Result<DashboardStatus, ControlBackendError> {
            self.result.clone()
        }
    }

    #[derive(Clone)]
    struct RecordingSettingsStore {
        current: Rc<RefCell<Result<ControlSettingsSnapshot, ControlBackendError>>>,
        last_patch: Rc<RefCell<Option<ControlSettingsPatch>>>,
    }

    impl ControlSettingsStore for RecordingSettingsStore {
        fn load_settings(&self) -> Result<ControlSettingsSnapshot, ControlBackendError> {
            self.current.borrow().clone()
        }

        fn update_settings(
            &self,
            patch: &ControlSettingsPatch,
        ) -> Result<ControlSettingsSnapshot, ControlBackendError> {
            self.last_patch.replace(Some(patch.clone()));
            let mut current = self.current.borrow_mut();
            let settings = match current.as_mut() {
                Ok(settings) => settings,
                Err(error) => return Err(error.clone()),
            };
            if let Some(enabled) = patch.presence_lock_enabled {
                settings.presence_lock_enabled = enabled;
            }
            if let Some(logon_wake_mode) = patch.logon_wake_mode {
                settings.logon_wake_mode = Some(logon_wake_mode);
            }
            Ok(settings.clone())
        }
    }

    fn settings_store_fixture() -> RecordingSettingsStore {
        RecordingSettingsStore {
            current: Rc::new(RefCell::new(Ok(ControlSettingsSnapshot {
                presence_lock_enabled: true,
                logon_wake_mode: Some(LogonWakeMode::InputTriggered),
            }))),
            last_patch: Rc::new(RefCell::new(None)),
        }
    }

    #[derive(Clone)]
    struct RecordingCredentialEnrollmentStore {
        account_profile: Result<WindowsCredentialAccountProfile, ControlBackendError>,
        result: Result<WindowsCredentialEnrollmentOutcome, ControlBackendError>,
        last_payload: Rc<RefCell<Option<WindowsCredentialEnrollmentPayload>>>,
        last_password: Rc<RefCell<Option<String>>>,
    }

    impl WindowsCredentialEnrollmentStore for RecordingCredentialEnrollmentStore {
        fn load_windows_credential_account(
            &self,
        ) -> Result<WindowsCredentialAccountProfile, ControlBackendError> {
            self.account_profile.clone()
        }

        fn enroll_windows_credential(
            &self,
            payload: &WindowsCredentialEnrollmentPayload,
            password_secret: WindowsCredentialSecret,
        ) -> Result<WindowsCredentialEnrollmentOutcome, ControlBackendError> {
            self.last_payload.replace(Some(payload.clone()));
            self.last_password
                .replace(Some(password_secret.into_password()));
            self.result.clone()
        }
    }

    fn credential_enrollment_store_fixture() -> RecordingCredentialEnrollmentStore {
        RecordingCredentialEnrollmentStore {
            account_profile: Ok(WindowsCredentialAccountProfile {
                windows_account_username: "Leo16".to_owned(),
                user_id: "dev-user".to_owned(),
                user_sid: "S-1-5-21-real".to_owned(),
                account_type: WindowsCredentialAccountType::Local,
                credential_ref: "windows-credential-dev-user".to_owned(),
            }),
            result: Ok(WindowsCredentialEnrollmentOutcome {
                windows_account_username: "Leo16".to_owned(),
                user_id: "dev-user".to_owned(),
                user_sid: "S-1-5-21-winfaceunlock-pending".to_owned(),
                account_type: WindowsCredentialAccountType::Local,
                credential_ref: "windows-credential-dev-user".to_owned(),
            }),
            last_payload: Rc::new(RefCell::new(None)),
            last_password: Rc::new(RefCell::new(None)),
        }
    }

    #[derive(Clone)]
    struct RecordingFaceTemplateManagementStore {
        list_result: Result<FaceTemplateList, ControlBackendError>,
        delete_result: Result<DeleteFaceTemplateOutcome, ControlBackendError>,
        last_delete_payload: Rc<RefCell<Option<DeleteFaceTemplatePayload>>>,
    }

    impl FaceTemplateManagementStore for RecordingFaceTemplateManagementStore {
        fn list_face_templates(&self) -> Result<FaceTemplateList, ControlBackendError> {
            self.list_result.clone()
        }

        fn delete_face_template(
            &self,
            payload: &DeleteFaceTemplatePayload,
        ) -> Result<DeleteFaceTemplateOutcome, ControlBackendError> {
            self.last_delete_payload.replace(Some(payload.clone()));
            self.delete_result.clone()
        }
    }

    fn face_template_store_fixture() -> RecordingFaceTemplateManagementStore {
        RecordingFaceTemplateManagementStore {
            list_result: Ok(FaceTemplateList {
                templates: vec![FaceTemplateSummary {
                    face_template_ref: "active-service-template".to_owned(),
                    user_id: "dev-user".to_owned(),
                    display_name: Some("Leo16".to_owned()),
                    template_kind: FaceTemplateKind::SelectedTemplateSet,
                    recognition_model: FaceRecognitionModelSummary {
                        model_family: "opencv_sface".to_owned(),
                        model_version: "2021dec".to_owned(),
                    },
                    selected_template_count: 5,
                    rejected_sample_count: Some(1),
                    created_at_unix_ms: Some(1_782_000_000_000),
                    updated_at_unix_ms: Some(1_782_000_000_500),
                    source_state: FaceTemplateSourceState::ActiveServiceTemplate,
                }],
            }),
            delete_result: Ok(DeleteFaceTemplateOutcome {
                face_template_ref: "active-service-template".to_owned(),
                template_deleted: true,
                service_auth_requires_reconfiguration: true,
            }),
            last_delete_payload: Rc::new(RefCell::new(None)),
        }
    }

    #[derive(Clone)]
    struct RecordingFaceAuthServiceClient {
        responses: Rc<RefCell<VecDeque<Result<ServiceEvent, ProtocolError>>>>,
        requests: Rc<RefCell<Vec<ServiceRequest>>>,
    }

    impl FaceAuthServiceClient for RecordingFaceAuthServiceClient {
        fn send_service_request(
            &self,
            request: ServiceRequest,
        ) -> Result<ServiceEvent, ProtocolError> {
            self.requests.borrow_mut().push(request);
            self.responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(ProtocolError::InvalidMessage))
        }
    }

    fn face_auth_service_client_fixture(
        responses: Vec<Result<ServiceEvent, ProtocolError>>,
    ) -> RecordingFaceAuthServiceClient {
        RecordingFaceAuthServiceClient {
            responses: Rc::new(RefCell::new(VecDeque::from(responses))),
            requests: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn request(operation: ControlOperation, payload: Value) -> ControlRequestEnvelope {
        ControlRequestEnvelope {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: "control-c1-test".to_owned(),
            operation,
            payload,
        }
    }

    fn dashboard_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::GetDashboardStatus, payload)
    }

    fn settings_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::GetSettings, payload)
    }

    fn update_settings_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::UpdateSettings, payload)
    }

    fn credential_account_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::GetWindowsCredentialAccount, payload)
    }

    fn credential_enrollment_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::EnrollWindowsCredential, payload)
    }

    fn face_template_list_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::ListFaceTemplates, payload)
    }

    fn delete_face_template_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::DeleteFaceTemplate, payload)
    }

    fn start_face_enrollment_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::StartFaceEnrollment, payload)
    }

    fn get_face_enrollment_status_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::GetFaceEnrollmentStatus, payload)
    }

    fn get_face_enrollment_preview_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::GetFaceEnrollmentPreview, payload)
    }

    fn cancel_face_enrollment_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::CancelFaceEnrollment, payload)
    }

    fn finish_face_enrollment_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::FinishFaceEnrollment, payload)
    }

    fn face_auth_self_test_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::RunFaceAuthSelfTest, payload)
    }

    fn list_cameras_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::ListCameras, payload)
    }

    fn handler_with_face_template_store(
        face_template_store: RecordingFaceTemplateManagementStore,
    ) -> ControlHandler<
        FixedDashboardStatusProvider,
        RecordingSettingsStore,
        RecordingCredentialEnrollmentStore,
        RecordingFaceTemplateManagementStore,
        UnavailableFaceEnrollmentRuntime,
        UnavailableCameraDiscoveryProvider,
        UnavailableFaceAuthSelfTestRunner,
    > {
        ControlHandler::with_face_dependencies(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
            face_template_store,
            UnavailableFaceEnrollmentRuntime,
            UnavailableCameraDiscoveryProvider,
            UnavailableFaceAuthSelfTestRunner,
        )
    }

    fn handler_with_face_auth_service_client(
        service_client: RecordingFaceAuthServiceClient,
    ) -> ControlHandler<
        FixedDashboardStatusProvider,
        RecordingSettingsStore,
        RecordingCredentialEnrollmentStore,
        RecordingFaceTemplateManagementStore,
        UnavailableFaceEnrollmentRuntime,
        UnavailableCameraDiscoveryProvider,
        ServiceFaceAuthSelfTestRunner<RecordingFaceAuthServiceClient>,
    > {
        ControlHandler::with_face_dependencies(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
            face_template_store_fixture(),
            UnavailableFaceEnrollmentRuntime,
            UnavailableCameraDiscoveryProvider,
            ServiceFaceAuthSelfTestRunner::new(service_client),
        )
    }

    fn handler_with_face_enrollment_runtime(
        face_enrollment_runtime: CommandFaceEnrollmentRuntime<
            RecordingFaceEnrollmentProcessFactory,
            RecordingFaceEnrollmentTemplateApplier,
        >,
    ) -> ControlHandler<
        FixedDashboardStatusProvider,
        RecordingSettingsStore,
        RecordingCredentialEnrollmentStore,
        RecordingFaceTemplateManagementStore,
        CommandFaceEnrollmentRuntime<
            RecordingFaceEnrollmentProcessFactory,
            RecordingFaceEnrollmentTemplateApplier,
        >,
        UnavailableCameraDiscoveryProvider,
        UnavailableFaceAuthSelfTestRunner,
    > {
        ControlHandler::with_face_dependencies(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
            face_template_store_fixture(),
            face_enrollment_runtime,
            UnavailableCameraDiscoveryProvider,
            UnavailableFaceAuthSelfTestRunner,
        )
    }

    fn handler_with_camera_discovery_provider(
        camera_discovery_provider: FixedCameraDiscoveryProvider,
    ) -> ControlHandler<
        FixedDashboardStatusProvider,
        RecordingSettingsStore,
        RecordingCredentialEnrollmentStore,
        RecordingFaceTemplateManagementStore,
        UnavailableFaceEnrollmentRuntime,
        FixedCameraDiscoveryProvider,
        UnavailableFaceAuthSelfTestRunner,
    > {
        ControlHandler::with_face_dependencies(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
            face_template_store_fixture(),
            UnavailableFaceEnrollmentRuntime,
            camera_discovery_provider,
            UnavailableFaceAuthSelfTestRunner,
        )
    }

    #[derive(Clone)]
    struct RecordingFaceEnrollmentTemplateApplier {
        applied_template_paths: Arc<Mutex<Vec<PathBuf>>>,
        result: Result<(), ControlBackendError>,
    }

    impl RecordingFaceEnrollmentTemplateApplier {
        fn successful() -> Self {
            Self {
                applied_template_paths: Arc::new(Mutex::new(Vec::new())),
                result: Ok(()),
            }
        }

        fn failed(error: ControlBackendError) -> Self {
            Self {
                applied_template_paths: Arc::new(Mutex::new(Vec::new())),
                result: Err(error),
            }
        }

        fn applied_template_paths(&self) -> Result<Vec<PathBuf>, String> {
            self.applied_template_paths
                .lock()
                .map(|paths| paths.clone())
                .map_err(|_| "recording face enrollment template applier lock poisoned".to_owned())
        }
    }

    impl FaceEnrollmentTemplateApplier for RecordingFaceEnrollmentTemplateApplier {
        fn apply_face_enrollment_template(
            &self,
            template_path: &Path,
        ) -> Result<(), ControlBackendError> {
            self.applied_template_paths
                .lock()
                .map_err(|_| {
                    ControlBackendError::face_enrollment_failed(
                        "recording face enrollment template applier lock poisoned",
                    )
                })?
                .push(template_path.to_path_buf());
            self.result.clone()
        }
    }

    #[derive(Clone)]
    struct FixedCameraDiscoveryProvider {
        result: Result<CameraDeviceList, ControlBackendError>,
    }

    impl FixedCameraDiscoveryProvider {
        fn available(cameras: Vec<CameraDeviceSummary>) -> Self {
            Self {
                result: Ok(CameraDeviceList { cameras }),
            }
        }
    }

    impl CameraDiscoveryProvider for FixedCameraDiscoveryProvider {
        fn list_cameras(&self) -> Result<CameraDeviceList, ControlBackendError> {
            self.result.clone()
        }
    }

    #[derive(Clone)]
    struct RecordingFaceEnrollmentProcessFactory {
        state: Arc<Mutex<RecordingFaceEnrollmentProcessFactoryState>>,
    }

    struct RecordingFaceEnrollmentProcessFactoryState {
        root_dir: PathBuf,
        template_json_on_start: Option<String>,
        exit_sequences: VecDeque<VecDeque<Option<FaceEnrollmentExit>>>,
        starts: Vec<RecordingFaceEnrollmentStart>,
        cancelled_session_ids: Vec<String>,
        start_error: Option<FaceEnrollmentProcessError>,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct RecordingFaceEnrollmentStart {
        session_id: String,
        user_id: String,
        camera_id: String,
        output_dir: PathBuf,
    }

    impl RecordingFaceEnrollmentProcessFactory {
        fn new(
            root_dir: PathBuf,
            exit_sequences: Vec<Vec<Option<FaceEnrollmentExit>>>,
            template_json_on_start: Option<String>,
        ) -> Self {
            Self {
                state: Arc::new(Mutex::new(RecordingFaceEnrollmentProcessFactoryState {
                    root_dir,
                    template_json_on_start,
                    exit_sequences: exit_sequences
                        .into_iter()
                        .map(VecDeque::from)
                        .collect::<VecDeque<_>>(),
                    starts: Vec::new(),
                    cancelled_session_ids: Vec::new(),
                    start_error: None,
                })),
            }
        }

        fn with_start_error(root_dir: PathBuf, start_error: FaceEnrollmentProcessError) -> Self {
            let factory = Self::new(root_dir, Vec::new(), None);
            if let Ok(mut state) = factory.state.lock() {
                state.start_error = Some(start_error);
            }
            factory
        }

        fn starts(&self) -> Result<Vec<RecordingFaceEnrollmentStart>, String> {
            self.state
                .lock()
                .map(|state| state.starts.clone())
                .map_err(|_| "recording face enrollment factory lock poisoned".to_owned())
        }

        fn cancelled_session_ids(&self) -> Result<Vec<String>, String> {
            self.state
                .lock()
                .map(|state| state.cancelled_session_ids.clone())
                .map_err(|_| "recording face enrollment factory lock poisoned".to_owned())
        }
    }

    impl FaceEnrollmentProcessFactory for RecordingFaceEnrollmentProcessFactory {
        type Process = RecordingFaceEnrollmentProcess;

        fn prepare_output_dir(
            &self,
            session_id: &str,
        ) -> Result<PathBuf, FaceEnrollmentProcessError> {
            let root_dir = self
                .state
                .lock()
                .map_err(|_| {
                    FaceEnrollmentProcessError::OutputDirectoryUnavailable(
                        "recording face enrollment factory lock poisoned".to_owned(),
                    )
                })?
                .root_dir
                .clone();
            let output_dir = root_dir.join(sanitize_session_path_segment(session_id));
            fs::create_dir_all(&output_dir).map_err(|error| {
                FaceEnrollmentProcessError::OutputDirectoryUnavailable(format!(
                    "test output directory unavailable: {error}"
                ))
            })?;
            Ok(output_dir)
        }

        fn start_guided_enrollment(
            &self,
            session_id: &str,
            payload: &FaceEnrollmentStartPayload,
            output_dir: &Path,
        ) -> Result<Self::Process, FaceEnrollmentProcessError> {
            let mut state = self.state.lock().map_err(|_| {
                FaceEnrollmentProcessError::ProcessStartFailed(
                    "recording face enrollment factory lock poisoned".to_owned(),
                )
            })?;
            if let Some(error) = state.start_error.clone() {
                return Err(error);
            }
            state.starts.push(RecordingFaceEnrollmentStart {
                session_id: session_id.to_owned(),
                user_id: payload.user_id.clone(),
                camera_id: payload.camera_id.clone(),
                output_dir: output_dir.to_path_buf(),
            });
            if let Some(template_json) = state.template_json_on_start.as_deref() {
                fs::write(output_dir.join(SELECTED_TEMPLATES_FILE_NAME), template_json).map_err(
                    |error| {
                        FaceEnrollmentProcessError::ProcessStartFailed(format!(
                            "test template write failed: {error}"
                        ))
                    },
                )?;
            }
            let exit_sequence = state.exit_sequences.pop_front().unwrap_or_default();
            Ok(RecordingFaceEnrollmentProcess {
                session_id: session_id.to_owned(),
                exit_sequence,
                state: self.state.clone(),
            })
        }
    }

    struct RecordingFaceEnrollmentProcess {
        session_id: String,
        exit_sequence: VecDeque<Option<FaceEnrollmentExit>>,
        state: Arc<Mutex<RecordingFaceEnrollmentProcessFactoryState>>,
    }

    impl FaceEnrollmentProcess for RecordingFaceEnrollmentProcess {
        fn try_wait(&mut self) -> Result<Option<FaceEnrollmentExit>, FaceEnrollmentProcessError> {
            Ok(self.exit_sequence.pop_front().flatten())
        }

        fn cancel(&mut self) -> Result<(), FaceEnrollmentProcessError> {
            self.state
                .lock()
                .map_err(|_| {
                    FaceEnrollmentProcessError::ProcessCancelFailed(
                        "recording face enrollment factory lock poisoned".to_owned(),
                    )
                })?
                .cancelled_session_ids
                .push(self.session_id.clone());
            Ok(())
        }
    }

    fn completed_face_template_json() -> String {
        r#"{
            "user_id": "dev-user",
            "recognizer_model_family": "opencv_sface",
            "recognizer_model_version": "2021dec",
            "quality_summary": {
                "selected_template_count": 2,
                "rejected_sample_count": 1
            },
            "templates": [
                {
                    "selected_for_unlock": true,
                    "embedding": {
                        "values": [0.1, 0.2, 0.3]
                    }
                },
                {
                    "selected_for_unlock": true,
                    "embedding": {
                        "values": [0.3, 0.2, 0.1]
                    }
                }
            ]
        }"#
        .to_owned()
    }

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{prefix}-{}-{suffix}", std::process::id()))
    }

    fn auth_grant_fixture(session_id: &str) -> AuthGrant {
        AuthGrant {
            grant_id: GrantId("grant-1".to_owned()),
            nonce: Nonce("nonce-1".to_owned()),
            session_id: SessionId(session_id.to_owned()),
            user_id: UserId("dev-user".to_owned()),
            source: AuthSource::LocalCamera,
            score: AuthScore {
                match_score: 0.82,
                liveness_score: None,
            },
            issued_at_unix_ms: 1_000,
            expires_at_unix_ms: 6_000,
        }
    }

    fn credential_material_fixture() -> ProtectedCredentialMaterial {
        ProtectedCredentialMaterial {
            user_id: UserId("dev-user".to_owned()),
            domain: "LIU".to_owned(),
            username: "Leo16".to_owned(),
            protected_password: vec![1, 2, 3, 4],
            protection: CredentialMaterialProtection::DpapiLocalMachineV1,
        }
    }

    fn dashboard_status_fixture() -> DashboardStatus {
        DashboardStatus {
            service: ServiceStatusSummary {
                installation_state: ServiceInstallationState::Installed,
                runtime_state: ServiceRuntimeState::Running,
                process_id: Some(100),
            },
            provider: ProviderStatusSummary {
                registration_state: ProviderRegistrationState::PartiallyRegistered,
                credential_provider_registered: true,
                com_server_registered: false,
                project_config_registered: true,
            },
            service_config: ServiceConfigSummary {
                registry_config_state: RegistryConfigState::Present,
                auth_mode: Some("local_camera".to_owned()),
                face_template_path: None,
                presence_lock_enabled: Some(true),
                presence_detector_kind: Some("person".to_owned()),
                presence_tracking_mode: Some("owner_face".to_owned()),
            },
            data_directory: DataDirectorySummary {
                program_data_dir: Some("C:\\ProgramData\\WinFaceUnlock".to_owned()),
                program_data_presence: PathPresence::Present,
                presence_audit_dir: None,
                presence_audit_presence: PathPresence::Unknown,
            },
            presence_runtime: Some(PresenceRuntimeSummary {
                monitor_state: PresenceMonitorState::Running,
                session_id: Some(1),
                reason: Some("monitor_running".to_owned()),
                updated_at_unix_ms: Some(1_782_000_000_000),
            }),
        }
    }

    #[test]
    fn get_dashboard_status_returns_completed_response() -> Result<(), serde_json::Error> {
        let expected_status = dashboard_status_fixture();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(expected_status.clone()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(dashboard_request(json!({})));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Dashboard status loaded.");
        let decoded_status: DashboardStatus = serde_json::from_value(response.safe_details)?;
        assert_eq!(decoded_status, expected_status);
        Ok(())
    }

    #[test]
    fn get_dashboard_status_rejects_non_empty_payload() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(dashboard_request(json!({
            "unexpected": true,
        })));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_dashboard_status_request"
        );
    }

    #[test]
    fn unsupported_protocol_returns_version_details() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );
        let mut request = dashboard_request(json!({}));
        request.protocol_version = CONTROL_PROTOCOL_VERSION + 1;

        let response = handler.handle_request(request);

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::UnsupportedProtocol
        );
        assert_eq!(
            response.safe_details["supported_protocol_version"],
            CONTROL_PROTOCOL_VERSION
        );
    }

    #[test]
    fn provider_error_maps_to_semantic_response() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Err(ControlBackendError::dashboard_status_unavailable(
                    "service manager is unavailable",
                )),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(dashboard_request(json!({})));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::ServiceUnavailable
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "dashboard_status_unavailable"
        );
        assert!(
            response
                .next_recommended_action
                .as_deref()
                .is_some_and(|action| action.contains("service"))
        );
    }

    #[test]
    fn permission_error_maps_to_permission_denied_response() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Err(ControlBackendError::permission_denied(
                    "registry access denied",
                )),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(dashboard_request(Value::Null));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::PermissionDenied
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "permission_denied"
        );
    }

    #[test]
    fn get_settings_returns_completed_response() -> Result<(), serde_json::Error> {
        let settings_store = settings_store_fixture();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store,
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(settings_request(json!({})));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Settings loaded.");
        let decoded_settings: ControlSettingsSnapshot =
            serde_json::from_value(response.safe_details)?;
        assert!(decoded_settings.presence_lock_enabled);
        assert_eq!(
            decoded_settings.logon_wake_mode,
            Some(LogonWakeMode::InputTriggered)
        );
        Ok(())
    }

    #[test]
    fn get_windows_credential_account_returns_current_profile() -> Result<(), serde_json::Error> {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(credential_account_request(json!({})));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Windows credential account loaded.");
        let decoded_profile: WindowsCredentialAccountProfile =
            serde_json::from_value(response.safe_details)?;
        assert_eq!(decoded_profile.windows_account_username, "Leo16");
        assert_eq!(decoded_profile.user_id, "dev-user");
        assert_eq!(decoded_profile.user_sid, "S-1-5-21-real");
        assert_eq!(
            decoded_profile.credential_ref,
            "windows-credential-dev-user"
        );
        Ok(())
    }

    #[test]
    fn get_windows_credential_account_rejects_payload() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(credential_account_request(json!({
            "unexpected": true,
        })));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_credential_account_request"
        );
    }

    #[test]
    fn list_face_templates_returns_backend_template_summaries() -> Result<(), serde_json::Error> {
        let handler = handler_with_face_template_store(face_template_store_fixture());

        let response = handler.handle_request(face_template_list_request(json!({})));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Face templates loaded.");
        let decoded_list: FaceTemplateList = serde_json::from_value(response.safe_details)?;
        let summary = decoded_list
            .templates
            .first()
            .ok_or_else(|| serde_json::Error::io(std::io::Error::other("missing summary")))?;
        assert_eq!(summary.face_template_ref, "active-service-template");
        assert_eq!(summary.user_id, "dev-user");
        assert_eq!(summary.selected_template_count, 5);
        assert_eq!(
            summary.source_state,
            FaceTemplateSourceState::ActiveServiceTemplate
        );
        Ok(())
    }

    #[test]
    fn list_face_templates_rejects_payload() {
        let handler = handler_with_face_template_store(face_template_store_fixture());

        let response = handler.handle_request(face_template_list_request(json!({
            "unexpected": true,
        })));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_face_template_request"
        );
    }

    #[test]
    fn parse_diagnostics_camera_list_reads_camera_lines() -> Result<(), String> {
        let list = parse_diagnostics_camera_list(
            "camera_count: 2\ncamera: opencv-index:0 Local camera 0\ncamera: opencv-index:1 USB camera\n",
        )?;

        assert_eq!(list.cameras.len(), 2);
        assert_eq!(list.cameras[0].camera_id, "opencv-index:0");
        assert_eq!(list.cameras[0].display_name, "Local camera 0");
        assert_eq!(list.cameras[1].camera_id, "opencv-index:1");
        assert_eq!(list.cameras[1].display_name, "USB camera");
        Ok(())
    }

    #[test]
    fn diagnostics_factory_resolves_model_paths_from_cli_ancestors()
    -> Result<(), Box<dyn std::error::Error>> {
        let root_dir = unique_test_dir("winfaceunlock-control-model-paths");
        let cli_dir = root_dir.join("target").join("debug");
        let models_dir = root_dir.join("models");
        fs::create_dir_all(&cli_dir)?;
        fs::create_dir_all(&models_dir)?;
        let diagnostics_cli_path = cli_dir.join(DIAGNOSTICS_CLI_FILE_NAME);
        let yunet_model_path = models_dir.join("face_detection_yunet_2023mar.onnx");
        fs::write(&diagnostics_cli_path, b"fake exe")?;
        fs::write(&yunet_model_path, b"fake model")?;

        let factory = DiagnosticsCliEnrollmentProcessFactory::new(
            diagnostics_cli_path,
            root_dir.join("face-enrollment"),
        );

        assert_eq!(
            factory.resolve_model_path(ENV_YUNET_MODEL_PATH, DEFAULT_YUNET_MODEL_PATH),
            yunet_model_path
        );
        let _ = fs::remove_dir_all(root_dir);
        Ok(())
    }

    #[test]
    fn list_cameras_returns_structured_camera_list() -> Result<(), serde_json::Error> {
        let handler =
            handler_with_camera_discovery_provider(FixedCameraDiscoveryProvider::available(vec![
                CameraDeviceSummary {
                    camera_id: "opencv-index:1".to_owned(),
                    display_name: "USB camera".to_owned(),
                },
            ]));

        let response = handler.handle_request(list_cameras_request(json!({})));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        let list: CameraDeviceList = serde_json::from_value(response.safe_details)?;
        assert_eq!(list.cameras.len(), 1);
        assert_eq!(list.cameras[0].camera_id, "opencv-index:1");
        Ok(())
    }

    #[test]
    fn delete_face_template_validates_ref_and_calls_store() {
        let face_store = face_template_store_fixture();
        let last_delete_payload = face_store.last_delete_payload.clone();
        let handler = handler_with_face_template_store(face_store);

        let response = handler.handle_request(delete_face_template_request(json!({
            "face_template_ref": "active-service-template",
        })));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(
            last_delete_payload
                .borrow()
                .as_ref()
                .map(|payload| payload.face_template_ref.as_str()),
            Some("active-service-template")
        );
    }

    #[test]
    fn delete_face_template_rejects_empty_ref() {
        let handler = handler_with_face_template_store(face_template_store_fixture());

        let response = handler.handle_request(delete_face_template_request(json!({
            "face_template_ref": "",
        })));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_face_template_request"
        );
    }

    #[test]
    fn start_face_enrollment_uses_semantic_unavailable_error_until_runtime_is_connected() {
        let handler = handler_with_face_template_store(face_template_store_fixture());

        let response = handler.handle_request(start_face_enrollment_request(json!({
            "user_id": "dev-user",
            "camera_id": "opencv-index:0",
        })));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::ServiceUnavailable
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "face_enrollment_unavailable"
        );
    }

    #[test]
    fn start_face_enrollment_starts_backend_session_without_exposing_process_details()
    -> Result<(), Box<dyn std::error::Error>> {
        let root_dir = unique_test_dir("winfaceunlock-control-enrollment-start");
        let factory =
            RecordingFaceEnrollmentProcessFactory::new(root_dir.clone(), vec![vec![None]], None);
        let starts = factory.clone();
        let handler = handler_with_face_enrollment_runtime(
            CommandFaceEnrollmentRuntime::with_template_applier(
                factory,
                RecordingFaceEnrollmentTemplateApplier::successful(),
            ),
        );

        let response = handler.handle_request(start_face_enrollment_request(json!({
            "user_id": "dev-user",
            "camera_id": "opencv-index:0",
        })));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Face enrollment started.");
        let status: FaceEnrollmentSessionStatus =
            serde_json::from_value(response.safe_details.clone())?;
        assert_eq!(status.session_state, FaceEnrollmentSessionState::Running);
        assert_eq!(status.user_id, "dev-user");
        assert_eq!(status.camera_id, "opencv-index:0");
        let recorded_starts = starts.starts()?;
        assert_eq!(recorded_starts.len(), 1);
        assert_eq!(recorded_starts[0].user_id, "dev-user");
        assert_eq!(recorded_starts[0].camera_id, "opencv-index:0");
        let response_text = serde_json::to_string(&response)?;
        assert!(!response_text.contains("diagnostics_cli"));
        assert!(!response_text.contains("selected_templates"));
        assert!(!response_text.contains("embedding"));
        let _ = fs::remove_dir_all(root_dir);
        Ok(())
    }

    #[test]
    fn face_enrollment_status_reads_runtime_progress_file() -> Result<(), Box<dyn std::error::Error>>
    {
        let root_dir = unique_test_dir("winfaceunlock-control-enrollment-progress");
        let factory =
            RecordingFaceEnrollmentProcessFactory::new(root_dir.clone(), vec![vec![None]], None);
        let starts = factory.clone();
        let handler = handler_with_face_enrollment_runtime(
            CommandFaceEnrollmentRuntime::with_template_applier(
                factory,
                RecordingFaceEnrollmentTemplateApplier::successful(),
            ),
        );

        let start = handler.handle_request(start_face_enrollment_request(json!({
            "user_id": "dev-user",
            "camera_id": "opencv-index:0",
        })));
        let start_status: FaceEnrollmentSessionStatus = serde_json::from_value(start.safe_details)?;
        let recorded_starts = starts.starts()?;
        fs::write(
            recorded_starts[0]
                .output_dir
                .join(ENROLLMENT_STATUS_FILE_NAME),
            serde_json::to_vec(&json!({
                "session_state": "waiting_for_pose",
                "current_step": "yaw_left_mild",
                "current_instruction_code": "turn_head_left",
                "accepted_sample_count": 2,
                "required_sample_count": 3,
                "last_frame_result": "pose_not_ready",
            }))?,
        )?;

        let response = handler.handle_request(get_face_enrollment_status_request(json!({
            "enrollment_session_id": start_status.enrollment_session_id,
        })));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        let status: FaceEnrollmentSessionStatus = serde_json::from_value(response.safe_details)?;
        assert_eq!(
            status.session_state,
            FaceEnrollmentSessionState::WaitingForPose
        );
        assert_eq!(status.current_step.as_deref(), Some("yaw_left_mild"));
        assert_eq!(
            status.current_instruction_code.as_deref(),
            Some("turn_head_left")
        );
        assert_eq!(status.accepted_sample_count, 2);
        assert_eq!(status.required_sample_count, Some(3));
        assert_eq!(
            status.last_frame_result,
            Some(FaceEnrollmentFrameResult::PoseNotReady)
        );
        let _ = fs::remove_dir_all(root_dir);
        Ok(())
    }

    #[test]
    fn face_enrollment_preview_returns_latest_backend_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        let root_dir = unique_test_dir("winfaceunlock-control-enrollment-preview");
        let factory =
            RecordingFaceEnrollmentProcessFactory::new(root_dir.clone(), vec![vec![None]], None);
        let starts = factory.clone();
        let handler = handler_with_face_enrollment_runtime(
            CommandFaceEnrollmentRuntime::with_template_applier(
                factory,
                RecordingFaceEnrollmentTemplateApplier::successful(),
            ),
        );

        let start = handler.handle_request(start_face_enrollment_request(json!({
            "user_id": "dev-user",
            "camera_id": "opencv-index:0",
        })));
        let start_status: FaceEnrollmentSessionStatus = serde_json::from_value(start.safe_details)?;
        let recorded_starts = starts.starts()?;
        fs::write(
            recorded_starts[0]
                .output_dir
                .join(ENROLLMENT_PREVIEW_FRAME_FILE_NAME),
            [1_u8, 2, 3],
        )?;

        let response = handler.handle_request(get_face_enrollment_preview_request(json!({
            "enrollment_session_id": start_status.enrollment_session_id,
        })));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        let preview: FaceEnrollmentPreviewFrame = serde_json::from_value(response.safe_details)?;
        assert!(preview.preview_available);
        assert_eq!(preview.mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(preview.image_base64.as_deref(), Some("AQID"));
        assert!(preview.frame_updated_at_unix_ms.is_some());
        let _ = fs::remove_dir_all(root_dir);
        Ok(())
    }

    #[test]
    fn start_face_enrollment_blocks_second_running_session()
    -> Result<(), Box<dyn std::error::Error>> {
        let root_dir = unique_test_dir("winfaceunlock-control-enrollment-single-flight");
        let factory = RecordingFaceEnrollmentProcessFactory::new(
            root_dir.clone(),
            vec![vec![None], vec![None]],
            None,
        );
        let handler = handler_with_face_enrollment_runtime(
            CommandFaceEnrollmentRuntime::with_template_applier(
                factory,
                RecordingFaceEnrollmentTemplateApplier::successful(),
            ),
        );

        let first = handler.handle_request(start_face_enrollment_request(json!({
            "user_id": "dev-user",
            "camera_id": "opencv-index:0",
        })));
        let second = handler.handle_request(start_face_enrollment_request(json!({
            "user_id": "dev-user",
            "camera_id": "opencv-index:0",
        })));

        assert_eq!(first.operation_status, ControlOperationStatus::Completed);
        assert_eq!(second.operation_status, ControlOperationStatus::Failed);
        assert_eq!(
            second.safe_details["control_error_code"],
            "face_enrollment_already_running"
        );
        let _ = fs::remove_dir_all(root_dir);
        Ok(())
    }

    #[test]
    fn face_enrollment_status_and_finish_return_completed_safe_summary()
    -> Result<(), Box<dyn std::error::Error>> {
        let root_dir = unique_test_dir("winfaceunlock-control-enrollment-finish");
        let factory = RecordingFaceEnrollmentProcessFactory::new(
            root_dir.clone(),
            vec![vec![Some(FaceEnrollmentExit {
                exit_success: true,
                exit_code: Some(0),
            })]],
            Some(completed_face_template_json()),
        );
        let template_applier = RecordingFaceEnrollmentTemplateApplier::successful();
        let applied_template_paths = template_applier.clone();
        let handler = handler_with_face_enrollment_runtime(
            CommandFaceEnrollmentRuntime::with_template_applier(factory, template_applier),
        );

        let start = handler.handle_request(start_face_enrollment_request(json!({
            "user_id": "dev-user",
            "camera_id": "opencv-index:0",
        })));
        let start_status: FaceEnrollmentSessionStatus = serde_json::from_value(start.safe_details)?;
        let status = handler.handle_request(get_face_enrollment_status_request(json!({
            "enrollment_session_id": start_status.enrollment_session_id,
        })));

        assert_eq!(status.operation_status, ControlOperationStatus::Completed);
        let completed_status: FaceEnrollmentSessionStatus =
            serde_json::from_value(status.safe_details.clone())?;
        assert_eq!(
            completed_status.session_state,
            FaceEnrollmentSessionState::Completed
        );
        assert_eq!(
            completed_status
                .template_summary
                .as_ref()
                .map(|summary| summary.selected_template_count),
            Some(2)
        );

        let finish = handler.handle_request(finish_face_enrollment_request(json!({
            "enrollment_session_id": completed_status.enrollment_session_id,
        })));

        assert_eq!(finish.operation_status, ControlOperationStatus::Completed);
        let outcome: FaceEnrollmentFinishOutcome =
            serde_json::from_value(finish.safe_details.clone())?;
        assert_eq!(outcome.session_state, FaceEnrollmentSessionState::Completed);
        assert_eq!(outcome.user_id, "dev-user");
        assert_eq!(outcome.template_summary.selected_template_count, 2);
        assert_eq!(outcome.template_summary.rejected_sample_count, 1);
        assert!(outcome.service_auth_configured);
        assert!(
            outcome
                .face_template_ref
                .starts_with("face-enrollment-template-")
        );
        let applied_paths = applied_template_paths.applied_template_paths()?;
        assert_eq!(applied_paths.len(), 1);
        assert_eq!(
            applied_paths[0].file_name().and_then(|name| name.to_str()),
            Some(SELECTED_TEMPLATES_FILE_NAME)
        );
        let response_text = serde_json::to_string(&finish)?;
        assert!(!response_text.contains("embedding"));
        assert!(!response_text.contains("selected_templates"));
        assert!(!response_text.contains("password"));
        let _ = fs::remove_dir_all(root_dir);
        Ok(())
    }

    #[test]
    fn face_enrollment_finish_fails_when_service_template_cannot_be_configured()
    -> Result<(), Box<dyn std::error::Error>> {
        let root_dir = unique_test_dir("winfaceunlock-control-enrollment-apply-failure");
        let factory = RecordingFaceEnrollmentProcessFactory::new(
            root_dir.clone(),
            vec![vec![Some(FaceEnrollmentExit {
                exit_success: true,
                exit_code: Some(0),
            })]],
            Some(completed_face_template_json()),
        );
        let template_applier = RecordingFaceEnrollmentTemplateApplier::failed(
            ControlBackendError::requires_elevation(
                "service face template configuration update requires elevation",
            ),
        );
        let handler = handler_with_face_enrollment_runtime(
            CommandFaceEnrollmentRuntime::with_template_applier(factory, template_applier),
        );

        let start = handler.handle_request(start_face_enrollment_request(json!({
            "user_id": "dev-user",
            "camera_id": "opencv-index:0",
        })));
        let start_status: FaceEnrollmentSessionStatus = serde_json::from_value(start.safe_details)?;
        let status = handler.handle_request(get_face_enrollment_status_request(json!({
            "enrollment_session_id": start_status.enrollment_session_id,
        })));
        let completed_status: FaceEnrollmentSessionStatus =
            serde_json::from_value(status.safe_details)?;
        let finish = handler.handle_request(finish_face_enrollment_request(json!({
            "enrollment_session_id": completed_status.enrollment_session_id,
        })));

        assert_eq!(
            finish.operation_status,
            ControlOperationStatus::RequiresElevation
        );
        assert_eq!(
            finish.safe_details["control_error_code"],
            "elevation_required"
        );
        let _ = fs::remove_dir_all(root_dir);
        Ok(())
    }

    #[test]
    fn service_ipc_template_applier_sends_apply_face_template_request()
    -> Result<(), Box<dyn std::error::Error>> {
        let service_client =
            face_auth_service_client_fixture(vec![Ok(ServiceEvent::FaceTemplateApplied {
                template_path: PathBuf::from(r"C:\ProgramData\WinFaceUnlock\selected.json"),
            })]);
        let requests = service_client.requests.clone();
        let applier = ServiceIpcFaceEnrollmentTemplateApplier::new(service_client);
        let template_path = PathBuf::from(r"C:\ProgramData\WinFaceUnlock\selected.json");

        let result = applier.apply_face_enrollment_template(&template_path);

        assert!(result.is_ok());
        assert_eq!(
            requests.borrow().as_slice(),
            &[ServiceRequest::ApplyFaceTemplate { template_path }]
        );
        Ok(())
    }

    #[test]
    fn cancel_face_enrollment_cancels_running_backend_session()
    -> Result<(), Box<dyn std::error::Error>> {
        let root_dir = unique_test_dir("winfaceunlock-control-enrollment-cancel");
        let factory =
            RecordingFaceEnrollmentProcessFactory::new(root_dir.clone(), vec![vec![None]], None);
        let recording = factory.clone();
        let handler = handler_with_face_enrollment_runtime(
            CommandFaceEnrollmentRuntime::with_template_applier(
                factory,
                RecordingFaceEnrollmentTemplateApplier::successful(),
            ),
        );

        let start = handler.handle_request(start_face_enrollment_request(json!({
            "user_id": "dev-user",
            "camera_id": "opencv-index:0",
        })));
        let start_status: FaceEnrollmentSessionStatus = serde_json::from_value(start.safe_details)?;
        let cancel = handler.handle_request(cancel_face_enrollment_request(json!({
            "enrollment_session_id": start_status.enrollment_session_id,
        })));

        assert_eq!(cancel.operation_status, ControlOperationStatus::Completed);
        let cancelled_status: FaceEnrollmentSessionStatus =
            serde_json::from_value(cancel.safe_details)?;
        assert_eq!(
            cancelled_status.session_state,
            FaceEnrollmentSessionState::Cancelled
        );
        assert_eq!(
            recording.cancelled_session_ids()?,
            vec![cancelled_status.enrollment_session_id]
        );
        let _ = fs::remove_dir_all(root_dir);
        Ok(())
    }

    #[test]
    fn cancel_face_enrollment_after_completion_preserves_completed_session()
    -> Result<(), Box<dyn std::error::Error>> {
        let root_dir = unique_test_dir("winfaceunlock-control-enrollment-cancel-completed");
        let factory = RecordingFaceEnrollmentProcessFactory::new(
            root_dir.clone(),
            vec![vec![Some(FaceEnrollmentExit {
                exit_success: true,
                exit_code: Some(0),
            })]],
            Some(completed_face_template_json()),
        );
        let recording = factory.clone();
        let handler = handler_with_face_enrollment_runtime(
            CommandFaceEnrollmentRuntime::with_template_applier(
                factory,
                RecordingFaceEnrollmentTemplateApplier::successful(),
            ),
        );

        let start = handler.handle_request(start_face_enrollment_request(json!({
            "user_id": "dev-user",
            "camera_id": "opencv-index:0",
        })));
        let start_status: FaceEnrollmentSessionStatus = serde_json::from_value(start.safe_details)?;
        let cancel = handler.handle_request(cancel_face_enrollment_request(json!({
            "enrollment_session_id": start_status.enrollment_session_id,
        })));

        assert_eq!(cancel.operation_status, ControlOperationStatus::Completed);
        let completed_status: FaceEnrollmentSessionStatus =
            serde_json::from_value(cancel.safe_details)?;
        assert_eq!(
            completed_status.session_state,
            FaceEnrollmentSessionState::Completed
        );
        assert_eq!(
            completed_status
                .template_summary
                .as_ref()
                .map(|summary| summary.selected_template_count),
            Some(2)
        );
        assert!(recording.cancelled_session_ids()?.is_empty());
        let _ = fs::remove_dir_all(root_dir);
        Ok(())
    }

    #[test]
    fn face_enrollment_start_maps_camera_error_to_semantic_response() {
        let factory = RecordingFaceEnrollmentProcessFactory::with_start_error(
            unique_test_dir("winfaceunlock-control-enrollment-camera-error"),
            FaceEnrollmentProcessError::CameraUnavailable("camera is busy".to_owned()),
        );
        let handler = handler_with_face_enrollment_runtime(
            CommandFaceEnrollmentRuntime::with_template_applier(
                factory,
                RecordingFaceEnrollmentTemplateApplier::successful(),
            ),
        );

        let response = handler.handle_request(start_face_enrollment_request(json!({
            "user_id": "dev-user",
            "camera_id": "opencv-index:0",
        })));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::ServiceUnavailable
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "camera_unavailable"
        );
    }

    #[test]
    fn run_face_auth_self_test_rejects_invalid_payload_before_runner() {
        let handler = handler_with_face_template_store(face_template_store_fixture());

        let response = handler.handle_request(face_auth_self_test_request(json!({
            "session_id": "",
        })));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_auth_self_test_request"
        );
    }

    #[test]
    fn run_face_auth_self_test_fetches_credential_material_and_returns_layered_outcome()
    -> Result<(), serde_json::Error> {
        let grant = auth_grant_fixture("self-test-1");
        let service_client = face_auth_service_client_fixture(vec![
            Ok(ServiceEvent::AuthSucceeded {
                grant: grant.clone(),
            }),
            Ok(ServiceEvent::CredentialMaterialReady {
                grant_id: grant.grant_id.clone(),
                protected_credential_material: credential_material_fixture(),
            }),
        ]);
        let requests = service_client.requests.clone();
        let handler = handler_with_face_auth_service_client(service_client);

        let response = handler.handle_request(face_auth_self_test_request(json!({
            "session_id": "self-test-1",
            "require_credential_ready": true,
        })));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Face auth self-test completed.");
        let outcome: FaceAuthSelfTestOutcome =
            serde_json::from_value(response.safe_details.clone())?;
        assert!(outcome.auth_match_passed);
        assert!(outcome.grant_issued);
        assert!(outcome.credential_material_ready);
        assert!(outcome.credential_decryption_succeeded);
        assert!(outcome.pipe_delivery_confirmed);
        assert_eq!(outcome.best_match_score, Some(0.82));
        assert_eq!(outcome.matched_face_template_ref, None);
        assert_eq!(requests.borrow().len(), 2);
        assert!(matches!(
            &requests.borrow()[0],
            ServiceRequest::WakeAuth {
                source: AuthSource::LocalCamera,
                ..
            }
        ));
        assert!(matches!(
            &requests.borrow()[1],
            ServiceRequest::FetchCredentialMaterial { .. }
        ));
        assert!(!serde_json::to_string(&response)?.contains("protected_password"));
        Ok(())
    }

    #[test]
    fn run_face_auth_self_test_maps_auth_failure_to_match_failure() {
        let service_client = face_auth_service_client_fixture(vec![Ok(ServiceEvent::AuthFailed {
            session_id: SessionId("self-test-2".to_owned()),
            reason: AuthFailureReason::NoFaceDetected,
        })]);
        let handler = handler_with_face_auth_service_client(service_client);

        let response = handler.handle_request(face_auth_self_test_request(json!({
            "session_id": "self-test-2",
        })));

        assert_eq!(response.operation_status, ControlOperationStatus::Failed);
        assert_eq!(
            response.safe_details["control_error_code"],
            "auth_match_failed"
        );
    }

    #[test]
    fn run_face_auth_self_test_maps_ipc_unavailable_to_service_unavailable() {
        let service_client =
            face_auth_service_client_fixture(vec![Err(ProtocolError::TransportUnavailable)]);
        let handler = handler_with_face_auth_service_client(service_client);

        let response = handler.handle_request(face_auth_self_test_request(json!({
            "session_id": "self-test-3",
        })));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::ServiceUnavailable
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "service_status_unavailable"
        );
    }

    #[test]
    fn update_settings_persists_patch_and_returns_snapshot() -> Result<(), serde_json::Error> {
        let settings_store = settings_store_fixture();
        let last_patch = settings_store.last_patch.clone();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store,
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(update_settings_request(json!({
            "presence_lock_enabled": false,
        })));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Settings updated.");
        let decoded_settings: ControlSettingsSnapshot =
            serde_json::from_value(response.safe_details)?;
        assert!(!decoded_settings.presence_lock_enabled);
        assert_eq!(
            last_patch.borrow().as_ref(),
            Some(&ControlSettingsPatch {
                presence_lock_enabled: Some(false),
                logon_wake_mode: None,
            })
        );
        Ok(())
    }

    #[test]
    fn update_settings_accepts_input_triggered_logon_wake_mode() -> Result<(), serde_json::Error> {
        let settings_store = settings_store_fixture();
        let last_patch = settings_store.last_patch.clone();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store,
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(update_settings_request(json!({
            "logon_wake_mode": "input_triggered",
        })));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        let decoded_settings: ControlSettingsSnapshot =
            serde_json::from_value(response.safe_details)?;
        assert_eq!(
            decoded_settings.logon_wake_mode,
            Some(LogonWakeMode::InputTriggered)
        );
        assert_eq!(
            last_patch.borrow().as_ref(),
            Some(&ControlSettingsPatch {
                presence_lock_enabled: None,
                logon_wake_mode: Some(LogonWakeMode::InputTriggered),
            })
        );
        Ok(())
    }

    #[test]
    fn update_settings_rejects_empty_patch() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(update_settings_request(json!({})));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_settings_request"
        );
    }

    #[test]
    fn update_settings_elevation_error_maps_to_requires_elevation_response() {
        let settings_store = RecordingSettingsStore {
            current: Rc::new(RefCell::new(Err(ControlBackendError::requires_elevation(
                "service settings registry update requires elevation",
            )))),
            last_patch: Rc::new(RefCell::new(None)),
        };
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store,
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(update_settings_request(json!({
            "presence_lock_enabled": true,
        })));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::RequiresElevation
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "elevation_required"
        );
    }

    #[test]
    fn generic_control_request_rejects_credential_enrollment_without_secret_channel() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(credential_enrollment_request(json!({})));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_credential_enrollment_request"
        );
    }

    #[test]
    fn credential_enrollment_secret_channel_persists_safe_payload() -> Result<(), serde_json::Error>
    {
        let credential_store = credential_enrollment_store_fixture();
        let last_payload = credential_store.last_payload.clone();
        let last_password = credential_store.last_password.clone();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_store,
        );

        let response = handler.handle_windows_credential_enrollment_request(
            credential_enrollment_request(json!({
                "windows_account_username": "Leo16",
                "user_id": "dev-user",
                "user_sid": "S-1-5-21-winfaceunlock-pending",
                "account_type": "local",
            })),
            WindowsCredentialSecret::from_password("secret".to_owned()),
        );

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Windows credential enrolled.");
        let decoded_outcome: WindowsCredentialEnrollmentOutcome =
            serde_json::from_value(response.safe_details.clone())?;
        assert_eq!(decoded_outcome.windows_account_username, "Leo16");
        assert_eq!(
            last_payload
                .borrow()
                .as_ref()
                .map(|payload| payload.user_id.as_str()),
            Some("dev-user")
        );
        assert_eq!(last_password.borrow().as_deref(), Some("secret"));
        assert!(!serde_json::to_string(&response)?.contains("secret"));
        Ok(())
    }

    #[test]
    fn credential_enrollment_rejects_empty_secret() {
        let credential_store = credential_enrollment_store_fixture();
        let last_payload = credential_store.last_payload.clone();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_store,
        );

        let response = handler.handle_windows_credential_enrollment_request(
            credential_enrollment_request(json!({})),
            WindowsCredentialSecret::from_password(String::new()),
        );

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_credential_enrollment_request"
        );
        assert_eq!(last_payload.borrow().as_ref(), None);
    }
}
