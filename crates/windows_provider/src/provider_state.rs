use std::{
    fmt,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use common_protocol::{ProtocolError, SessionId};
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WakeRequestStart {
    Started { session_id: SessionId },
    Blocked { reason: WakeRequestBlockReason },
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
}

struct ProviderStateInner {
    usage_scenario: CREDENTIAL_PROVIDER_USAGE_SCENARIO,
    tile_visibility: ProviderTileVisibility,
    events: Option<AgileReference<ICredentialProviderEvents>>,
    advise_context: usize,
    unlock_attempt: UnlockAttemptState,
    wake_retry_allowed_at_unix_ms: i64,
}

impl ProviderState {
    pub fn new() -> Self {
        Self::with_tile_visibility(ProviderTileVisibility::default())
    }

    pub fn with_tile_visibility(tile_visibility: ProviderTileVisibility) -> Self {
        Self {
            inner: Mutex::new(ProviderStateInner {
                usage_scenario: CPUS_INVALID,
                tile_visibility,
                events: None,
                advise_context: 0,
                unlock_attempt: UnlockAttemptState::default(),
                wake_retry_allowed_at_unix_ms: 0,
            }),
        }
    }

    pub fn set_usage_scenario(&self, usage_scenario: CREDENTIAL_PROVIDER_USAGE_SCENARIO) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.usage_scenario = usage_scenario;
        }
    }

    pub fn set_events(&self, events: Option<ICredentialProviderEvents>, advise_context: usize) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.events = events
                .as_ref()
                .and_then(|event_sink| AgileReference::new(event_sink).ok());
            inner.advise_context = advise_context;
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

    pub fn credential_material(&self) -> Option<CredentialMaterial> {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| inner.unlock_attempt.credential_material.clone())
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
                WakeRequestStart::Started {
                    session_id: Self::session_id_for_current_process(),
                }
            })
            .unwrap_or(WakeRequestStart::Blocked {
                reason: WakeRequestBlockReason::WakeAlreadyRunning,
            })
    }

    pub fn mark_automatic_retry_started(&self, attempt_number: u32, attempt_limit: u32) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.unlock_attempt.phase = ProviderUnlockPhase::WakeRequested {
                attempt_number,
                attempt_limit,
            };
        }
        self.notify_credentials_changed();
    }

    pub fn mark_credential_material_serialized(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.unlock_attempt.phase = ProviderUnlockPhase::Completed;
            inner.unlock_attempt.credential_material = None;
        }
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
        SessionId(format!(
            "credential-provider-session-{}",
            std::process::id()
        ))
    }

    pub fn notify_credentials_changed(&self) {
        if let Ok(inner) = self.inner.lock()
            && let Some(events) = &inner.events
            && let Ok(resolved_events) = events.resolve()
        {
            unsafe {
                let _ = resolved_events.CredentialsChanged(inner.advise_context);
            }
        }
    }

    pub fn apply_wake_outcome(
        &self,
        outcome: ProviderWakeOutcome,
        attempt_number: u32,
        attempt_limit: u32,
        automatic_retry_pending: bool,
    ) {
        if let Ok(mut inner) = self.inner.lock() {
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
        protocol_error: ProtocolError,
        attempt_number: u32,
        attempt_limit: u32,
    ) {
        if let Ok(mut inner) = self.inner.lock() {
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

    pub fn prepare_credential_material(&self, credential_material: CredentialMaterial) {
        if let Ok(mut inner) = self.inner.lock() {
            Self::record_credential_material_ready_locked(&mut inner, credential_material);
        }
        self.notify_credentials_changed();
    }

    pub fn mark_report_result_received(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.unlock_attempt.phase = ProviderUnlockPhase::Completed;
            inner.unlock_attempt.credential_material = None;
            inner.wake_retry_allowed_at_unix_ms = 0;
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
mod tests {
    use windows::Win32::UI::Shell::CPUS_UNLOCK_WORKSTATION;

    use super::*;

    #[test]
    fn usage_scenario_defaults_to_invalid_then_updates_explicitly() {
        let state = ProviderState::new();

        assert_eq!(state.usage_scenario(), CPUS_INVALID);
        state.set_usage_scenario(CPUS_UNLOCK_WORKSTATION);
        assert_eq!(state.usage_scenario(), CPUS_UNLOCK_WORKSTATION);
    }

    #[test]
    fn visible_mode_shows_one_tile_before_credential_material_exists() {
        let state = ProviderState::with_tile_visibility(
            ProviderTileVisibility::VisibleBeforeCredentialReady,
        );

        let plan = state.credential_count_plan();

        assert_eq!(plan.credential_count, 1);
        assert_eq!(
            plan.default_credential_index,
            CREDENTIAL_PROVIDER_NO_DEFAULT
        );
        assert!(!plan.auto_logon_with_default);
    }

    #[test]
    fn hidden_mode_hides_tile_before_credential_material_exists() {
        let state =
            ProviderState::with_tile_visibility(ProviderTileVisibility::HiddenUntilCredentialReady);

        let plan = state.credential_count_plan();

        assert_eq!(plan.credential_count, 0);
        assert_eq!(
            plan.default_credential_index,
            CREDENTIAL_PROVIDER_NO_DEFAULT
        );
        assert!(!plan.auto_logon_with_default);
    }

    #[test]
    fn wake_request_can_start_once_until_result_arrives() {
        let state = ProviderState::new();

        assert!(matches!(
            state.begin_wake_request(3),
            WakeRequestStart::Started { .. }
        ));
        assert_eq!(
            state.begin_wake_request(3),
            WakeRequestStart::Blocked {
                reason: WakeRequestBlockReason::WakeAlreadyRunning
            }
        );
        assert_eq!(
            state.credential_status_message(),
            "Local face authentication is running (1/3)"
        );
    }

    #[test]
    fn credential_material_requests_default_auto_logon() {
        let state =
            ProviderState::with_tile_visibility(ProviderTileVisibility::HiddenUntilCredentialReady);

        state.prepare_credential_material(CredentialMaterial {
            domain: ".".to_owned(),
            username: "leo16".to_owned(),
            password: "secret".to_owned(),
        });
        let plan = state.credential_count_plan();

        assert_eq!(plan.credential_count, 1);
        assert_eq!(plan.default_credential_index, 0);
        assert!(plan.auto_logon_with_default);
    }

    #[test]
    fn serialized_credential_material_is_consumed_after_use() {
        let state = ProviderState::new();
        state.prepare_credential_material(CredentialMaterial {
            domain: ".".to_owned(),
            username: "leo16".to_owned(),
            password: "secret".to_owned(),
        });

        state.mark_credential_material_serialized();
        let plan = state.credential_count_plan();

        assert_eq!(plan.credential_count, 1);
        assert_eq!(
            plan.default_credential_index,
            CREDENTIAL_PROVIDER_NO_DEFAULT
        );
        assert!(!plan.auto_logon_with_default);
        assert_eq!(state.credential_material(), None);
    }

    #[test]
    fn wake_failure_enforces_retry_cooldown() {
        let state = ProviderState::new();

        state.apply_wake_transport_error(ProtocolError::TransportUnavailable, 1, 3);

        assert_eq!(
            state.begin_wake_request(3),
            WakeRequestStart::Blocked {
                reason: WakeRequestBlockReason::RetryCooldownActive
            }
        );
    }

    #[test]
    fn intermediate_auth_failure_reports_automatic_retry() {
        let state = ProviderState::new();

        state.apply_wake_outcome(
            ProviderWakeOutcome::AuthFailed {
                session_id: SessionId("session-1".to_owned()),
                auth_failure_reason: common_protocol::AuthFailureReason::LivenessFailed,
            },
            1,
            3,
            true,
        );

        assert_eq!(
            state.credential_status_message(),
            "Face authentication failed. Retrying automatically (1/3)"
        );

        state.mark_automatic_retry_started(2, 3);
        assert_eq!(
            state.credential_status_message(),
            "Local face authentication is running (2/3)"
        );
    }

    #[test]
    fn final_auth_failure_reports_pin_or_password_fallback() {
        let state = ProviderState::new();

        state.apply_wake_outcome(
            ProviderWakeOutcome::AuthFailed {
                session_id: SessionId("session-1".to_owned()),
                auth_failure_reason: common_protocol::AuthFailureReason::LivenessFailed,
            },
            3,
            3,
            false,
        );

        assert_eq!(
            state.credential_status_message(),
            "Face authentication failed. Use PIN or password fallback"
        );
    }

    #[test]
    fn intermediate_auth_failure_keeps_face_tile_visible_during_retry() {
        let state = ProviderState::new();

        state.apply_wake_outcome(
            ProviderWakeOutcome::AuthFailed {
                session_id: SessionId("session-1".to_owned()),
                auth_failure_reason: common_protocol::AuthFailureReason::MatchBelowThreshold,
            },
            1,
            3,
            true,
        );

        let plan = state.credential_count_plan();

        assert_eq!(plan.credential_count, 1);
        assert_eq!(
            plan.default_credential_index,
            CREDENTIAL_PROVIDER_NO_DEFAULT
        );
        assert!(!plan.auto_logon_with_default);
    }

    #[test]
    fn credential_material_debug_redacts_password() {
        let material = CredentialMaterial {
            domain: ".".to_owned(),
            username: "leo16".to_owned(),
            password: "secret".to_owned(),
        };

        let debug = format!("{material:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret"));
    }
}
