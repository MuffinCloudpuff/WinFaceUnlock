#![allow(unsafe_code)]

use std::{
    fmt, fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use common_protocol::SERVICE_NAME;
use control_protocol::{
    ControlSettingsPatch, ControlSettingsSnapshot, DEFAULT_LOGON_FACE_MATCH_THRESHOLD,
    DashboardStatus, DataDirectorySummary, FaceRecognitionModelSummary, FaceTemplateKind,
    FaceTemplateList, FaceTemplateSourceState, FaceTemplateSummary, LogonWakeMode,
    MAX_LOGON_FACE_MATCH_THRESHOLD, MIN_LOGON_FACE_MATCH_THRESHOLD, PathPresence,
    PresenceMonitorState, PresenceRuntimeSummary, ProviderRegistrationState, ProviderStatusSummary,
    RegistryConfigState, ServiceConfigSummary, ServiceInstallationState, ServiceRuntimeState,
    ServiceStatusSummary,
};
use serde::Deserialize;
use windows_provider::{
    COM_INPROC_SERVER_REGISTRY_PATH, LOGON_WAKE_MODE_BACKGROUND_POLICY,
    LOGON_WAKE_MODE_BACKGROUND_SILENT_RECOGNITION, LOGON_WAKE_MODE_HYBRID,
    LOGON_WAKE_MODE_INPUT_TRIGGERED, LOGON_WAKE_MODE_TRIGGERED_RECOGNITION,
    PROVIDER_CLSID_REGISTRY_PATH, PROVIDER_ROOT_REGISTRY_PATH, REG_VALUE_AUTO_WAKE_ON_ADVISE,
    REG_VALUE_LOGON_WAKE_MODE,
};
use windows_service::{
    service::{ServiceAccess, ServiceState, ServiceStatus},
    service_manager::{ServiceManager, ServiceManagerAccess},
};

const RUNTIME_DIR_NAME: &str = "runtime";
const PREVIEW_FRAME_FILE_NAME: &str = "preview_frame.jpg";
const PRESENCE_AUDIT_DIR_NAME: &str = "presence-audit";
const PRESENCE_RUNTIME_STATUS_FILE_NAME: &str = "presence-runtime-status.json";
const SERVICE_CONFIG_REGISTRY_PATH: &str = r"SOFTWARE\WinFaceUnlock\Service";
const REG_AUTH_MODE: &str = "AuthMode";
const REG_FACE_TEMPLATE_PATH: &str = "FaceTemplatePath";
const REG_CAMERA_ID: &str = "CameraId";
const REG_YUNET_MODEL_PATH: &str = "YuNetModelPath";
const REG_SFACE_MODEL_PATH: &str = "SFaceModelPath";
const REG_MINIFASNET_MODEL_PATH: &str = "MiniFasNetModelPath";
const REG_MATCH_THRESHOLD: &str = "MatchThreshold";
const REG_PRESENCE_LOCK_ENABLED: &str = "PresenceLockEnabled";
const REG_INTRUDER_SNAP_ENABLED: &str = "IntruderSnapEnabled";
const REG_PRESENCE_DETECTOR_KIND: &str = "PresenceDetectorKind";
const REG_PRESENCE_TRACKING_MODE: &str = "PresenceTrackingMode";
const AUTH_MODE_LOCAL_CAMERA: &str = "local-camera";
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
pub const ACTIVE_SERVICE_FACE_TEMPLATE_REF: &str = "active-service-template";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlStatusPaths {
    pub program_data_dir: PathBuf,
    pub presence_audit_dir: PathBuf,
    pub presence_runtime_status_path: PathBuf,
}

impl ControlStatusPaths {
    pub fn from_environment_or_default() -> Self {
        let install_dir = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| std::env::temp_dir().join("WinFaceUnlock"));
        Self::from_program_data_dir(install_dir)
    }

    pub fn from_program_data_dir(program_data_dir: PathBuf) -> Self {
        Self {
            presence_audit_dir: program_data_dir.join(PRESENCE_AUDIT_DIR_NAME),
            presence_runtime_status_path: program_data_dir
                .join(RUNTIME_DIR_NAME)
                .join(PRESENCE_RUNTIME_STATUS_FILE_NAME),
            program_data_dir,
        }
    }
}

pub struct WindowsDashboardStatusProvider {
    paths: ControlStatusPaths,
}

impl WindowsDashboardStatusProvider {
    pub fn from_environment_or_default() -> Self {
        Self {
            paths: ControlStatusPaths::from_environment_or_default(),
        }
    }

    pub fn with_paths(paths: ControlStatusPaths) -> Self {
        Self { paths }
    }

    pub fn load_dashboard_status(&self) -> Result<DashboardStatus, ControlStatusError> {
        load_dashboard_status(&WindowsStatusSource { paths: &self.paths })
    }
}

impl Default for WindowsDashboardStatusProvider {
    fn default() -> Self {
        Self::from_environment_or_default()
    }
}

pub struct WindowsFaceTemplateStatusStore {
    paths: ControlStatusPaths,
}

impl WindowsFaceTemplateStatusStore {
    pub fn from_environment_or_default() -> Self {
        Self {
            paths: ControlStatusPaths::from_environment_or_default(),
        }
    }

    pub fn with_paths(paths: ControlStatusPaths) -> Self {
        Self { paths }
    }

    pub fn load_face_templates(&self) -> Result<FaceTemplateList, FaceTemplateStatusError> {
        load_face_template_list(&WindowsStatusSource { paths: &self.paths })
    }

    pub fn load_active_face_template_path(&self) -> Result<PathBuf, FaceTemplateStatusError> {
        load_active_face_template_path(&WindowsStatusSource { paths: &self.paths })
    }

    pub fn apply_active_face_template_path(
        &self,
        template_path: &Path,
    ) -> Result<(), FaceTemplateStatusError> {
        apply_active_face_template_path(&WindowsFaceTemplateConfigWriter, template_path)
    }

    pub fn apply_local_camera_auth_config(
        &self,
        template_path: &Path,
        camera_id: &str,
        install_dir: &Path,
    ) -> Result<(), FaceTemplateStatusError> {
        apply_local_camera_auth_config(
            &WindowsFaceTemplateConfigWriter,
            template_path,
            camera_id,
            install_dir,
        )
    }
}

impl Default for WindowsFaceTemplateStatusStore {
    fn default() -> Self {
        Self::from_environment_or_default()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsControlSettingsStore;

impl WindowsControlSettingsStore {
    pub fn new() -> Self {
        Self
    }

    pub fn load_settings(&self) -> Result<ControlSettingsSnapshot, ControlStatusError> {
        load_control_settings(&WindowsSettingsSource)
    }

    pub fn update_settings(
        &self,
        patch: &ControlSettingsPatch,
    ) -> Result<ControlSettingsSnapshot, ControlStatusError> {
        update_control_settings(&WindowsSettingsSource, patch)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlStatusError {
    ServiceStatusUnavailable(String),
    ProviderStatusUnavailable(String),
    ServiceConfigUnavailable(String),
    SettingsUnavailable(String),
    SettingsPersistenceFailed(String),
    DataDirectoryStatusUnavailable(String),
    PresenceRuntimeStatusUnavailable(String),
    ElevationRequired(String),
    PermissionDenied(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FaceTemplateStatusError {
    ServiceConfigUnavailable(String),
    TemplateConfigMissing(String),
    TemplateFileMissing(String),
    TemplateParseFailed(String),
    TemplateEmpty(String),
    PermissionDenied(String),
}

impl FaceTemplateStatusError {
    pub fn message(&self) -> &str {
        match self {
            Self::ServiceConfigUnavailable(message)
            | Self::TemplateConfigMissing(message)
            | Self::TemplateFileMissing(message)
            | Self::TemplateParseFailed(message)
            | Self::TemplateEmpty(message)
            | Self::PermissionDenied(message) => message,
        }
    }
}

trait FaceTemplateConfigWriter {
    fn write_active_face_template_path(
        &self,
        template_path: &Path,
    ) -> Result<(), FaceTemplateStatusError>;

    fn write_local_camera_auth_config(
        &self,
        template_path: &Path,
        camera_id: &str,
        install_dir: &Path,
    ) -> Result<(), FaceTemplateStatusError>;
}

struct WindowsFaceTemplateConfigWriter;

impl FaceTemplateConfigWriter for WindowsFaceTemplateConfigWriter {
    fn write_active_face_template_path(
        &self,
        template_path: &Path,
    ) -> Result<(), FaceTemplateStatusError> {
        registry::write_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_FACE_TEMPLATE_PATH,
            &template_path.display().to_string(),
        )
        .map_err(face_template_registry_write_error)
    }

    fn write_local_camera_auth_config(
        &self,
        template_path: &Path,
        camera_id: &str,
        install_dir: &Path,
    ) -> Result<(), FaceTemplateStatusError> {
        let models_dir = install_dir.join("models");
        for (name, value) in [
            (REG_AUTH_MODE, AUTH_MODE_LOCAL_CAMERA.to_owned()),
            (REG_FACE_TEMPLATE_PATH, template_path.display().to_string()),
            (REG_CAMERA_ID, camera_id.to_owned()),
            (
                REG_YUNET_MODEL_PATH,
                models_dir
                    .join("face_detection_yunet_2023mar.onnx")
                    .display()
                    .to_string(),
            ),
            (
                REG_SFACE_MODEL_PATH,
                models_dir
                    .join("ghostfacenet_v1_stride2.onnx")
                    .display()
                    .to_string(),
            ),
            (
                REG_MINIFASNET_MODEL_PATH,
                models_dir.join("minifasnet_v2.onnx").display().to_string(),
            ),
        ] {
            registry::write_string_value(SERVICE_CONFIG_REGISTRY_PATH, name, &value)
                .map_err(face_template_registry_write_error)?;
        }
        Ok(())
    }
}

impl fmt::Display for FaceTemplateStatusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for FaceTemplateStatusError {}

impl ControlStatusError {
    pub fn message(&self) -> &str {
        match self {
            Self::ServiceStatusUnavailable(message)
            | Self::ProviderStatusUnavailable(message)
            | Self::ServiceConfigUnavailable(message)
            | Self::SettingsUnavailable(message)
            | Self::SettingsPersistenceFailed(message)
            | Self::DataDirectoryStatusUnavailable(message)
            | Self::PresenceRuntimeStatusUnavailable(message)
            | Self::ElevationRequired(message)
            | Self::PermissionDenied(message) => message,
        }
    }
}

impl fmt::Display for ControlStatusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for ControlStatusError {}

trait StatusSource {
    fn query_service_status(&self) -> Result<Option<ObservedServiceStatus>, ControlStatusError>;
    fn query_provider_registry_status(
        &self,
    ) -> Result<ObservedProviderRegistryStatus, ControlStatusError>;
    fn query_service_config_status(
        &self,
    ) -> Result<ObservedServiceConfigStatus, ControlStatusError>;
    fn query_data_directory_status(&self) -> Result<DataDirectorySummary, ControlStatusError>;
    fn query_presence_runtime_status(
        &self,
    ) -> Result<Option<PresenceRuntimeSummary>, ControlStatusError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedServiceStatus {
    runtime_state: ServiceRuntimeState,
    process_id: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedProviderRegistryStatus {
    credential_provider_registered: bool,
    com_server_registered: bool,
    project_config_registered: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedServiceConfigStatus {
    registry_config_exists: bool,
    auth_mode: Option<String>,
    face_template_path: Option<String>,
    presence_lock_enabled: Option<String>,
    intruder_snap_enabled: Option<String>,
    presence_detector_kind: Option<String>,
    presence_tracking_mode: Option<String>,
}

trait SettingsSource {
    fn read_presence_lock_enabled(&self) -> Result<Option<String>, ControlStatusError>;
    fn write_presence_lock_enabled(&self, enabled: bool) -> Result<(), ControlStatusError>;
    fn read_intruder_snap_enabled(&self) -> Result<Option<String>, ControlStatusError>;
    fn write_intruder_snap_enabled(&self, enabled: bool) -> Result<(), ControlStatusError>;
    fn read_logon_wake_mode(&self) -> Result<Option<String>, ControlStatusError>;
    fn read_auto_wake_on_advise(&self) -> Result<Option<String>, ControlStatusError>;
    fn write_logon_wake_mode(&self, mode: LogonWakeMode) -> Result<(), ControlStatusError>;
    fn read_logon_face_match_threshold(&self) -> Result<Option<String>, ControlStatusError>;
    fn write_logon_face_match_threshold(&self, threshold: f32) -> Result<(), ControlStatusError>;
}

fn load_dashboard_status(
    source: &impl StatusSource,
) -> Result<DashboardStatus, ControlStatusError> {
    Ok(DashboardStatus {
        service: map_service_status(source.query_service_status()?),
        provider: map_provider_status(source.query_provider_registry_status()?),
        service_config: map_service_config(source.query_service_config_status()?),
        data_directory: source.query_data_directory_status()?,
        presence_runtime: source.query_presence_runtime_status()?,
    })
}

fn load_face_template_list(
    source: &impl StatusSource,
) -> Result<FaceTemplateList, FaceTemplateStatusError> {
    let template_path = load_active_face_template_path(source)?;

    Ok(FaceTemplateList {
        templates: vec![read_active_service_face_template_summary(&template_path)?],
    })
}

fn load_active_face_template_path(
    source: &impl StatusSource,
) -> Result<PathBuf, FaceTemplateStatusError> {
    let service_config = source
        .query_service_config_status()
        .map_err(face_template_service_config_error)?;
    if !service_config.registry_config_exists {
        return Err(FaceTemplateStatusError::TemplateConfigMissing(
            "service configuration is missing a face template path".to_owned(),
        ));
    }

    let template_path = service_config
        .face_template_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            FaceTemplateStatusError::TemplateConfigMissing(
                "service configuration is missing a face template path".to_owned(),
            )
        })?;

    Ok(template_path)
}

fn apply_active_face_template_path(
    writer: &impl FaceTemplateConfigWriter,
    template_path: &Path,
) -> Result<(), FaceTemplateStatusError> {
    if template_path.as_os_str().is_empty() {
        return Err(FaceTemplateStatusError::TemplateConfigMissing(
            "face template path is empty".to_owned(),
        ));
    }
    writer.write_active_face_template_path(template_path)
}

fn apply_local_camera_auth_config(
    writer: &impl FaceTemplateConfigWriter,
    template_path: &Path,
    camera_id: &str,
    install_dir: &Path,
) -> Result<(), FaceTemplateStatusError> {
    if template_path.as_os_str().is_empty() {
        return Err(FaceTemplateStatusError::TemplateConfigMissing(
            "face template path is empty".to_owned(),
        ));
    }
    if camera_id.trim().is_empty() {
        return Err(FaceTemplateStatusError::TemplateConfigMissing(
            "camera id is empty".to_owned(),
        ));
    }
    if install_dir.as_os_str().is_empty() {
        return Err(FaceTemplateStatusError::TemplateConfigMissing(
            "install dir is empty".to_owned(),
        ));
    }
    writer.write_local_camera_auth_config(template_path, camera_id.trim(), install_dir)
}

fn read_active_service_face_template_summary(
    template_path: &Path,
) -> Result<FaceTemplateSummary, FaceTemplateStatusError> {
    summarize_selected_face_template_file(
        template_path,
        ACTIVE_SERVICE_FACE_TEMPLATE_REF,
        FaceTemplateSourceState::ActiveServiceTemplate,
    )
}

pub fn summarize_selected_face_template_file(
    template_path: &Path,
    face_template_ref: impl Into<String>,
    source_state: FaceTemplateSourceState,
) -> Result<FaceTemplateSummary, FaceTemplateStatusError> {
    let metadata = fs::metadata(template_path).map_err(face_template_file_read_error)?;
    let bytes = fs::read(template_path).map_err(face_template_file_read_error)?;
    let selected_template_set: SelectedTemplateSetFile =
        serde_json::from_slice(&bytes).map_err(|error| {
            FaceTemplateStatusError::TemplateParseFailed(format!(
                "active face template file is invalid: {error}"
            ))
        })?;
    let selected_template_count = selected_template_set.selected_template_count();
    if selected_template_count == 0 {
        return Err(FaceTemplateStatusError::TemplateEmpty(
            "active face template file does not contain selected unlock templates".to_owned(),
        ));
    }

    Ok(FaceTemplateSummary {
        face_template_ref: face_template_ref.into(),
        user_id: selected_template_set.user_id.clone(),
        display_name: Some(default_face_display_name()),
        avatar_preview: read_face_avatar_preview(template_path),
        template_kind: FaceTemplateKind::SelectedTemplateSet,
        recognition_model: FaceRecognitionModelSummary {
            model_family: selected_template_set.recognizer_model_family,
            model_version: selected_template_set.recognizer_model_version,
        },
        selected_template_count,
        rejected_sample_count: selected_template_set
            .quality_summary
            .and_then(|summary| summary.rejected_sample_count),
        created_at_unix_ms: selected_template_set.enrollment_created_at_unix_ms,
        updated_at_unix_ms: metadata_modified_unix_ms(&metadata),
        source_state,
    })
}

fn default_face_display_name() -> String {
    "用户1".to_owned()
}

fn read_face_avatar_preview(template_path: &Path) -> Option<control_protocol::FaceAvatarPreview> {
    let preview_frame_path = template_path.parent()?.join(PREVIEW_FRAME_FILE_NAME);
    let image_bytes = fs::read(&preview_frame_path).ok()?;
    Some(control_protocol::FaceAvatarPreview {
        mime_type: "image/jpeg".to_owned(),
        image_base64: BASE64_STANDARD.encode(image_bytes),
        updated_at_unix_ms: metadata_modified_unix_ms(&fs::metadata(preview_frame_path).ok()?),
    })
}

#[derive(Deserialize)]
struct SelectedTemplateSetFile {
    user_id: String,
    recognizer_model_family: String,
    recognizer_model_version: String,
    enrollment_created_at_unix_ms: Option<i64>,
    #[serde(default)]
    templates: Vec<SelectedTemplateFileEntry>,
    quality_summary: Option<SelectedTemplateQualitySummary>,
}

impl SelectedTemplateSetFile {
    fn selected_template_count(&self) -> u32 {
        self.quality_summary
            .as_ref()
            .and_then(|summary| summary.selected_template_count)
            .unwrap_or_else(|| {
                self.templates
                    .iter()
                    .filter(|template| template.selected_for_unlock)
                    .count()
                    .try_into()
                    .unwrap_or(u32::MAX)
            })
    }
}

#[derive(Deserialize)]
struct SelectedTemplateFileEntry {
    #[serde(default = "default_selected_for_unlock")]
    selected_for_unlock: bool,
}

#[derive(Deserialize)]
struct SelectedTemplateQualitySummary {
    selected_template_count: Option<u32>,
    rejected_sample_count: Option<u32>,
}

fn default_selected_for_unlock() -> bool {
    true
}

fn metadata_modified_unix_ms(metadata: &fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()
}

fn map_service_status(status: Option<ObservedServiceStatus>) -> ServiceStatusSummary {
    match status {
        Some(status) => ServiceStatusSummary {
            installation_state: ServiceInstallationState::Installed,
            runtime_state: status.runtime_state,
            process_id: status.process_id,
        },
        None => ServiceStatusSummary {
            installation_state: ServiceInstallationState::Missing,
            runtime_state: ServiceRuntimeState::Missing,
            process_id: None,
        },
    }
}

fn map_provider_status(status: ObservedProviderRegistryStatus) -> ProviderStatusSummary {
    let registration_state = if status.credential_provider_registered
        && status.com_server_registered
        && status.project_config_registered
    {
        ProviderRegistrationState::Registered
    } else if status.credential_provider_registered
        || status.com_server_registered
        || status.project_config_registered
    {
        ProviderRegistrationState::PartiallyRegistered
    } else {
        ProviderRegistrationState::NotRegistered
    };

    ProviderStatusSummary {
        registration_state,
        credential_provider_registered: status.credential_provider_registered,
        com_server_registered: status.com_server_registered,
        project_config_registered: status.project_config_registered,
    }
}

fn map_service_config(status: ObservedServiceConfigStatus) -> ServiceConfigSummary {
    ServiceConfigSummary {
        registry_config_state: if status.registry_config_exists {
            RegistryConfigState::Present
        } else {
            RegistryConfigState::Missing
        },
        auth_mode: status.auth_mode,
        face_template_path: status.face_template_path,
        presence_lock_enabled: status
            .presence_lock_enabled
            .as_deref()
            .and_then(parse_registry_bool),
        intruder_snap_enabled: status
            .intruder_snap_enabled
            .as_deref()
            .and_then(parse_registry_bool),
        presence_detector_kind: status.presence_detector_kind,
        presence_tracking_mode: status.presence_tracking_mode,
    }
}

fn parse_registry_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "enabled" => Some(true),
        "0" | "false" | "no" | "disabled" => Some(false),
        _ => None,
    }
}

fn bool_registry_value(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn logon_wake_mode_registry_value(mode: LogonWakeMode) -> &'static str {
    match mode {
        LogonWakeMode::TriggeredRecognition => LOGON_WAKE_MODE_TRIGGERED_RECOGNITION,
        LogonWakeMode::BackgroundSilentRecognition => LOGON_WAKE_MODE_BACKGROUND_SILENT_RECOGNITION,
    }
}

fn parse_logon_wake_mode(value: &str) -> Option<LogonWakeMode> {
    match value.trim() {
        LOGON_WAKE_MODE_TRIGGERED_RECOGNITION
        | "triggered_recognition"
        | LOGON_WAKE_MODE_INPUT_TRIGGERED
        | "input_triggered" => Some(LogonWakeMode::TriggeredRecognition),
        LOGON_WAKE_MODE_BACKGROUND_SILENT_RECOGNITION
        | "background_silent_recognition"
        | LOGON_WAKE_MODE_BACKGROUND_POLICY
        | "background_policy"
        | LOGON_WAKE_MODE_HYBRID => Some(LogonWakeMode::BackgroundSilentRecognition),
        _ => None,
    }
}

fn legacy_logon_wake_mode(auto_wake_on_advise: Option<String>) -> Option<LogonWakeMode> {
    auto_wake_on_advise
        .as_deref()
        .and_then(parse_registry_bool)
        .and_then(|enabled| enabled.then_some(LogonWakeMode::TriggeredRecognition))
}

fn parse_logon_face_match_threshold(value: &str) -> Option<f32> {
    let threshold = value.trim().parse::<f32>().ok()?;
    if is_valid_logon_face_match_threshold(threshold) {
        Some(threshold)
    } else {
        None
    }
}

fn is_valid_logon_face_match_threshold(threshold: f32) -> bool {
    threshold.is_finite()
        && (MIN_LOGON_FACE_MATCH_THRESHOLD..=MAX_LOGON_FACE_MATCH_THRESHOLD).contains(&threshold)
}

fn load_control_settings(
    source: &impl SettingsSource,
) -> Result<ControlSettingsSnapshot, ControlStatusError> {
    Ok(ControlSettingsSnapshot {
        presence_lock_enabled: source
            .read_presence_lock_enabled()?
            .as_deref()
            .and_then(parse_registry_bool)
            .unwrap_or(false),
        intruder_snap_enabled: source
            .read_intruder_snap_enabled()?
            .as_deref()
            .and_then(parse_registry_bool)
            .unwrap_or(true),
        logon_wake_mode: source
            .read_logon_wake_mode()?
            .as_deref()
            .and_then(parse_logon_wake_mode)
            .or_else(|| legacy_logon_wake_mode(source.read_auto_wake_on_advise().ok().flatten())),
        logon_face_match_threshold: source
            .read_logon_face_match_threshold()?
            .as_deref()
            .and_then(parse_logon_face_match_threshold)
            .unwrap_or(DEFAULT_LOGON_FACE_MATCH_THRESHOLD),
    })
}

fn update_control_settings(
    source: &impl SettingsSource,
    patch: &ControlSettingsPatch,
) -> Result<ControlSettingsSnapshot, ControlStatusError> {
    if let Some(enabled) = patch.presence_lock_enabled {
        source.write_presence_lock_enabled(enabled)?;
    }
    if let Some(enabled) = patch.intruder_snap_enabled {
        source.write_intruder_snap_enabled(enabled)?;
    }
    if let Some(mode) = patch.logon_wake_mode {
        source.write_logon_wake_mode(mode)?;
    }
    if let Some(threshold) = patch.logon_face_match_threshold {
        if !is_valid_logon_face_match_threshold(threshold) {
            return Err(ControlStatusError::SettingsPersistenceFailed(format!(
                "logon face match threshold must be between {MIN_LOGON_FACE_MATCH_THRESHOLD:.2} and {MAX_LOGON_FACE_MATCH_THRESHOLD:.2}"
            )));
        }
        source.write_logon_face_match_threshold(threshold)?;
    }
    load_control_settings(source)
}

struct WindowsStatusSource<'a> {
    paths: &'a ControlStatusPaths,
}

struct WindowsSettingsSource;

impl SettingsSource for WindowsSettingsSource {
    fn read_presence_lock_enabled(&self) -> Result<Option<String>, ControlStatusError> {
        registry::read_string_value(SERVICE_CONFIG_REGISTRY_PATH, REG_PRESENCE_LOCK_ENABLED)
            .map_err(settings_read_error)
    }

    fn write_presence_lock_enabled(&self, enabled: bool) -> Result<(), ControlStatusError> {
        registry::write_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_LOCK_ENABLED,
            bool_registry_value(enabled),
        )
        .map_err(settings_write_error)
    }

    fn read_intruder_snap_enabled(&self) -> Result<Option<String>, ControlStatusError> {
        registry::read_string_value(SERVICE_CONFIG_REGISTRY_PATH, REG_INTRUDER_SNAP_ENABLED)
            .map_err(settings_read_error)
    }

    fn write_intruder_snap_enabled(&self, enabled: bool) -> Result<(), ControlStatusError> {
        registry::write_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_INTRUDER_SNAP_ENABLED,
            bool_registry_value(enabled),
        )
        .map_err(settings_write_error)
    }

    fn read_logon_wake_mode(&self) -> Result<Option<String>, ControlStatusError> {
        registry::read_string_value(PROVIDER_ROOT_REGISTRY_PATH, REG_VALUE_LOGON_WAKE_MODE)
            .map_err(settings_read_error)
    }

    fn read_auto_wake_on_advise(&self) -> Result<Option<String>, ControlStatusError> {
        registry::read_string_value(PROVIDER_ROOT_REGISTRY_PATH, REG_VALUE_AUTO_WAKE_ON_ADVISE)
            .map_err(settings_read_error)
    }

    fn write_logon_wake_mode(&self, mode: LogonWakeMode) -> Result<(), ControlStatusError> {
        registry::write_string_value(
            PROVIDER_ROOT_REGISTRY_PATH,
            REG_VALUE_LOGON_WAKE_MODE,
            logon_wake_mode_registry_value(mode),
        )
        .map_err(settings_write_error)?;
        registry::write_string_value(
            PROVIDER_ROOT_REGISTRY_PATH,
            REG_VALUE_AUTO_WAKE_ON_ADVISE,
            bool_registry_value(true),
        )
        .map_err(settings_write_error)
    }

    fn read_logon_face_match_threshold(&self) -> Result<Option<String>, ControlStatusError> {
        registry::read_string_value(SERVICE_CONFIG_REGISTRY_PATH, REG_MATCH_THRESHOLD)
            .map_err(settings_read_error)
    }

    fn write_logon_face_match_threshold(&self, threshold: f32) -> Result<(), ControlStatusError> {
        registry::write_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_MATCH_THRESHOLD,
            &format!("{threshold:.3}"),
        )
        .map_err(settings_write_error)
    }
}

impl StatusSource for WindowsStatusSource<'_> {
    fn query_service_status(&self) -> Result<Option<ObservedServiceStatus>, ControlStatusError> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(service_error)?;
        match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
            Ok(service) => service
                .query_status()
                .map(|status| Some(map_observed_service_status(status)))
                .map_err(service_error),
            Err(error) if is_service_missing_error(&error) => Ok(None),
            Err(error) => Err(service_error(error)),
        }
    }

    fn query_provider_registry_status(
        &self,
    ) -> Result<ObservedProviderRegistryStatus, ControlStatusError> {
        Ok(ObservedProviderRegistryStatus {
            credential_provider_registered: registry::key_exists(PROVIDER_CLSID_REGISTRY_PATH)
                .map_err(provider_registry_error)?,
            com_server_registered: registry::key_exists(COM_INPROC_SERVER_REGISTRY_PATH)
                .map_err(provider_registry_error)?,
            project_config_registered: registry::key_exists(PROVIDER_ROOT_REGISTRY_PATH)
                .map_err(provider_registry_error)?,
        })
    }

    fn query_service_config_status(
        &self,
    ) -> Result<ObservedServiceConfigStatus, ControlStatusError> {
        let registry_config_exists =
            registry::key_exists(SERVICE_CONFIG_REGISTRY_PATH).map_err(service_config_error)?;
        Ok(ObservedServiceConfigStatus {
            registry_config_exists,
            auth_mode: registry::read_string_value(SERVICE_CONFIG_REGISTRY_PATH, REG_AUTH_MODE)
                .map_err(service_config_error)?,
            face_template_path: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_FACE_TEMPLATE_PATH,
            )
            .map_err(service_config_error)?,
            presence_lock_enabled: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_LOCK_ENABLED,
            )
            .ok()
            .flatten(),
            intruder_snap_enabled: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_INTRUDER_SNAP_ENABLED,
            )
            .ok()
            .flatten(),
            presence_detector_kind: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_DETECTOR_KIND,
            )
            .map_err(service_config_error)?,
            presence_tracking_mode: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_TRACKING_MODE,
            )
            .map_err(service_config_error)?,
        })
    }

    fn query_data_directory_status(&self) -> Result<DataDirectorySummary, ControlStatusError> {
        let paths = self.resolved_paths_from_service_config()?;
        Ok(DataDirectorySummary {
            program_data_dir: Some(paths.program_data_dir.display().to_string()),
            program_data_presence: path_presence(&paths.program_data_dir),
            presence_audit_dir: Some(paths.presence_audit_dir.display().to_string()),
            presence_audit_presence: path_presence(&paths.presence_audit_dir),
        })
    }

    fn query_presence_runtime_status(
        &self,
    ) -> Result<Option<PresenceRuntimeSummary>, ControlStatusError> {
        let paths = self.resolved_paths_from_service_config()?;
        if !paths.presence_runtime_status_path.exists() {
            return Ok(Some(PresenceRuntimeSummary {
                monitor_state: PresenceMonitorState::Unavailable,
                session_id: None,
                reason: Some("presence runtime status file is missing".to_owned()),
                updated_at_unix_ms: None,
            }));
        }
        let text = fs::read_to_string(&paths.presence_runtime_status_path).map_err(|error| {
            ControlStatusError::PresenceRuntimeStatusUnavailable(format!(
                "presence runtime status file cannot be read: {error}"
            ))
        })?;
        let status: PresenceRuntimeStatusFile = serde_json::from_str(&text).map_err(|error| {
            ControlStatusError::PresenceRuntimeStatusUnavailable(format!(
                "presence runtime status file is invalid: {error}"
            ))
        })?;
        Ok(Some(status.into_summary()))
    }
}

impl WindowsStatusSource<'_> {
    fn resolved_paths_from_service_config(&self) -> Result<ControlStatusPaths, ControlStatusError> {
        let template_path =
            registry::read_string_value(SERVICE_CONFIG_REGISTRY_PATH, REG_FACE_TEMPLATE_PATH)
                .map_err(service_config_error)?;
        template_path
            .as_deref()
            .and_then(install_dir_from_service_template_path)
            .map(ControlStatusPaths::from_program_data_dir)
            .or_else(|| Some(self.paths.clone()))
            .ok_or_else(|| {
                ControlStatusError::DataDirectoryStatusUnavailable(
                    "install data root could not be resolved".to_owned(),
                )
            })
    }
}

fn install_dir_from_service_template_path(template_path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(template_path);
    let face_enrollment_dir = path.parent()?;
    if face_enrollment_dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("face-enrollment"))
    {
        return face_enrollment_dir.parent().map(Path::to_path_buf);
    }
    None
}

fn map_observed_service_status(status: ServiceStatus) -> ObservedServiceStatus {
    ObservedServiceStatus {
        runtime_state: map_service_runtime_state(status.current_state),
        process_id: status.process_id,
    }
}

fn map_service_runtime_state(state: ServiceState) -> ServiceRuntimeState {
    match state {
        ServiceState::Running => ServiceRuntimeState::Running,
        ServiceState::Stopped => ServiceRuntimeState::Stopped,
        ServiceState::Paused => ServiceRuntimeState::Paused,
        ServiceState::StartPending => ServiceRuntimeState::StartPending,
        ServiceState::StopPending => ServiceRuntimeState::StopPending,
        other => ServiceRuntimeState::Unknown(format!("{other:?}")),
    }
}

fn path_presence(path: &Path) -> PathPresence {
    if path.exists() {
        PathPresence::Present
    } else {
        PathPresence::Missing
    }
}

#[derive(Deserialize)]
struct PresenceRuntimeStatusFile {
    state: String,
    session_id: Option<u32>,
    reason: String,
    updated_at_unix_ms: i64,
}

impl PresenceRuntimeStatusFile {
    fn into_summary(self) -> PresenceRuntimeSummary {
        PresenceRuntimeSummary {
            monitor_state: map_presence_monitor_state(&self.state),
            session_id: self.session_id,
            reason: Some(self.reason),
            updated_at_unix_ms: Some(self.updated_at_unix_ms),
        }
    }
}

fn map_presence_monitor_state(state: &str) -> PresenceMonitorState {
    match state {
        "running" => PresenceMonitorState::Running,
        "stopped" => PresenceMonitorState::Stopped,
        "disabled" => PresenceMonitorState::Disabled,
        "unavailable" => PresenceMonitorState::Unavailable,
        other => PresenceMonitorState::Unknown(other.to_owned()),
    }
}

fn service_error(error: windows_service::Error) -> ControlStatusError {
    if is_access_denied_service_error(&error) {
        ControlStatusError::PermissionDenied("service status access denied".to_owned())
    } else {
        ControlStatusError::ServiceStatusUnavailable(format!(
            "service status cannot be queried: {error}"
        ))
    }
}

fn is_service_missing_error(error: &windows_service::Error) -> bool {
    matches!(
        error,
        windows_service::Error::Winapi(io_error)
            if io_error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST)
    )
}

fn is_access_denied_service_error(error: &windows_service::Error) -> bool {
    matches!(
        error,
        windows_service::Error::Winapi(io_error)
            if io_error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
    )
}

fn provider_registry_error(error: RegistryReadError) -> ControlStatusError {
    if error.is_access_denied() {
        ControlStatusError::PermissionDenied("provider registry access denied".to_owned())
    } else {
        ControlStatusError::ProviderStatusUnavailable(format!(
            "provider registry status cannot be read: {error}"
        ))
    }
}

fn service_config_error(error: RegistryReadError) -> ControlStatusError {
    if error.is_access_denied() {
        ControlStatusError::PermissionDenied("service config registry access denied".to_owned())
    } else {
        ControlStatusError::ServiceConfigUnavailable(format!(
            "service config registry status cannot be read: {error}"
        ))
    }
}

fn face_template_service_config_error(error: ControlStatusError) -> FaceTemplateStatusError {
    match error {
        ControlStatusError::PermissionDenied(message) => {
            FaceTemplateStatusError::PermissionDenied(message)
        }
        other => FaceTemplateStatusError::ServiceConfigUnavailable(format!(
            "service face template configuration cannot be read: {other}"
        )),
    }
}

fn face_template_registry_write_error(error: RegistryWriteError) -> FaceTemplateStatusError {
    if error.is_access_denied() {
        FaceTemplateStatusError::PermissionDenied(
            "service face template configuration update requires elevation".to_owned(),
        )
    } else {
        FaceTemplateStatusError::ServiceConfigUnavailable(format!(
            "service face template configuration cannot be updated: {error}"
        ))
    }
}

fn face_template_file_read_error(error: std::io::Error) -> FaceTemplateStatusError {
    if error.kind() == std::io::ErrorKind::NotFound {
        FaceTemplateStatusError::TemplateFileMissing(
            "configured face template file is missing".to_owned(),
        )
    } else if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
        FaceTemplateStatusError::PermissionDenied(
            "configured face template file access denied".to_owned(),
        )
    } else {
        FaceTemplateStatusError::TemplateParseFailed(format!(
            "configured face template file cannot be read: {error}"
        ))
    }
}

fn settings_read_error(error: RegistryReadError) -> ControlStatusError {
    if error.is_access_denied() {
        ControlStatusError::PermissionDenied("service settings registry access denied".to_owned())
    } else {
        ControlStatusError::SettingsUnavailable(format!(
            "service settings registry cannot be read: {error}"
        ))
    }
}

fn settings_write_error(error: RegistryWriteError) -> ControlStatusError {
    if error.is_access_denied() {
        ControlStatusError::ElevationRequired(
            "service settings registry update requires elevation".to_owned(),
        )
    } else {
        ControlStatusError::SettingsPersistenceFailed(format!(
            "service settings registry cannot be updated: {error}"
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RegistryReadError {
    operation: &'static str,
    path: String,
    value_name: Option<String>,
    code: u32,
}

impl RegistryReadError {
    fn is_access_denied(&self) -> bool {
        self.code == ERROR_ACCESS_DENIED
    }
}

impl fmt::Display for RegistryReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value_name {
            Some(value_name) => write!(
                formatter,
                "{} failed for {}\\{}: error {}",
                self.operation, self.path, value_name, self.code
            ),
            None => write!(
                formatter,
                "{} failed for {}: error {}",
                self.operation, self.path, self.code
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RegistryWriteError {
    operation: &'static str,
    path: String,
    value_name: String,
    code: u32,
}

impl RegistryWriteError {
    fn is_access_denied(&self) -> bool {
        self.code == ERROR_ACCESS_DENIED
    }
}

impl fmt::Display for RegistryWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} failed for {}\\{}: error {}",
            self.operation, self.path, self.value_name, self.code
        )
    }
}

#[cfg(windows)]
mod registry {
    use std::ptr;

    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
        System::Registry::{
            HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
            RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        },
    };

    use super::{RegistryReadError, RegistryWriteError};

    pub fn key_exists(path: &str) -> Result<bool, RegistryReadError> {
        match open_key(path) {
            Ok(Some(_key)) => Ok(true),
            Ok(None) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn read_string_value(
        path: &str,
        value_name: &str,
    ) -> Result<Option<String>, RegistryReadError> {
        let Some(key) = open_key(path)? else {
            return Ok(None);
        };
        let name = to_wide_null(value_name);
        let mut data_type = 0_u32;
        let mut byte_len = 0_u32;
        let probe_status = unsafe {
            RegQueryValueExW(
                key.raw,
                name.as_ptr(),
                ptr::null_mut(),
                &mut data_type,
                ptr::null_mut(),
                &mut byte_len,
            )
        };
        if probe_status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if probe_status != ERROR_SUCCESS {
            return Err(registry_error(
                "query value",
                path,
                Some(value_name),
                probe_status,
            ));
        }
        if data_type != REG_SZ || byte_len < 2 {
            return Ok(None);
        }

        let mut data = vec![0_u16; byte_len as usize / size_of::<u16>()];
        let query_status = unsafe {
            RegQueryValueExW(
                key.raw,
                name.as_ptr(),
                ptr::null_mut(),
                &mut data_type,
                data.as_mut_ptr().cast::<u8>(),
                &mut byte_len,
            )
        };
        if query_status != ERROR_SUCCESS {
            return Err(registry_error(
                "query value",
                path,
                Some(value_name),
                query_status,
            ));
        }
        if data_type != REG_SZ {
            return Ok(None);
        }
        if let Some(terminator_index) = data.iter().position(|value| *value == 0) {
            data.truncate(terminator_index);
        }
        Ok(String::from_utf16(&data).ok())
    }

    pub fn write_string_value(
        path: &str,
        value_name: &str,
        value: &str,
    ) -> Result<(), RegistryWriteError> {
        let key = create_key(path)?;
        let name = to_wide_null(value_name);
        let data = to_wide_null(value);
        let byte_len = (data.len() * size_of::<u16>()) as u32;
        let status = unsafe {
            RegSetValueExW(
                key.raw,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast::<u8>(),
                byte_len,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(registry_write_error("set value", path, value_name, status))
        }
    }

    fn open_key(path: &str) -> Result<Option<OwnedRegistryKey>, RegistryReadError> {
        let path_wide = to_wide_null(path);
        let mut key: HKEY = ptr::null_mut();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                path_wide.as_ptr(),
                0,
                KEY_READ,
                &mut key,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(Some(OwnedRegistryKey { raw: key }))
        } else if status == ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else {
            Err(registry_error("open key", path, None, status))
        }
    }

    fn create_key(path: &str) -> Result<OwnedRegistryKey, RegistryWriteError> {
        let path_wide = to_wide_null(path);
        let mut key: HKEY = ptr::null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                path_wide.as_ptr(),
                0,
                ptr::null_mut(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                ptr::null(),
                &mut key,
                ptr::null_mut(),
            )
        };
        if status == ERROR_SUCCESS {
            Ok(OwnedRegistryKey { raw: key })
        } else {
            Err(registry_write_error("create key", path, "", status))
        }
    }

    struct OwnedRegistryKey {
        raw: HKEY,
    }

    impl Drop for OwnedRegistryKey {
        fn drop(&mut self) {
            if !self.raw.is_null() {
                unsafe {
                    let _ = RegCloseKey(self.raw);
                }
            }
        }
    }

    fn registry_error(
        operation: &'static str,
        path: &str,
        value_name: Option<&str>,
        code: WIN32_ERROR,
    ) -> RegistryReadError {
        RegistryReadError {
            operation,
            path: path.to_owned(),
            value_name: value_name.map(str::to_owned),
            code,
        }
    }

    fn registry_write_error(
        operation: &'static str,
        path: &str,
        value_name: &str,
        code: WIN32_ERROR,
    ) -> RegistryWriteError {
        RegistryWriteError {
            operation,
            path: path.to_owned(),
            value_name: value_name.to_owned(),
            code,
        }
    }

    fn to_wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(windows))]
mod registry {
    use super::{RegistryReadError, RegistryWriteError};

    pub fn key_exists(_path: &str) -> Result<bool, RegistryReadError> {
        Ok(false)
    }

    pub fn read_string_value(
        _path: &str,
        _value_name: &str,
    ) -> Result<Option<String>, RegistryReadError> {
        Ok(None)
    }

    pub fn write_string_value(
        path: &str,
        value_name: &str,
        _value: &str,
    ) -> Result<(), RegistryWriteError> {
        Err(RegistryWriteError {
            operation: "set value",
            path: path.to_owned(),
            value_name: value_name.to_owned(),
            code: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque};

    use super::*;

    #[derive(Clone)]
    struct FakeStatusSource {
        service: Result<Option<ObservedServiceStatus>, ControlStatusError>,
        provider: Result<ObservedProviderRegistryStatus, ControlStatusError>,
        service_config: Result<ObservedServiceConfigStatus, ControlStatusError>,
        data_directory: Result<DataDirectorySummary, ControlStatusError>,
        presence_runtime: Result<Option<PresenceRuntimeSummary>, ControlStatusError>,
    }

    impl Default for FakeStatusSource {
        fn default() -> Self {
            Self {
                service: Ok(Some(ObservedServiceStatus {
                    runtime_state: ServiceRuntimeState::Running,
                    process_id: Some(42),
                })),
                provider: Ok(ObservedProviderRegistryStatus {
                    credential_provider_registered: true,
                    com_server_registered: true,
                    project_config_registered: true,
                }),
                service_config: Ok(ObservedServiceConfigStatus {
                    registry_config_exists: true,
                    auth_mode: Some("local-camera".to_owned()),
                    face_template_path: Some(r"C:\ProgramData\WinFaceUnlock\faces.json".to_owned()),
                    presence_lock_enabled: Some("true".to_owned()),
                    intruder_snap_enabled: Some("true".to_owned()),
                    presence_detector_kind: Some("person".to_owned()),
                    presence_tracking_mode: Some("owner-face".to_owned()),
                }),
                data_directory: Ok(DataDirectorySummary {
                    program_data_dir: Some(r"C:\ProgramData\WinFaceUnlock".to_owned()),
                    program_data_presence: PathPresence::Present,
                    presence_audit_dir: Some(
                        r"C:\ProgramData\WinFaceUnlock\presence-audit".to_owned(),
                    ),
                    presence_audit_presence: PathPresence::Present,
                }),
                presence_runtime: Ok(Some(PresenceRuntimeSummary {
                    monitor_state: PresenceMonitorState::Running,
                    session_id: Some(1),
                    reason: Some("monitor_running".to_owned()),
                    updated_at_unix_ms: Some(1_782_000_000_000),
                })),
            }
        }
    }

    impl StatusSource for FakeStatusSource {
        fn query_service_status(
            &self,
        ) -> Result<Option<ObservedServiceStatus>, ControlStatusError> {
            self.service.clone()
        }

        fn query_provider_registry_status(
            &self,
        ) -> Result<ObservedProviderRegistryStatus, ControlStatusError> {
            self.provider.clone()
        }

        fn query_service_config_status(
            &self,
        ) -> Result<ObservedServiceConfigStatus, ControlStatusError> {
            self.service_config.clone()
        }

        fn query_data_directory_status(&self) -> Result<DataDirectorySummary, ControlStatusError> {
            self.data_directory.clone()
        }

        fn query_presence_runtime_status(
            &self,
        ) -> Result<Option<PresenceRuntimeSummary>, ControlStatusError> {
            self.presence_runtime.clone()
        }
    }

    #[derive(Default)]
    struct FakeSettingsSource {
        presence_lock_enabled: RefCell<Option<String>>,
        intruder_snap_enabled: RefCell<Option<String>>,
        logon_wake_mode: RefCell<Option<String>>,
        auto_wake_on_advise: RefCell<Option<String>>,
        logon_face_match_threshold: RefCell<Option<String>>,
    }

    impl SettingsSource for FakeSettingsSource {
        fn read_presence_lock_enabled(&self) -> Result<Option<String>, ControlStatusError> {
            Ok(self.presence_lock_enabled.borrow().clone())
        }

        fn write_presence_lock_enabled(&self, enabled: bool) -> Result<(), ControlStatusError> {
            self.presence_lock_enabled
                .replace(Some(bool_registry_value(enabled).to_owned()));
            Ok(())
        }

        fn read_intruder_snap_enabled(&self) -> Result<Option<String>, ControlStatusError> {
            Ok(self.intruder_snap_enabled.borrow().clone())
        }

        fn write_intruder_snap_enabled(&self, enabled: bool) -> Result<(), ControlStatusError> {
            self.intruder_snap_enabled
                .replace(Some(bool_registry_value(enabled).to_owned()));
            Ok(())
        }

        fn read_logon_wake_mode(&self) -> Result<Option<String>, ControlStatusError> {
            Ok(self.logon_wake_mode.borrow().clone())
        }

        fn read_auto_wake_on_advise(&self) -> Result<Option<String>, ControlStatusError> {
            Ok(self.auto_wake_on_advise.borrow().clone())
        }

        fn write_logon_wake_mode(&self, mode: LogonWakeMode) -> Result<(), ControlStatusError> {
            self.logon_wake_mode
                .replace(Some(logon_wake_mode_registry_value(mode).to_owned()));
            self.auto_wake_on_advise
                .replace(Some(bool_registry_value(true).to_owned()));
            Ok(())
        }

        fn read_logon_face_match_threshold(&self) -> Result<Option<String>, ControlStatusError> {
            Ok(self.logon_face_match_threshold.borrow().clone())
        }

        fn write_logon_face_match_threshold(
            &self,
            threshold: f32,
        ) -> Result<(), ControlStatusError> {
            self.logon_face_match_threshold
                .replace(Some(format!("{threshold:.3}")));
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeFaceTemplateConfigWriter {
        local_camera_auth_config: RefCell<Option<(PathBuf, String, PathBuf)>>,
    }

    impl FaceTemplateConfigWriter for FakeFaceTemplateConfigWriter {
        fn write_active_face_template_path(
            &self,
            _template_path: &Path,
        ) -> Result<(), FaceTemplateStatusError> {
            Ok(())
        }

        fn write_local_camera_auth_config(
            &self,
            template_path: &Path,
            camera_id: &str,
            install_dir: &Path,
        ) -> Result<(), FaceTemplateStatusError> {
            *self.local_camera_auth_config.borrow_mut() = Some((
                template_path.to_path_buf(),
                camera_id.to_owned(),
                install_dir.to_path_buf(),
            ));
            Ok(())
        }
    }

    #[test]
    fn service_missing_maps_to_distinct_service_states() -> Result<(), ControlStatusError> {
        let source = FakeStatusSource {
            service: Ok(None),
            ..FakeStatusSource::default()
        };

        let status = load_dashboard_status(&source)?;

        assert_eq!(
            status.service.installation_state,
            ServiceInstallationState::Missing
        );
        assert_eq!(status.service.runtime_state, ServiceRuntimeState::Missing);
        assert_eq!(status.service.process_id, None);
        Ok(())
    }

    #[test]
    fn provider_partially_registered_keeps_layer_details() -> Result<(), ControlStatusError> {
        let source = FakeStatusSource {
            provider: Ok(ObservedProviderRegistryStatus {
                credential_provider_registered: true,
                com_server_registered: false,
                project_config_registered: true,
            }),
            ..FakeStatusSource::default()
        };

        let status = load_dashboard_status(&source)?;

        assert_eq!(
            status.provider.registration_state,
            ProviderRegistrationState::PartiallyRegistered
        );
        assert!(status.provider.credential_provider_registered);
        assert!(!status.provider.com_server_registered);
        assert!(status.provider.project_config_registered);
        Ok(())
    }

    #[test]
    fn registry_config_missing_maps_without_losing_provider_state() -> Result<(), ControlStatusError>
    {
        let source = FakeStatusSource {
            service_config: Ok(ObservedServiceConfigStatus {
                registry_config_exists: false,
                auth_mode: None,
                face_template_path: None,
                presence_lock_enabled: None,
                intruder_snap_enabled: None,
                presence_detector_kind: None,
                presence_tracking_mode: None,
            }),
            ..FakeStatusSource::default()
        };

        let status = load_dashboard_status(&source)?;

        assert_eq!(
            status.service_config.registry_config_state,
            RegistryConfigState::Missing
        );
        assert_eq!(status.service_config.auth_mode, None);
        assert_eq!(
            status.provider.registration_state,
            ProviderRegistrationState::Registered
        );
        Ok(())
    }

    #[test]
    fn data_directory_missing_keeps_both_path_presence_values() -> Result<(), ControlStatusError> {
        let source = FakeStatusSource {
            data_directory: Ok(DataDirectorySummary {
                program_data_dir: Some(r"C:\ProgramData\WinFaceUnlock".to_owned()),
                program_data_presence: PathPresence::Missing,
                presence_audit_dir: Some(r"C:\ProgramData\WinFaceUnlock\presence-audit".to_owned()),
                presence_audit_presence: PathPresence::Missing,
            }),
            ..FakeStatusSource::default()
        };

        let status = load_dashboard_status(&source)?;

        assert_eq!(
            status.data_directory.program_data_presence,
            PathPresence::Missing
        );
        assert_eq!(
            status.data_directory.presence_audit_presence,
            PathPresence::Missing
        );
        Ok(())
    }

    #[test]
    fn provider_status_failure_returns_provider_specific_error() -> Result<(), &'static str> {
        let source = FakeStatusSource {
            provider: Err(ControlStatusError::ProviderStatusUnavailable(
                "provider registry status cannot be read".to_owned(),
            )),
            ..FakeStatusSource::default()
        };

        let error = match load_dashboard_status(&source) {
            Ok(_) => {
                return Err("provider failure should surface");
            }
            Err(error) => error,
        };

        assert!(matches!(
            error,
            ControlStatusError::ProviderStatusUnavailable(_)
        ));
        assert!(error.message().contains("provider registry"));
        Ok(())
    }

    #[test]
    fn face_template_list_reads_active_service_template_summary()
    -> Result<(), Box<dyn std::error::Error>> {
        let template_path = unique_temp_path("selected_templates.json");
        let avatar_path = template_path.with_file_name(PREVIEW_FRAME_FILE_NAME);
        fs::write(
            &template_path,
            r#"{
                "user_id": "dev-user",
                "recognizer_model_family": "opencv_sface",
                "recognizer_model_version": "2021dec",
                "enrollment_created_at_unix_ms": 1782000000000,
                "quality_summary": {
                    "selected_template_count": 2,
                    "rejected_sample_count": 1
                },
                "templates": [
                    { "selected_for_unlock": true },
                    { "selected_for_unlock": true }
                ]
            }"#,
        )?;
        fs::write(&avatar_path, [0xff, 0xd8, 0xff, 0xd9])?;
        let source = FakeStatusSource {
            service_config: Ok(ObservedServiceConfigStatus {
                registry_config_exists: true,
                auth_mode: Some("local-camera".to_owned()),
                face_template_path: Some(template_path.display().to_string()),
                presence_lock_enabled: Some("true".to_owned()),
                intruder_snap_enabled: Some("true".to_owned()),
                presence_detector_kind: None,
                presence_tracking_mode: None,
            }),
            ..FakeStatusSource::default()
        };

        let templates = load_face_template_list(&source)?;
        let _ = fs::remove_file(template_path);
        let _ = fs::remove_file(avatar_path);
        let summary = templates
            .templates
            .first()
            .ok_or("expected one active service template summary")?;

        assert_eq!(templates.templates.len(), 1);
        assert_eq!(summary.face_template_ref, ACTIVE_SERVICE_FACE_TEMPLATE_REF);
        assert_eq!(summary.user_id, "dev-user");
        assert_eq!(summary.display_name.as_deref(), Some("用户1"));
        let avatar_preview = summary
            .avatar_preview
            .as_ref()
            .ok_or("expected preview frame avatar")?;
        assert_eq!(avatar_preview.mime_type, "image/jpeg");
        assert_eq!(avatar_preview.image_base64, "/9j/2Q==");
        assert!(avatar_preview.updated_at_unix_ms.is_some());
        assert_eq!(summary.selected_template_count, 2);
        assert_eq!(summary.rejected_sample_count, Some(1));
        assert_eq!(summary.recognition_model.model_family, "opencv_sface");
        assert_eq!(summary.recognition_model.model_version, "2021dec");
        assert_eq!(
            summary.source_state,
            FaceTemplateSourceState::ActiveServiceTemplate
        );
        Ok(())
    }

    #[test]
    fn apply_local_camera_auth_config_passes_template_camera_and_install_dir()
    -> Result<(), Box<dyn std::error::Error>> {
        let writer = FakeFaceTemplateConfigWriter::default();
        let template_path = PathBuf::from(r"D:\tools\WinFaceUnlock\face-enrollment\selected.json");
        let install_dir = PathBuf::from(r"D:\tools\WinFaceUnlock");

        apply_local_camera_auth_config(&writer, &template_path, " opencv-index:1 ", &install_dir)?;

        assert_eq!(
            *writer.local_camera_auth_config.borrow(),
            Some((template_path, "opencv-index:1".to_owned(), install_dir))
        );
        Ok(())
    }

    #[test]
    fn face_template_list_distinguishes_missing_template_config() -> Result<(), &'static str> {
        let source = FakeStatusSource {
            service_config: Ok(ObservedServiceConfigStatus {
                registry_config_exists: true,
                auth_mode: Some("local-camera".to_owned()),
                face_template_path: None,
                presence_lock_enabled: Some("true".to_owned()),
                intruder_snap_enabled: Some("true".to_owned()),
                presence_detector_kind: None,
                presence_tracking_mode: None,
            }),
            ..FakeStatusSource::default()
        };

        let error = match load_face_template_list(&source) {
            Ok(_) => return Err("missing face template config should fail distinctly"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            FaceTemplateStatusError::TemplateConfigMissing(_)
        ));
        Ok(())
    }

    #[test]
    fn presence_runtime_file_state_maps_to_protocol_state() {
        let status = PresenceRuntimeStatusFile {
            state: "disabled".to_owned(),
            session_id: None,
            reason: "presence_lock_disabled".to_owned(),
            updated_at_unix_ms: 100,
        }
        .into_summary();

        assert_eq!(status.monitor_state, PresenceMonitorState::Disabled);
        assert_eq!(status.reason.as_deref(), Some("presence_lock_disabled"));
    }

    #[test]
    fn bool_registry_values_accept_setup_and_windows_spellings() {
        let values = VecDeque::from(["true", "1", "enabled", "false", "0", "disabled"]);

        assert_eq!(
            values
                .iter()
                .map(|value| parse_registry_bool(value))
                .collect::<Vec<_>>(),
            vec![
                Some(true),
                Some(true),
                Some(true),
                Some(false),
                Some(false),
                Some(false)
            ]
        );
    }

    #[test]
    fn settings_default_presence_lock_to_false_when_registry_value_is_missing()
    -> Result<(), ControlStatusError> {
        let source = FakeSettingsSource::default();

        let settings = load_control_settings(&source)?;

        assert!(!settings.presence_lock_enabled);
        Ok(())
    }

    #[test]
    fn settings_update_writes_presence_lock_and_returns_snapshot() -> Result<(), ControlStatusError>
    {
        let source = FakeSettingsSource {
            presence_lock_enabled: RefCell::new(Some("true".to_owned())),
            ..FakeSettingsSource::default()
        };

        let settings = update_control_settings(
            &source,
            &ControlSettingsPatch {
                presence_lock_enabled: Some(false),
                intruder_snap_enabled: None,
                logon_wake_mode: None,
                logon_face_match_threshold: None,
            },
        )?;

        assert!(!settings.presence_lock_enabled);
        assert_eq!(
            source.presence_lock_enabled.borrow().as_deref(),
            Some("false")
        );
        Ok(())
    }

    #[test]
    fn settings_reads_triggered_recognition_logon_wake_mode() -> Result<(), ControlStatusError> {
        let source = FakeSettingsSource {
            logon_wake_mode: RefCell::new(Some(LOGON_WAKE_MODE_TRIGGERED_RECOGNITION.to_owned())),
            ..FakeSettingsSource::default()
        };

        let settings = load_control_settings(&source)?;

        assert_eq!(
            settings.logon_wake_mode,
            Some(LogonWakeMode::TriggeredRecognition)
        );
        Ok(())
    }

    #[test]
    fn settings_maps_legacy_hybrid_to_background_silent_recognition()
    -> Result<(), ControlStatusError> {
        let source = FakeSettingsSource {
            logon_wake_mode: RefCell::new(Some(LOGON_WAKE_MODE_HYBRID.to_owned())),
            ..FakeSettingsSource::default()
        };

        let settings = load_control_settings(&source)?;

        assert_eq!(
            settings.logon_wake_mode,
            Some(LogonWakeMode::BackgroundSilentRecognition)
        );
        Ok(())
    }

    #[test]
    fn settings_derives_input_triggered_from_legacy_auto_wake() -> Result<(), ControlStatusError> {
        let source = FakeSettingsSource {
            auto_wake_on_advise: RefCell::new(Some("true".to_owned())),
            ..FakeSettingsSource::default()
        };

        let settings = load_control_settings(&source)?;

        assert_eq!(
            settings.logon_wake_mode,
            Some(LogonWakeMode::TriggeredRecognition)
        );
        Ok(())
    }

    #[test]
    fn settings_update_writes_logon_wake_mode_and_legacy_auto_wake()
    -> Result<(), ControlStatusError> {
        let source = FakeSettingsSource::default();

        let settings = update_control_settings(
            &source,
            &ControlSettingsPatch {
                presence_lock_enabled: None,
                intruder_snap_enabled: None,
                logon_wake_mode: Some(LogonWakeMode::TriggeredRecognition),
                logon_face_match_threshold: None,
            },
        )?;

        assert_eq!(
            settings.logon_wake_mode,
            Some(LogonWakeMode::TriggeredRecognition)
        );
        assert_eq!(
            source.logon_wake_mode.borrow().as_deref(),
            Some(LOGON_WAKE_MODE_TRIGGERED_RECOGNITION)
        );
        assert_eq!(source.auto_wake_on_advise.borrow().as_deref(), Some("true"));
        Ok(())
    }

    #[test]
    fn settings_default_logon_face_threshold_when_registry_value_is_missing()
    -> Result<(), ControlStatusError> {
        let source = FakeSettingsSource::default();

        let settings = load_control_settings(&source)?;

        assert_eq!(
            settings.logon_face_match_threshold,
            DEFAULT_LOGON_FACE_MATCH_THRESHOLD
        );
        Ok(())
    }

    #[test]
    fn settings_reads_logon_face_threshold_from_registry() -> Result<(), ControlStatusError> {
        let source = FakeSettingsSource {
            logon_face_match_threshold: RefCell::new(Some("0.52".to_owned())),
            ..FakeSettingsSource::default()
        };

        let settings = load_control_settings(&source)?;

        assert_eq!(settings.logon_face_match_threshold, 0.52);
        Ok(())
    }

    #[test]
    fn settings_update_writes_logon_face_threshold() -> Result<(), ControlStatusError> {
        let source = FakeSettingsSource::default();

        let settings = update_control_settings(
            &source,
            &ControlSettingsPatch {
                presence_lock_enabled: None,
                intruder_snap_enabled: None,
                logon_wake_mode: None,
                logon_face_match_threshold: Some(0.50),
            },
        )?;

        assert_eq!(settings.logon_face_match_threshold, 0.50);
        assert_eq!(
            source.logon_face_match_threshold.borrow().as_deref(),
            Some("0.500")
        );
        Ok(())
    }

    #[test]
    fn settings_update_rejects_out_of_range_logon_face_threshold() -> Result<(), &'static str> {
        let source = FakeSettingsSource::default();

        let error = match update_control_settings(
            &source,
            &ControlSettingsPatch {
                presence_lock_enabled: None,
                intruder_snap_enabled: None,
                logon_wake_mode: None,
                logon_face_match_threshold: Some(0.10),
            },
        ) {
            Ok(_) => return Err("out-of-range threshold should be rejected"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            ControlStatusError::SettingsPersistenceFailed(_)
        ));
        Ok(())
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "winfaceunlock-control-status-{}-{}-{name}",
            std::process::id(),
            nanos
        ))
    }
}
