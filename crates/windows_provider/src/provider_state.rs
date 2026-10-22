use std::{
    fmt,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use common_protocol::{ProtocolError, SessionId};
use logon_safety::{LogonBlock, LogonSubmissionGuard, SubmissionToken, WindowsLogonResult};
use windows::Win32::UI::Shell::{
    CPUS_INVALID, CREDENTIAL_PROVIDER_NO_DEFAULT, CREDENTIAL_PROVIDER_USAGE_SCENARIO,
    ICredentialProviderEvents,
};
use windows_core::AgileReference;

use crate::broker_client::ProviderWakeOutcome;

const WAKE_RETRY_COOLDOWN_MS: i64 = 3_000;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProviderTileVisibility {
    #[default]
    VisibleBeforeCredentialReady,
    HiddenUntilCredentialReady,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CredentialCountPlan {
    pub credential_count: u32,
    pub default_credential_index: u32,
    pub auto_logon_with_default: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WakeRequestBlockReason {
    CredentialAlreadyReady,
    WakeAlreadyRunning,
    RetryCooldownActive,
    WindowsSubmissionFinished,
    AutomaticLogonSuppressed(LogonBlock),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WakeRequestStart {
    Started {
        session_id: SessionId,
        request_id: u64,
    },
    Blocked {
        reason: WakeRequestBlockReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderWakeFailure {
    AuthFailed {
        reason: common_protocol::AuthFailureReason,
    },
    RequestRejected {
        protocol_error: ProtocolError,
    },
    TransportUnavailable {
        protocol_error: ProtocolError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderUnlockPhase {
    ProviderLoaded,
    WakeRequested {
        attempt_number: u32,
        attempt_limit: u32,
    },
    CredentialMaterialReady,
    AwaitingWindowsResult,
    AutomaticLogonSuppressed {
        reason: LogonBlock,
    },
    WakeFailed {
        failure: ProviderWakeFailure,
        attempt_number: u32,
        attempt_limit: u32,
        automatic_retry_pending: bool,
    },
    Completed,
}

#[derive(Debug)]
pub struct UnlockAttemptState {
    pub phase: ProviderUnlockPhase,
    pub credential_material: Option<CredentialMaterial>,
}

impl Default for UnlockAttemptState {
    fn default() -> Self {
        Self {
            phase: ProviderUnlockPhase::ProviderLoaded,
            credential_material: None,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct CredentialMaterial {
    pub domain: String,
    pub username: String,
    pub password: String,
}

impl fmt::Debug for CredentialMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialMaterial")
            .field("domain", &self.domain)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

pub struct ProviderState {
    inner: Mutex<ProviderStateInner>,
    logon_guard: Arc<dyn LogonSubmissionGuard>,
}

struct ProviderStateInner {
    usage_scenario: CREDENTIAL_PROVIDER_USAGE_SCENARIO,
    tile_visibility: ProviderTileVisibility,
    events: Option<AgileReference<ICredentialProviderEvents>>,
    advise_context: usize,
    unlock_attempt: UnlockAttemptState,
    wake_retry_allowed_at_unix_ms: i64,
    wake_request_id: u64,
    credential_generation: Option<u64>,
    submission: Option<SubmissionToken>,
}

impl ProviderState {
    pub fn new() -> Self {
        Self::with_tile_visibility(ProviderTileVisibility::default())
    }

    pub fn with_tile_visibility(tile_visibility: ProviderTileVisibility) -> Self {
        #[cfg(test)]
        let guard = Arc::new(logon_safety::MemoryLogonGuard::default());
        #[cfg(not(test))]
        let guard = Arc::new(logon_safety::PersistentLogonGuard);
        Self::with_guard(tile_visibility, guard)
    }

    fn with_guard(
        tile_visibility: ProviderTileVisibility,
        logon_guard: Arc<dyn LogonSubmissionGuard>,
    ) -> Self {
        Self {
            inner: Mutex::new(ProviderStateInner {
                usage_scenario: CPUS_INVALID,
                tile_visibility,
                events: None,
                advise_context: 0,
                unlock_attempt: UnlockAttemptState::default(),
                wake_retry_allowed_at_unix_ms: 0,
                wake_request_id: 0,
                credential_generation: None,
                submission: None,
            }),
            logon_guard,
        }
    }

    pub fn set_usage_scenario(&self, usage_scenario: CREDENTIAL_PROVIDER_USAGE_SCENARIO) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.usage_scenario = usage_scenario;
        }
    }

    pub fn set_events(&self, events: Option<ICredentialProviderEvents>, advise_context: usize) {
        let has_new_events = events.is_some();
        let should_notify_ready_credential = if let Ok(mut inner) = self.inner.lock() {
            if !has_new_events {
                // An old worker must not publish into a later Advise lifecycle.
                inner.wake_request_id = inner.wake_request_id.saturating_add(1);
                inner.credential_generation = None;
                inner.unlock_attempt.credential_material = None;
                if matches!(
                    inner.unlock_attempt.phase,
                    ProviderUnlockPhase::WakeRequested { .. }
                        | ProviderUnlockPhase::WakeFailed { .. }
                        | ProviderUnlockPhase::CredentialMaterialReady
                ) {
                    inner.unlock_attempt.phase = ProviderUnlockPhase::ProviderLoaded;
                }
            } else if inner.events.is_none()
                && matches!(
                    inner.unlock_attempt.phase,
                    ProviderUnlockPhase::Completed
                        | ProviderUnlockPhase::AutomaticLogonSuppressed { .. }
                )
                && self.logon_guard.ready_generation().is_ok()
            {
                inner.unlock_attempt.phase = ProviderUnlockPhase::ProviderLoaded;
            }
            inner.events = events
                .as_ref()
                .and_then(|event_sink| AgileReference::new(event_sink).ok());
            inner.advise_context = advise_context;
            has_new_events && inner.unlock_attempt.credential_material.is_some()
        } else {
            false
        };
        if should_notify_ready_credential {
            self.notify_credentials_changed();
        }
    }

    pub fn usage_scenario(&self) -> CREDENTIAL_PROVIDER_USAGE_SCENARIO {
        self.inner
            .lock()
            .map(|inner| inner.usage_scenario)
            .unwrap_or(CPUS_INVALID)
    }

    pub fn has_events_sink(&self) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.events.is_some())
            .unwrap_or(false)
    }

    pub fn take_credential_for_submission(&self) -> Option<CredentialMaterial> {
        let mut inner = self.inner.lock().ok()?;
        if inner.unlock_attempt.phase != ProviderUnlockPhase::CredentialMaterialReady {
            return None;
        }
        let material = inner.unlock_attempt.credential_material.take()?;
        let generation = inner.credential_generation?;
        match self.logon_guard.reserve(generation) {
            Ok(token) => {
                inner.submission = Some(token);
                inner.unlock_attempt.phase = ProviderUnlockPhase::AwaitingWindowsResult;
                Some(material)
            }
            Err(reason) => {
                inner.unlock_attempt.phase =
                    ProviderUnlockPhase::AutomaticLogonSuppressed { reason };
                None
            }
        }
    }

    pub fn cancel_unsubmitted_credential(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(token) = inner.submission.take() {
                let _ = self.logon_guard.cancel_unsubmitted(token);
            }
            inner.unlock_attempt.phase = ProviderUnlockPhase::Completed;
        }
    }

    pub fn credential_count_plan(&self) -> CredentialCountPlan {
        self.inner
            .lock()
            .map(|inner| {
                if inner.unlock_attempt.credential_material.is_some() {
                    CredentialCountPlan {
                        credential_count: 1,
                        default_credential_index: 0,
                        auto_logon_with_default: true,
                    }
                } else if inner.tile_visibility
                    == ProviderTileVisibility::HiddenUntilCredentialReady
                {
                    CredentialCountPlan {
                        credential_count: 0,
                        default_credential_index: CREDENTIAL_PROVIDER_NO_DEFAULT,
                        auto_logon_with_default: false,
                    }
                } else {
                    CredentialCountPlan {
                        credential_count: 1,
                        default_credential_index: CREDENTIAL_PROVIDER_NO_DEFAULT,
                        auto_logon_with_default: false,
                    }
                }
            })
            .unwrap_or(CredentialCountPlan {
                credential_count: 0,
                default_credential_index: CREDENTIAL_PROVIDER_NO_DEFAULT,
                auto_logon_with_default: false,
            })
    }

    pub fn credential_status_message(&self) -> String {
        self.inner
            .lock()
            .map(|inner| match &inner.unlock_attempt.phase {
                ProviderUnlockPhase::ProviderLoaded => {
                    "Select WinFaceUnlock to try face sign-in".to_owned()
                }
                ProviderUnlockPhase::WakeRequested {
                    attempt_number,
                    attempt_limit,
                } => format!(
                    "Local face authentication is running ({attempt_number}/{attempt_limit})"
                ),
                ProviderUnlockPhase::CredentialMaterialReady => {
                    "Face authentication credential is ready".to_owned()
                }
                ProviderUnlockPhase::AwaitingWindowsResult => "Waiting for Windows sign-in result; automatic resubmission is paused".to_owned(),
                ProviderUnlockPhase::AutomaticLogonSuppressed { .. } => "Automatic face sign-in is paused. Use Windows PIN/password, then save your Windows credential again in WinFaceUnlock".to_owned(),
                ProviderUnlockPhase::WakeFailed {
                    failure: ProviderWakeFailure::AuthFailed { .. },
                    attempt_number,
                    attempt_limit,
                    automatic_retry_pending: true,
                } => format!(
                    "Face authentication failed. Retrying automatically ({attempt_number}/{attempt_limit})"
                ),
                ProviderUnlockPhase::WakeFailed {
                    failure: ProviderWakeFailure::AuthFailed { .. },
                    ..
                } => {
                    "Face authentication failed. Use PIN or password fallback".to_owned()
                }
                ProviderUnlockPhase::WakeFailed {
                    failure: ProviderWakeFailure::RequestRejected { .. },
                    ..
                } => "Face sign-in request was rejected. Use PIN or password fallback".to_owned(),
                ProviderUnlockPhase::WakeFailed {
                    failure: ProviderWakeFailure::TransportUnavailable { .. },
                    ..
                } => "Face service is unavailable. Use PIN or password fallback".to_owned(),
                ProviderUnlockPhase::Completed => "Face authentication completed".to_owned(),
            })
            .unwrap_or_else(|_| "Select WinFaceUnlock to try face sign-in".to_owned())
    }

    pub fn begin_wake_request(&self, attempt_limit: u32) -> WakeRequestStart {
        self.inner
            .lock()
            .map(|mut inner| {
                if matches!(
                    inner.unlock_attempt.phase,
                    ProviderUnlockPhase::AwaitingWindowsResult
                        | ProviderUnlockPhase::Completed
                        | ProviderUnlockPhase::AutomaticLogonSuppressed { .. }
                ) {
                    return WakeRequestStart::Blocked {
                        reason: WakeRequestBlockReason::WindowsSubmissionFinished,
                    };
                }
                let generation = match self.logon_guard.ready_generation() {
                    Ok(generation) => generation,
                    Err(reason) => {
                        inner.unlock_attempt.phase =
                            ProviderUnlockPhase::AutomaticLogonSuppressed { reason };
                        return WakeRequestStart::Blocked {
                            reason: WakeRequestBlockReason::AutomaticLogonSuppressed(reason),
                        };
                    }
                };
                if inner.unlock_attempt.credential_material.is_some() {
                    return WakeRequestStart::Blocked {
                        reason: WakeRequestBlockReason::CredentialAlreadyReady,
                    };
                }

                let now_unix_ms = current_time_unix_ms();
                if matches!(
                    inner.unlock_attempt.phase,
                    ProviderUnlockPhase::WakeRequested { .. }
                ) {
                    return WakeRequestStart::Blocked {
                        reason: WakeRequestBlockReason::WakeAlreadyRunning,
                    };
                }

                if matches!(
                    inner.unlock_attempt.phase,
                    ProviderUnlockPhase::WakeFailed { .. }
                ) && now_unix_ms < inner.wake_retry_allowed_at_unix_ms
                {
                    return WakeRequestStart::Blocked {
                        reason: WakeRequestBlockReason::RetryCooldownActive,
                    };
                }

                inner.unlock_attempt.phase = ProviderUnlockPhase::WakeRequested {
                    attempt_number: 1,
                    attempt_limit,
                };
                let Some(request_id) = inner.wake_request_id.checked_add(1) else {
                    return WakeRequestStart::Blocked {
                        reason: WakeRequestBlockReason::WindowsSubmissionFinished,
                    };
                };
                inner.wake_request_id = request_id;
                inner.credential_generation = Some(generation);
                WakeRequestStart::Started {
                    session_id: Self::session_id_for_current_process(),
                    request_id,
                }
            })
            .unwrap_or(WakeRequestStart::Blocked {
                reason: WakeRequestBlockReason::WakeAlreadyRunning,
            })
    }

    pub fn mark_automatic_retry_started(
        &self,
        request_id: u64,
        attempt_number: u32,
        attempt_limit: u32,
    ) -> bool {
        if let Ok(mut inner) = self.inner.lock() {
            if !self.can_apply_wake(&inner, request_id) {
                return false;
            }
            inner.unlock_attempt.phase = ProviderUnlockPhase::WakeRequested {
                attempt_number,
                attempt_limit,
            };
        } else {
            return false;
        }
        self.notify_credentials_changed();
        true
    }

    fn can_apply_wake(&self, inner: &ProviderStateInner, request_id: u64) -> bool {
        inner.wake_request_id == request_id
            && !matches!(
                inner.unlock_attempt.phase,
                ProviderUnlockPhase::AwaitingWindowsResult
                    | ProviderUnlockPhase::Completed
                    | ProviderUnlockPhase::AutomaticLogonSuppressed { .. }
            )
            && inner
                .credential_generation
                .is_some_and(|generation| self.logon_guard.ready_generation() == Ok(generation))
    }

    pub fn is_wake_request_current(&self, request_id: u64) -> bool {
        self.inner
            .lock()
            .is_ok_and(|inner| self.can_apply_wake(&inner, request_id))
    }

    fn record_wake_failure_locked(inner: &mut ProviderStateInner) {
        inner.wake_retry_allowed_at_unix_ms = current_time_unix_ms() + WAKE_RETRY_COOLDOWN_MS;
    }

    fn record_credential_material_ready_locked(
        inner: &mut ProviderStateInner,
        credential_material: CredentialMaterial,
    ) {
        inner.unlock_attempt.phase = ProviderUnlockPhase::CredentialMaterialReady;
        inner.unlock_attempt.credential_material = Some(credential_material);
        inner.wake_retry_allowed_at_unix_ms = 0;
    }

    fn session_id_for_current_process() -> SessionId {
        SessionId("credential-provider-logon-auto-wake".to_owned())
    }

    pub fn notify_credentials_changed(&self) {
        let notification = self.inner.lock().ok().and_then(|inner| {
            inner
                .events
                .as_ref()
                .map(|events| (events.clone(), inner.advise_context))
        });
        if let Some((events, context)) = notification
            && let Ok(resolved_events) = events.resolve()
        {
            unsafe {
                let _ = resolved_events.CredentialsChanged(context);
            }
        }
    }

    pub fn apply_wake_outcome(
        &self,
        request_id: u64,
        outcome: ProviderWakeOutcome,
        attempt_number: u32,
        attempt_limit: u32,
        automatic_retry_pending: bool,
    ) {
        if let Ok(mut inner) = self.inner.lock() {
            if !self.can_apply_wake(&inner, request_id) {
                return;
            }
            match outcome {
                ProviderWakeOutcome::CredentialMaterialReady {
                    credential_material,
                    ..
                } => {
                    Self::record_credential_material_ready_locked(&mut inner, credential_material);
                }
                ProviderWakeOutcome::AuthFailed {
                    auth_failure_reason,
                    ..
                } => {
                    inner.unlock_attempt.phase = ProviderUnlockPhase::WakeFailed {
                        failure: ProviderWakeFailure::AuthFailed {
                            reason: auth_failure_reason,
                        },
                        attempt_number,
                        attempt_limit,
                        automatic_retry_pending,
                    };
                    Self::record_wake_failure_locked(&mut inner);
                }
                ProviderWakeOutcome::RequestRejected { protocol_error, .. } => {
                    inner.unlock_attempt.phase = ProviderUnlockPhase::WakeFailed {
                        failure: ProviderWakeFailure::RequestRejected { protocol_error },
                        attempt_number,
                        attempt_limit,
                        automatic_retry_pending: false,
                    };
                    Self::record_wake_failure_locked(&mut inner);
                }
            }
        }
        self.notify_credentials_changed();
    }

    pub fn apply_wake_transport_error(
        &self,
        request_id: u64,
        protocol_error: ProtocolError,
        attempt_number: u32,
        attempt_limit: u32,
    ) {
        if let Ok(mut inner) = self.inner.lock() {
            if !self.can_apply_wake(&inner, request_id) {
                return;
            }
            inner.unlock_attempt.phase = ProviderUnlockPhase::WakeFailed {
                failure: ProviderWakeFailure::TransportUnavailable { protocol_error },
                attempt_number,
                attempt_limit,
                automatic_retry_pending: false,
            };
            Self::record_wake_failure_locked(&mut inner);
        }
        self.notify_credentials_changed();
    }

    #[cfg(test)]
    pub fn prepare_credential_material(&self, credential_material: CredentialMaterial) {
        if let Ok(mut inner) = self.inner.lock() {
            let Ok(generation) = self.logon_guard.ready_generation() else {
                return;
            };
            inner.credential_generation = Some(generation);
            Self::record_credential_material_ready_locked(&mut inner, credential_material);
        }
        self.notify_credentials_changed();
    }

    pub fn mark_report_result_received(&self, result: WindowsLogonResult) {
        if let Ok(mut inner) = self.inner.lock() {
            let Some(token) = inner.submission.take() else {
                return;
            };
            let saved = self.logon_guard.complete(token, result);
            inner.unlock_attempt.phase = match saved {
                Ok(()) if result.succeeded() => ProviderUnlockPhase::Completed,
                Ok(()) => ProviderUnlockPhase::AutomaticLogonSuppressed {
                    reason: LogonBlock::WindowsRejected(result),
                },
                Err(reason) => ProviderUnlockPhase::AutomaticLogonSuppressed { reason },
            };
            inner.unlock_attempt.credential_material = None;
            inner.credential_generation = None;
        }
    }
}

fn current_time_unix_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    millis.min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests;
