#![allow(unsafe_code)]

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use common_protocol::ProtocolError;
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
    fields::{FIELD_COUNT, allocate_field_descriptor},
    provider_config::ProviderRuntimeConfig,
    provider_log::{write_provider_event, write_provider_event_detail},
    provider_state::{ProviderState, WakeRequestStart},
};

const AUTOMATIC_WAKE_ATTEMPT_LIMIT: u32 = 3;
const AUTOMATIC_WAKE_RETRY_DELAY_MS: u64 = 600;

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
        if ProviderRuntimeConfig::from_registry_or_default().auto_wake_on_advise {
            request_wake_in_background(self.state.clone(), "Provider.AutoWake");
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

pub(crate) fn request_wake_in_background(state: Arc<ProviderState>, trigger_name: &'static str) {
    let session_id = match state.begin_wake_request(AUTOMATIC_WAKE_ATTEMPT_LIMIT) {
        WakeRequestStart::Started { session_id } => session_id,
        WakeRequestStart::Blocked { reason } => {
            write_provider_event_detail("Provider.WakeSkipped", format!("reason={reason:?}"));
            return;
        }
    };

    write_provider_event(trigger_name);
    let worker_state = state.clone();
    let spawn_result = std::thread::Builder::new()
        .name("winfaceunlock-provider-wake".to_owned())
        .spawn(move || {
            let runtime_config = ProviderRuntimeConfig::from_registry_or_default();
            let broker_client =
                ProviderBrokerClient::service_default(runtime_config.wake_auth_source);
            for attempt_number in 1..=AUTOMATIC_WAKE_ATTEMPT_LIMIT {
                if attempt_number > 1 {
                    thread::sleep(Duration::from_millis(AUTOMATIC_WAKE_RETRY_DELAY_MS));
                    worker_state
                        .mark_automatic_retry_started(attempt_number, AUTOMATIC_WAKE_ATTEMPT_LIMIT);
                }

                match broker_client.wake_and_fetch_credential_material(session_id.clone()) {
                    Ok(outcome) => {
                        let automatic_retry_pending =
                            matches!(&outcome, ProviderWakeOutcome::AuthFailed { .. })
                                && attempt_number < AUTOMATIC_WAKE_ATTEMPT_LIMIT;
                        write_provider_event("Provider.WakeCompleted");
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
                        write_provider_event("Provider.WakeTransportFailed");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_can_be_created_as_com_interface() {
        let _provider = create_provider();
    }
}
