use common_protocol::{AccountType, CredentialRef, ProtocolError, UserId};
use control_backend::{
    CommandFaceEnrollmentRuntime, ControlBackendError, ControlHandler,
    DiagnosticsCliEnrollmentProcessFactory, FaceEnrollmentPreviewEventSink,
    NamedPipeFaceAuthServiceClient, ServiceFaceAuthSelfTestRunner,
    ServiceIpcControlSettingsStore, ServiceIpcFaceEnrollmentTemplateApplier,
    WindowsCredentialEnrollmentStore, WindowsCredentialSecret,
};
use control_protocol::{
    ControlRequestEnvelope, ControlResponseEnvelope, WindowsCredentialAccountProfile,
    WindowsCredentialAccountType, WindowsCredentialEnrollmentOutcome, WindowsCredentialSecretState,
    WindowsCredentialEnrollmentPayload,
};
use control_status::{
    WindowsControlSettingsStore, WindowsDashboardStatusProvider, WindowsFaceTemplateStatusStore,
};
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize};
use win_service::credential_store_config::{
    enroll_windows_credential, is_credential_secret_configured, ServiceCredentialStorePaths,
    WindowsCredentialEnrollment,
};

#[derive(Clone, Copy)]
struct WindowsCredentialEnrollmentAdapter;

impl WindowsCredentialEnrollmentStore for WindowsCredentialEnrollmentAdapter {
    fn load_windows_credential_account(
        &self,
    ) -> Result<WindowsCredentialAccountProfile, ControlBackendError> {
        current_windows_credential_account()
    }

    fn enroll_windows_credential(
        &self,
        payload: &WindowsCredentialEnrollmentPayload,
        password_secret: WindowsCredentialSecret,
    ) -> Result<WindowsCredentialEnrollmentOutcome, ControlBackendError> {
        let username = resolve_windows_account_username(payload)?;
        let credential_ref = payload.resolved_credential_ref();
        let store_paths = ServiceCredentialStorePaths::from_environment_or_default();

        enroll_windows_credential(
            &store_paths,
            WindowsCredentialEnrollment {
                user_id: UserId(payload.user_id.clone()),
                user_sid: payload.user_sid.clone(),
                username: username.clone(),
                account_type: account_type_from_control(payload.account_type),
                credential_ref: CredentialRef(credential_ref.clone()),
                password: password_secret.into_password(),
            },
        )
        .map_err(credential_protocol_error_to_control_error)?;

        Ok(WindowsCredentialEnrollmentOutcome {
            windows_account_username: username,
            display_name: None,
            user_id: payload.user_id.clone(),
            user_sid: payload.user_sid.clone(),
            account_type: payload.account_type,
            credential_ref,
            credential_secret_state: WindowsCredentialSecretState::Configured,
        })
    }
}

#[derive(Clone)]
struct TauriFaceEnrollmentPreviewEventSink {
    app_handle: tauri::AppHandle,
}

impl TauriFaceEnrollmentPreviewEventSink {
    fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }
}

impl FaceEnrollmentPreviewEventSink for TauriFaceEnrollmentPreviewEventSink {
    fn emit_preview_frame(&self, event_json: &str) {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(event_json) else {
            return;
        };
        let _ = self
            .app_handle
            .emit("winfaceunlock://face-enrollment/preview-frame", event);
    }
}

struct ControlRuntimeState {
    face_enrollment_runtime:
        CommandFaceEnrollmentRuntime<DiagnosticsCliEnrollmentProcessFactory<TauriFaceEnrollmentPreviewEventSink>>,
    camera_discovery_provider: DiagnosticsCliEnrollmentProcessFactory<TauriFaceEnrollmentPreviewEventSink>,
}

impl ControlRuntimeState {
    fn from_app_handle(app_handle: tauri::AppHandle) -> Self {
        let default_runtime = DiagnosticsCliEnrollmentProcessFactory::from_environment_or_default();
        let diagnostics_runtime = DiagnosticsCliEnrollmentProcessFactory::with_preview_event_sink(
            default_runtime.diagnostics_cli_path().to_path_buf(),
            default_runtime.enrollment_root_dir().to_path_buf(),
            TauriFaceEnrollmentPreviewEventSink::new(app_handle),
        );
        Self {
            face_enrollment_runtime: CommandFaceEnrollmentRuntime::with_template_applier(
                diagnostics_runtime.clone(),
                ServiceIpcFaceEnrollmentTemplateApplier::default_named_pipe(),
            ),
            camera_discovery_provider: diagnostics_runtime,
        }
    }
}

const DEFAULT_CONTROL_USER_ID: &str = "dev-user";
#[cfg(not(windows))]
const DEFAULT_CONTROL_USER_SID: &str = "S-1-5-21-winfaceunlock-pending";
const ARG_CONTROL_DIAGNOSTICS_CLI: &str = "--winfaceunlock-control-diagnostics-cli";
const ARG_CONTROL_FACE_ENROLLMENT_OUTPUT_DIR: &str =
    "--winfaceunlock-control-face-enrollment-output-dir";
const ARG_CONTROL_YUNET_MODEL_PATH: &str = "--winfaceunlock-control-yunet-model-path";
const ARG_CONTROL_SFACE_MODEL_PATH: &str = "--winfaceunlock-control-sface-model-path";
const ENV_CONTROL_INSTALL_DIR: &str = "WINFACEUNLOCK_INSTALL_DIR";
const ENV_CONTROL_DIAGNOSTICS_CLI: &str = "WINFACEUNLOCK_DIAGNOSTICS_CLI";
const ENV_CONTROL_FACE_ENROLLMENT_OUTPUT_DIR: &str = "WINFACEUNLOCK_FACE_ENROLLMENT_OUTPUT_DIR";
const ENV_YUNET_MODEL_PATH: &str = "WINFACEUNLOCK_YUNET_MODEL_PATH";
const ENV_SFACE_MODEL_PATH: &str = "WINFACEUNLOCK_SFACE_MODEL_PATH";

#[tauri::command]
fn handle_control_request(
    runtime_state: tauri::State<'_, ControlRuntimeState>,
    request: ControlRequestEnvelope,
) -> ControlResponseEnvelope {
    let handler = ControlHandler::with_face_dependencies(
        WindowsDashboardStatusProvider::from_environment_or_default(),
        ServiceIpcControlSettingsStore::default_named_pipe(),
        WindowsCredentialEnrollmentAdapter,
        WindowsFaceTemplateStatusStore::from_environment_or_default(),
        runtime_state.face_enrollment_runtime.clone(),
        runtime_state.camera_discovery_provider.clone(),
        ServiceFaceAuthSelfTestRunner::new(NamedPipeFaceAuthServiceClient::default()),
    );
    handler.handle_request(request)
}

#[tauri::command]
fn handle_credential_enrollment_request(
    request: ControlRequestEnvelope,
    password_secret: String,
) -> ControlResponseEnvelope {
    let handler = ControlHandler::new(
        WindowsDashboardStatusProvider::from_environment_or_default(),
        WindowsControlSettingsStore::new(),
        WindowsCredentialEnrollmentAdapter,
    );
    handler.handle_windows_credential_enrollment_request(
        request,
        WindowsCredentialSecret::from_password(password_secret),
    )
}

fn resolve_windows_account_username(
    payload: &WindowsCredentialEnrollmentPayload,
) -> Result<String, ControlBackendError> {
    if let Some(username) = payload
        .windows_account_username
        .as_deref()
        .map(str::trim)
        .filter(|username| !username.is_empty())
    {
        return Ok(username.to_owned());
    }

    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .map(|username| username.trim().to_owned())
        .ok()
        .filter(|username| !username.is_empty())
        .ok_or_else(|| {
            ControlBackendError::credential_enrollment_unavailable(
                "current Windows account username is unavailable",
            )
        })
}

fn current_windows_credential_account(
) -> Result<WindowsCredentialAccountProfile, ControlBackendError> {
    let username = current_windows_account_username()?;
    let user_sid = current_windows_user_sid().map_err(|message| {
        ControlBackendError::credential_account_unavailable(format!(
            "current Windows account SID is unavailable: {message}"
        ))
    })?;
    let user_id = DEFAULT_CONTROL_USER_ID.to_owned();

    let credential_ref = format!("windows-credential-{user_id}");
    let store_paths = ServiceCredentialStorePaths::from_environment_or_default();
    let credential_secret_state = if is_credential_secret_configured(
        &store_paths,
        &UserId(user_id.clone()),
        &CredentialRef(credential_ref.clone()),
    )
    .map_err(credential_protocol_error_to_control_error)?
    {
        WindowsCredentialSecretState::Configured
    } else {
        WindowsCredentialSecretState::NotConfigured
    };

    Ok(WindowsCredentialAccountProfile {
        windows_account_username: username,
        display_name: None,
        user_id: user_id.clone(),
        user_sid,
        account_type: WindowsCredentialAccountType::Local,
        credential_ref,
        credential_secret_state,
    })
}

fn current_windows_account_username() -> Result<String, ControlBackendError> {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .map(|username| username.trim().to_owned())
        .ok()
        .filter(|username| !username.is_empty())
        .ok_or_else(|| {
            ControlBackendError::credential_account_unavailable(
                "current Windows account username is unavailable",
            )
        })
}

#[cfg(windows)]
fn current_windows_user_sid() -> Result<String, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree, HANDLE},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
            TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    struct TokenHandle(HANDLE);

    impl Drop for TokenHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let token = TokenHandle(token);

        let mut required_len = 0_u32;
        let _ = GetTokenInformation(
            token.0,
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut required_len,
        );
        if required_len == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }

        let mut buffer = vec![0_u8; required_len as usize];
        if GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required_len,
            &mut required_len,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }

        let token_user = buffer.as_ptr().cast::<TOKEN_USER>();
        let sid = (*token_user).User.Sid;
        let mut sid_text_ptr = std::ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut sid_text_ptr) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let sid_text = wide_ptr_to_string(sid_text_ptr);
        let _ = LocalFree(sid_text_ptr.cast());

        sid_text.ok_or_else(|| "Windows returned an invalid SID string".to_owned())
    }
}

#[cfg(not(windows))]
fn current_windows_user_sid() -> Result<String, String> {
    Ok(DEFAULT_CONTROL_USER_SID.to_owned())
}

#[cfg(windows)]
unsafe fn wide_ptr_to_string(ptr: windows_sys::core::PWSTR) -> Option<String> {
    if ptr.is_null() {
        return None;
    }

    let mut len = 0;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    Some(String::from_utf16_lossy(std::slice::from_raw_parts(
        ptr, len,
    )))
}

fn account_type_from_control(account_type: WindowsCredentialAccountType) -> AccountType {
    match account_type {
        WindowsCredentialAccountType::Local => AccountType::Local,
        WindowsCredentialAccountType::MicrosoftAccount => AccountType::MicrosoftAccount,
        WindowsCredentialAccountType::Domain => AccountType::Domain,
    }
}

fn credential_protocol_error_to_control_error(error: ProtocolError) -> ControlBackendError {
    match error {
        ProtocolError::TransportUnavailable => {
            ControlBackendError::credential_enrollment_unavailable(
                "credential store transport is unavailable",
            )
        }
        ProtocolError::Unauthorized
        | ProtocolError::InvalidMessage
        | ProtocolError::ExpiredGrant
        | ProtocolError::UsedGrant
        | ProtocolError::SessionMismatch => ControlBackendError::credential_enrollment_failed(
            format!("credential store rejected enrollment: {error:?}"),
        ),
    }
}

fn configure_webview2_low_memory_mode() {
    let low_memory_enabled = std::env::var("WINFACEUNLOCK_WEBVIEW2_LOW_MEMORY")
        .map(|value| value == "1")
        .unwrap_or(false);

    if low_memory_enabled && std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "--disable-gpu --disable-background-networking --disable-component-update --disable-extensions --disable-sync --disable-features=Translate,AutofillServerCommunication",
        );
    }
}

fn apply_control_runtime_launch_args() {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let Some(arg) = arg.to_str() else {
            continue;
        };
        let Some(env_name) = control_runtime_env_name_for_arg(arg) else {
            continue;
        };
        let Some(value) = args.next() else {
            continue;
        };
        std::env::set_var(env_name, value);
    }
}

fn default_control_install_dir_from_exe() {
    if std::env::var_os(ENV_CONTROL_INSTALL_DIR).is_some() {
        return;
    }

    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
    else {
        return;
    };

    if exe_dir.join("diagnostics_cli.exe").exists() {
        std::env::set_var(ENV_CONTROL_INSTALL_DIR, exe_dir);
    }
}

fn control_runtime_env_name_for_arg(arg: &str) -> Option<&'static str> {
    match arg {
        ARG_CONTROL_DIAGNOSTICS_CLI => Some(ENV_CONTROL_DIAGNOSTICS_CLI),
        ARG_CONTROL_FACE_ENROLLMENT_OUTPUT_DIR => Some(ENV_CONTROL_FACE_ENROLLMENT_OUTPUT_DIR),
        ARG_CONTROL_YUNET_MODEL_PATH => Some(ENV_YUNET_MODEL_PATH),
        ARG_CONTROL_SFACE_MODEL_PATH => Some(ENV_SFACE_MODEL_PATH),
        _ => None,
    }
}

fn fit_main_window_to_monitor(app: &tauri::App) -> tauri::Result<()> {
    const ASPECT_WIDTH: f64 = 4.0;
    const ASPECT_HEIGHT: f64 = 3.0;
    const TARGET_SCREEN_AREA: f64 = 0.30;
    const MAX_SCREEN_SCALE: f64 = 0.9;
    const MIN_WIDTH: f64 = 560.0;
    const MIN_HEIGHT: f64 = 420.0;

    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) else {
        window.center()?;
        return Ok(());
    };

    let work_area = monitor.work_area();
    let work_width = work_area.size.width as f64;
    let work_height = work_area.size.height as f64;
    let aspect_ratio = ASPECT_WIDTH / ASPECT_HEIGHT;

    let max_width = work_width * MAX_SCREEN_SCALE;
    let max_height = work_height * MAX_SCREEN_SCALE;
    let target_area = work_width * work_height * TARGET_SCREEN_AREA;

    let mut width = (target_area * aspect_ratio)
        .sqrt()
        .clamp(MIN_WIDTH, max_width);
    let mut height = (width / aspect_ratio).clamp(MIN_HEIGHT, max_height);

    if height >= max_height {
        height = max_height;
        width = (height * aspect_ratio).clamp(MIN_WIDTH, max_width);
    }

    let width = width.round() as u32;
    let height = height.round() as u32;
    let x = work_area.position.x + ((work_area.size.width.saturating_sub(width)) / 2) as i32;
    let y = work_area.position.y + ((work_area.size.height.saturating_sub(height)) / 2) as i32;

    window.set_size(PhysicalSize::new(width, height))?;
    window.set_position(PhysicalPosition::new(x, y))?;
    window.set_maximizable(true)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    apply_control_runtime_launch_args();
    default_control_install_dir_from_exe();
    configure_webview2_low_memory_mode();

    tauri::Builder::default()
        .setup(|app| {
            fit_main_window_to_monitor(app)?;
            app.manage(ControlRuntimeState::from_app_handle(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            handle_control_request,
            handle_credential_enrollment_request
        ])
        .run(tauri::generate_context!())
        .expect("error while running WinFaceUnlock control panel");
}
