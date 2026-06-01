use std::sync::Mutex;

use common_protocol::{ProtocolError, SessionId};
use windows::Win32::UI::Shell::{
    CPUS_INVALID, CREDENTIAL_PROVIDER_NO_DEFAULT, CREDENTIAL_PROVIDER_USAGE_SCENARIO,
    ICredentialProviderEvents,
};

use crate::broker_client::ProviderWakeOutcome;

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
    WakeRequested,
    CredentialMaterialReady,
    WakeFailed(ProviderWakeFailure),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialMaterial {
    pub domain: String,
    pub username: String,
    pub password: String,
}

pub struct ProviderState {
    inner: Mutex<ProviderStateInner>,
}

struct ProviderStateInner {
    usage_scenario: CREDENTIAL_PROVIDER_USAGE_SCENARIO,
    tile_visibility: ProviderTileVisibility,
    events: Option<ICredentialProviderEvents>,
    advise_context: usize,
    unlock_attempt: UnlockAttemptState,
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
            inner.events = events;
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

    pub fn credential_status_message(&self) -> &'static str {
        self.inner
            .lock()
            .map(|inner| match &inner.unlock_attempt.phase {
                ProviderUnlockPhase::ProviderLoaded => "Waiting for local face authentication",
                ProviderUnlockPhase::WakeRequested => "Local face authentication is running",
                ProviderUnlockPhase::CredentialMaterialReady => {
                    "Face authentication credential is ready"
                }
                ProviderUnlockPhase::WakeFailed(ProviderWakeFailure::AuthFailed { .. }) => {
                    "Face authentication was rejected"
                }
                ProviderUnlockPhase::WakeFailed(ProviderWakeFailure::RequestRejected {
                    ..
                }) => "Face authentication request was rejected",
                ProviderUnlockPhase::WakeFailed(ProviderWakeFailure::TransportUnavailable {
                    ..
                }) => "Face authentication service is unavailable",
                ProviderUnlockPhase::Completed => "Face authentication completed",
            })
            .unwrap_or("Waiting for local face authentication")
    }

    pub fn begin_wake_request(&self) -> Option<SessionId> {
        self.inner.lock().ok().and_then(|mut inner| {
            if !matches!(
                inner.unlock_attempt.phase,
                ProviderUnlockPhase::ProviderLoaded | ProviderUnlockPhase::WakeFailed(_)
            ) || inner.unlock_attempt.credential_material.is_some()
            {
                return None;
            }

            inner.unlock_attempt.phase = ProviderUnlockPhase::WakeRequested;
            Some(Self::session_id_for_current_process())
        })
    }

    pub fn apply_wake_outcome(&self, outcome: ProviderWakeOutcome) {
        if let Ok(mut inner) = self.inner.lock() {
            match outcome {
                ProviderWakeOutcome::CredentialMaterialReady {
                    credential_material,
                    ..
                } => {
                    inner.unlock_attempt.phase = ProviderUnlockPhase::CredentialMaterialReady;
                    inner.unlock_attempt.credential_material = Some(credential_material);
                }
                ProviderWakeOutcome::AuthFailed {
                    auth_failure_reason,
                    ..
                } => {
                    inner.unlock_attempt.phase =
                        ProviderUnlockPhase::WakeFailed(ProviderWakeFailure::AuthFailed {
                            reason: auth_failure_reason,
                        });
                }
                ProviderWakeOutcome::RequestRejected { protocol_error, .. } => {
                    inner.unlock_attempt.phase =
                        ProviderUnlockPhase::WakeFailed(ProviderWakeFailure::RequestRejected {
                            protocol_error,
                        });
                }
            }
        }
        self.notify_credentials_changed();
    }

    pub fn apply_wake_transport_error(&self, protocol_error: ProtocolError) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.unlock_attempt.phase =
                ProviderUnlockPhase::WakeFailed(ProviderWakeFailure::TransportUnavailable {
                    protocol_error,
                });
        }
        self.notify_credentials_changed();
    }

    pub fn prepare_credential_material(&self, credential_material: CredentialMaterial) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.unlock_attempt.phase = ProviderUnlockPhase::CredentialMaterialReady;
            inner.unlock_attempt.credential_material = Some(credential_material);
        }
        self.notify_credentials_changed();
    }

    pub fn mark_report_result_received(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.unlock_attempt.phase = ProviderUnlockPhase::Completed;
            inner.unlock_attempt.credential_material = None;
        }
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
        {
            unsafe {
                let _ = events.CredentialsChanged(inner.advise_context);
            }
        }
    }
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

        assert!(state.begin_wake_request().is_some());
        assert_eq!(state.begin_wake_request(), None);
        assert_eq!(
            state.credential_status_message(),
            "Local face authentication is running"
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
}
