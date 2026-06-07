use common_protocol::{AccountType, CredentialRef, ProtocolError, UserId};
use control_backend::{
    ControlBackendError, ControlHandler, WindowsCredentialEnrollmentStore, WindowsCredentialSecret,
};
use control_protocol::{
    ControlRequestEnvelope, ControlResponseEnvelope, WindowsCredentialAccountType,
    WindowsCredentialEnrollmentOutcome, WindowsCredentialEnrollmentPayload,
};
use control_status::{WindowsControlSettingsStore, WindowsDashboardStatusProvider};
use tauri::{Manager, PhysicalPosition, PhysicalSize};
use win_service::credential_store_config::{
    enroll_windows_credential, ServiceCredentialStorePaths, WindowsCredentialEnrollment,
};

#[derive(Clone, Copy)]
struct WindowsCredentialEnrollmentAdapter;

impl WindowsCredentialEnrollmentStore for WindowsCredentialEnrollmentAdapter {
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
            user_id: payload.user_id.clone(),
            user_sid: payload.user_sid.clone(),
            account_type: payload.account_type,
            credential_ref,
        })
    }
}

#[tauri::command]
fn handle_control_request(request: ControlRequestEnvelope) -> ControlResponseEnvelope {
    let handler = ControlHandler::new(
        WindowsDashboardStatusProvider::from_environment_or_default(),
        WindowsControlSettingsStore::new(),
        WindowsCredentialEnrollmentAdapter,
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
    configure_webview2_low_memory_mode();

    tauri::Builder::default()
        .setup(|app| {
            fit_main_window_to_monitor(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            handle_control_request,
            handle_credential_enrollment_request
        ])
        .run(tauri::generate_context!())
        .expect("error while running WinFaceUnlock control panel");
}
