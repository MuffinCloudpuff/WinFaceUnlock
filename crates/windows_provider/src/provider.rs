#![allow(unsafe_code)]

use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use common_protocol::{AuthTriggerSource, ProtocolError};
use windows::Win32::{
    Foundation::E_INVALIDARG,
    UI::Shell::{
        CPUS_CHANGE_PASSWORD, CPUS_CREDUI, CPUS_INVALID, CPUS_LOGON, CPUS_UNLOCK_WORKSTATION,
        CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION, CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR,
        CREDENTIAL_PROVIDER_USAGE_SCENARIO, ICredentialProvider, ICredentialProvider_Impl,
        ICredentialProviderCredential, ICredentialProviderEvents,
    },
};
use windows_core::{BOOL, Ref, Result, implement};

use crate::{
    broker_client::{ProviderBrokerClient, ProviderWakeOutcome},
    credential::create_credential,
    dll_lifetime::DllWorkerGuard,
    fields::{FIELD_COUNT, allocate_field_descriptor},
    provider_config::{ProviderLogonWakeMode, ProviderRuntimeConfig},
    provider_log::{write_provider_event, write_provider_event_detail},
    provider_state::{ProviderState, WakeRequestStart},
};

const AUTOMATIC_WAKE_ATTEMPT_LIMIT: u32 = 3;
const AUTOMATIC_WAKE_RETRY_DELAY_MS: u64 = 600;
const ADVISE_INPUT_WAIT_TIMEOUT_MS: u64 = 10 * 60 * 1_000;
const ADVISE_INPUT_POLL_INTERVAL_MS: u64 = 250;
const TRANSPORT_WAKE_ATTEMPT_LIMIT: u32 = 20;
const TRANSPORT_WAKE_RETRY_DELAY_MS: u64 = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WakeStartPolicy {
    Immediate,
    WaitForUserInputAfterAdvise,
}

#[implement(ICredentialProvider)]
pub struct WinFaceUnlockProvider {
    state: Arc<ProviderState>,
    credential: Mutex<Option<ICredentialProviderCredential>>,
}

impl WinFaceUnlockProvider {
    pub fn new() -> Self {
        let runtime_config = ProviderRuntimeConfig::from_registry_or_default();
        Self {
            state: Arc::new(ProviderState::with_tile_visibility(
                runtime_config.tile_visibility,
            )),
            credential: Mutex::new(None),
        }
    }
}

#[allow(non_snake_case)]
impl ICredentialProvider_Impl for WinFaceUnlockProvider_Impl {
    fn SetUsageScenario(
        &self,
        cpus: CREDENTIAL_PROVIDER_USAGE_SCENARIO,
        _dwflags: u32,
    ) -> Result<()> {
        write_provider_event("Provider.SetUsageScenario");
        match cpus {
            CPUS_LOGON | CPUS_UNLOCK_WORKSTATION => {
                self.state.set_usage_scenario(cpus);
                Ok(())
            }
            CPUS_CHANGE_PASSWORD | CPUS_CREDUI | CPUS_INVALID => Err(E_INVALIDARG.into()),
            _ => Err(E_INVALIDARG.into()),
        }
    }

    fn SetSerialization(
        &self,
        _pcpcs: *const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION,
    ) -> Result<()> {
        Ok(())
    }

    fn Advise(&self, pcpe: Ref<ICredentialProviderEvents>, upadvisecontext: usize) -> Result<()> {
        write_provider_event("Provider.Advise");
        self.state.set_events(pcpe.cloned(), upadvisecontext);
        match ProviderRuntimeConfig::from_registry_or_default().logon_wake_mode {
            Some(ProviderLogonWakeMode::InputTriggered) => {
                request_wake_in_background(
                    self.state.clone(),
                    "Provider.InputTriggeredWake",
                    WakeStartPolicy::WaitForUserInputAfterAdvise,
                    AuthTriggerSource::InputTriggered,
                );
            }
            Some(ProviderLogonWakeMode::BackgroundPolicy) => {
                request_wake_in_background(
                    self.state.clone(),
                    "Provider.BackgroundPolicyWake",
                    WakeStartPolicy::Immediate,
                    AuthTriggerSource::BackgroundPolicy,
                );
            }
            Some(ProviderLogonWakeMode::Hybrid) => {
                request_wake_in_background(
                    self.state.clone(),
                    "Provider.BackgroundPolicyWake",
                    WakeStartPolicy::Immediate,
                    AuthTriggerSource::BackgroundPolicy,
                );
                request_wake_in_background(
                    self.state.clone(),
                    "Provider.InputTriggeredWake",
                    WakeStartPolicy::WaitForUserInputAfterAdvise,
                    AuthTriggerSource::InputTriggered,
                );
            }
            None => {}
        }
        Ok(())
    }

    fn UnAdvise(&self) -> Result<()> {
        write_provider_event("Provider.UnAdvise");
        self.state.set_events(None, 0);
        Ok(())
    }

    fn GetFieldDescriptorCount(&self) -> Result<u32> {
        Ok(FIELD_COUNT)
    }

    fn GetFieldDescriptorAt(
        &self,
        dwindex: u32,
    ) -> Result<*mut CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR> {
        allocate_field_descriptor(dwindex)
    }

    fn GetCredentialCount(
        &self,
        pdwcount: *mut u32,
        pdwdefault: *mut u32,
        pbautologonwithdefault: *mut BOOL,
    ) -> Result<()> {
        write_provider_event("Provider.GetCredentialCount");
        if pdwcount.is_null() || pdwdefault.is_null() || pbautologonwithdefault.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            let plan = self.state.credential_count_plan();
            *pdwcount = plan.credential_count;
            *pdwdefault = plan.default_credential_index;
            *pbautologonwithdefault = plan.auto_logon_with_default.into();
        }
        Ok(())
    }

    fn GetCredentialAt(&self, dwindex: u32) -> Result<ICredentialProviderCredential> {
        write_provider_event("Provider.GetCredentialAt");
        if dwindex != 0 {
            return Err(E_INVALIDARG.into());
        }

        let mut credential = self.credential.lock().map_err(|_| E_INVALIDARG)?;
        if let Some(existing) = credential.as_ref() {
            return Ok(existing.clone());
        }

        let created = create_credential(self.state.clone());
        *credential = Some(created.clone());
        Ok(created)
    }
}

pub fn create_provider() -> ICredentialProvider {
    WinFaceUnlockProvider::new().into()
}

pub(crate) fn request_wake_in_background(
    state: Arc<ProviderState>,
    trigger_name: &'static str,
    start_policy: WakeStartPolicy,
    trigger_source: AuthTriggerSource,
) {
    let session_id = match state.begin_wake_request(AUTOMATIC_WAKE_ATTEMPT_LIMIT) {
        WakeRequestStart::Started { session_id } => session_id,
        WakeRequestStart::Blocked { reason } => {
            write_provider_event_detail("Provider.WakeSkipped", format!("reason={reason:?}"));
            return;
        }
    };

    write_provider_event(trigger_name);
    let worker_state = state.clone();
    let worker_guard = DllWorkerGuard::new();
    let spawn_result = std::thread::Builder::new()
        .name("winfaceunlock-provider-wake".to_owned())
        .spawn(move || {
            let _worker_guard = worker_guard;
            if !wait_for_wake_start_policy(&worker_state, start_policy) {
                write_provider_event_detail(
                    "Provider.WakeStopped",
                    "reason=user-input-wait-ended",
                );
                worker_state.apply_wake_transport_error(
                    ProtocolError::TransportUnavailable,
                    1,
                    AUTOMATIC_WAKE_ATTEMPT_LIMIT,
                );
                return;
            }

            let runtime_config = ProviderRuntimeConfig::from_registry_or_default();
            let broker_client =
                ProviderBrokerClient::service_default(runtime_config.wake_auth_source);
            for attempt_number in 1..=AUTOMATIC_WAKE_ATTEMPT_LIMIT {
                if attempt_number > 1 {
                    thread::sleep(Duration::from_millis(AUTOMATIC_WAKE_RETRY_DELAY_MS));
                    worker_state
                        .mark_automatic_retry_started(attempt_number, AUTOMATIC_WAKE_ATTEMPT_LIMIT);
                }

                write_provider_event_detail(
                    "Provider.WakeRequestStarted",
                    format!(
                        "attempt={attempt_number}/{AUTOMATIC_WAKE_ATTEMPT_LIMIT} session_id={}",
                        session_id.0
                    ),
                );
                match wake_and_fetch_with_transport_retry(
                    &broker_client,
                    session_id.clone(),
                    trigger_source,
                    TRANSPORT_WAKE_ATTEMPT_LIMIT,
                ) {
                    Ok(outcome) => {
                        let automatic_retry_pending = matches!(
                            &outcome,
                            ProviderWakeOutcome::AuthFailed { .. }
                        ) && attempt_number < AUTOMATIC_WAKE_ATTEMPT_LIMIT
                            && worker_state.has_events_sink();
                        write_wake_outcome_detail(
                            "Provider.WakeCompleted",
                            &outcome,
                            attempt_number,
                            AUTOMATIC_WAKE_ATTEMPT_LIMIT,
                            automatic_retry_pending,
                        );
                        worker_state.apply_wake_outcome(
                            outcome,
                            attempt_number,
                            AUTOMATIC_WAKE_ATTEMPT_LIMIT,
                            automatic_retry_pending,
                        );
                        if !automatic_retry_pending {
                            return;
                        }
                    }
                    Err(error) => {
                        write_provider_event_detail(
                            "Provider.WakeTransportFailed",
                            format!(
                                "attempt={attempt_number}/{AUTOMATIC_WAKE_ATTEMPT_LIMIT} error={error:?}"
                            ),
                        );
                        worker_state.apply_wake_transport_error(
                            error,
                            attempt_number,
                            AUTOMATIC_WAKE_ATTEMPT_LIMIT,
                        );
                        return;
                    }
                }
            }
        });
    if spawn_result.is_err() {
        write_provider_event("Provider.WakeThreadSpawnFailed");
        state.apply_wake_transport_error(
            ProtocolError::TransportUnavailable,
            1,
            AUTOMATIC_WAKE_ATTEMPT_LIMIT,
        );
    }
}

fn wait_for_wake_start_policy(state: &ProviderState, start_policy: WakeStartPolicy) -> bool {
    match start_policy {
        WakeStartPolicy::Immediate => true,
        WakeStartPolicy::WaitForUserInputAfterAdvise => wait_for_user_input_after_advise(state),
    }
}

fn wait_for_user_input_after_advise(state: &ProviderState) -> bool {
    let Some(baseline) = LastInputSnapshot::capture() else {
        write_provider_event("Provider.LockScreenInputProbeUnavailable");
        return true;
    };

    write_provider_event_detail(
        "Provider.LockScreenInputWaiting",
        format!("baseline_tick={}", baseline.last_input_tick_ms),
    );
    let deadline = Instant::now() + Duration::from_millis(ADVISE_INPUT_WAIT_TIMEOUT_MS);
    while Instant::now() < deadline {
        if !state.has_events_sink() {
            return false;
        }
        thread::sleep(Duration::from_millis(ADVISE_INPUT_POLL_INTERVAL_MS));
        let Some(current) = LastInputSnapshot::capture() else {
            return true;
        };
        if current.last_input_tick_ms != baseline.last_input_tick_ms {
            write_provider_event_detail(
                "Provider.LockScreenInputObserved",
                format!(
                    "baseline_tick={} current_tick={} age_ms={}",
                    baseline.last_input_tick_ms, current.last_input_tick_ms, current.input_age_ms
                ),
            );
            return true;
        }
    }

    false
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LastInputSnapshot {
    last_input_tick_ms: u32,
    input_age_ms: u64,
}

impl LastInputSnapshot {
    #[cfg(windows)]
    #[allow(unsafe_code)]
    fn capture() -> Option<Self> {
        use windows_sys::Win32::{
            System::SystemInformation::GetTickCount64,
            UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        };

        let mut last_input = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        let read_succeeded = unsafe { GetLastInputInfo(&mut last_input) };
        if read_succeeded == 0 {
            return None;
        }
        let now_tick_ms = unsafe { GetTickCount64() as u32 };
        let last_input_tick_ms = last_input.dwTime;
        Some(Self {
            last_input_tick_ms,
            input_age_ms: u64::from(now_tick_ms.wrapping_sub(last_input_tick_ms)),
        })
    }

    #[cfg(not(windows))]
    fn capture() -> Option<Self> {
        None
    }
}

fn write_wake_outcome_detail(
    event_name: &str,
    outcome: &ProviderWakeOutcome,
    attempt_number: u32,
    attempt_limit: u32,
    automatic_retry_pending: bool,
) {
    match outcome {
        ProviderWakeOutcome::CredentialMaterialReady { session_id, .. } => {
            write_provider_event_detail(
                event_name,
                format!(
                    "attempt={attempt_number}/{attempt_limit} outcome=CredentialMaterialReady session_id={}",
                    session_id.0
                ),
            );
        }
        ProviderWakeOutcome::AuthFailed {
            session_id,
            auth_failure_reason,
        } => {
            write_provider_event_detail(
                event_name,
                format!(
                    "attempt={attempt_number}/{attempt_limit} outcome=AuthFailed session_id={} reason={auth_failure_reason:?} retry_pending={automatic_retry_pending}",
                    session_id.0
                ),
            );
        }
        ProviderWakeOutcome::RequestRejected {
            session_id,
            protocol_error,
        } => {
            write_provider_event_detail(
                event_name,
                format!(
                    "attempt={attempt_number}/{attempt_limit} outcome=RequestRejected session_id={} error={protocol_error:?}",
                    session_id.0
                ),
            );
        }
    }
}

fn wake_and_fetch_with_transport_retry(
    broker_client: &ProviderBrokerClient,
    session_id: common_protocol::SessionId,
    trigger_source: AuthTriggerSource,
    transport_attempt_limit: u32,
) -> std::result::Result<ProviderWakeOutcome, ProtocolError> {
    for transport_attempt_number in 1..=transport_attempt_limit {
        match broker_client.wake_and_fetch_credential_material(session_id.clone(), trigger_source) {
            Ok(outcome) => return Ok(outcome),
            Err(error) if transport_attempt_number < transport_attempt_limit => {
                write_provider_event_detail(
                    "Provider.WakeTransportRetry",
                    format!(
                        "attempt={transport_attempt_number}/{transport_attempt_limit} error={error:?}"
                    ),
                );
                thread::sleep(Duration::from_millis(TRANSPORT_WAKE_RETRY_DELAY_MS));
            }
            Err(error) => return Err(error),
        }
    }

    Err(ProtocolError::TransportUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_can_be_created_as_com_interface() {
        let _provider = create_provider();
    }
}
