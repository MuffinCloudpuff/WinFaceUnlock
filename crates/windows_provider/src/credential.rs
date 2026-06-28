#![allow(unsafe_code)]

use std::sync::{Arc, Mutex};

use windows::Win32::{
    Foundation::{E_INVALIDARG, E_NOTIMPL, NTSTATUS},
    Graphics::Gdi::HBITMAP,
    UI::Shell::{
        CPGSR_NO_CREDENTIAL_NOT_FINISHED, CPGSR_RETURN_CREDENTIAL_FINISHED, CPSI_NONE,
        CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE,
        CREDENTIAL_PROVIDER_FIELD_STATE, CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE,
        CREDENTIAL_PROVIDER_STATUS_ICON, ICredentialProviderCredential,
        ICredentialProviderCredential_Impl, ICredentialProviderCredentialEvents,
    },
};
use windows_core::{BOOL, PCWSTR, PWSTR, Ref, Result, implement};

use common_protocol::AuthTriggerSource;

use crate::{
    auth_package::retrieve_negotiate_auth_package_id,
    fields::{FIELD_ID_STATUS, FIELD_ID_TITLE, allocate_wide_string, field_spec},
    provider::{
        AUTOMATIC_WAKE_ATTEMPT_LIMIT, WakeAttemptPolicy, WakeStartPolicy,
        request_wake_in_background,
    },
    provider_log::{write_provider_event, write_provider_event_detail},
    provider_state::ProviderState,
    serialization::pack_credential_material,
};

#[implement(ICredentialProviderCredential)]
pub struct WinFaceUnlockCredential {
    state: Arc<ProviderState>,
    credential_events: Mutex<Option<ICredentialProviderCredentialEvents>>,
}

impl WinFaceUnlockCredential {
    pub fn new(state: Arc<ProviderState>) -> Self {
        Self {
            state,
            credential_events: Mutex::new(None),
        }
    }
}

#[allow(non_snake_case)]
impl ICredentialProviderCredential_Impl for WinFaceUnlockCredential_Impl {
    fn Advise(&self, pcpce: Ref<ICredentialProviderCredentialEvents>) -> Result<()> {
        if let Ok(mut events) = self.credential_events.lock() {
            *events = pcpce.cloned();
        }
        Ok(())
    }

    fn UnAdvise(&self) -> Result<()> {
        if let Ok(mut events) = self.credential_events.lock() {
            *events = None;
        }
        Ok(())
    }

    fn SetSelected(&self) -> Result<BOOL> {
        write_provider_event("Credential.SetSelected");
        request_wake_in_background(
            self.state.clone(),
            "Credential.SelectedWake",
            WakeStartPolicy::Immediate,
            WakeAttemptPolicy::Finite {
                attempt_limit: AUTOMATIC_WAKE_ATTEMPT_LIMIT,
            },
            AuthTriggerSource::CredentialScreenEntered,
        );
        Ok(false.into())
    }

    fn SetDeselected(&self) -> Result<()> {
        Ok(())
    }

    fn GetFieldState(
        &self,
        dwfieldid: u32,
        pcpfs: *mut CREDENTIAL_PROVIDER_FIELD_STATE,
        pcpfis: *mut CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE,
    ) -> Result<()> {
        if pcpfs.is_null() || pcpfis.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let spec = field_spec(dwfieldid)?;
        unsafe {
            *pcpfs = spec.field_state;
            *pcpfis = spec.interactive_state;
        }
        Ok(())
    }

    fn GetStringValue(&self, dwfieldid: u32) -> Result<PWSTR> {
        let value = match dwfieldid {
            FIELD_ID_TITLE => "WinFaceUnlock".to_owned(),
            FIELD_ID_STATUS => self.state.credential_status_message(),
            _ => return Err(E_INVALIDARG.into()),
        };
        allocate_wide_string(&value)
    }

    fn GetBitmapValue(&self, dwfieldid: u32) -> Result<HBITMAP> {
        if dwfieldid == FIELD_ID_TITLE {
            Ok(HBITMAP::default())
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetCheckboxValue(
        &self,
        _dwfieldid: u32,
        _pbchecked: *mut BOOL,
        _ppszlabel: *mut PWSTR,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn GetSubmitButtonValue(&self, _dwfieldid: u32) -> Result<u32> {
        Err(E_NOTIMPL.into())
    }

    fn GetComboBoxValueCount(
        &self,
        _dwfieldid: u32,
        _pcitems: *mut u32,
        _pdwselecteditem: *mut u32,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn GetComboBoxValueAt(&self, _dwfieldid: u32, _dwitem: u32) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn SetStringValue(&self, _dwfieldid: u32, _psz: &PCWSTR) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn SetCheckboxValue(&self, _dwfieldid: u32, _bchecked: BOOL) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn SetComboBoxSelectedValue(&self, _dwfieldid: u32, _dwselecteditem: u32) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn CommandLinkClicked(&self, _dwfieldid: u32) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn GetSerialization(
        &self,
        pcpgsr: *mut CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE,
        pcpcs: *mut CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION,
        ppszoptionalstatustext: *mut PWSTR,
        pcpsioptionalstatusicon: *mut CREDENTIAL_PROVIDER_STATUS_ICON,
    ) -> Result<()> {
        write_provider_event("Credential.GetSerialization");
        if pcpgsr.is_null()
            || pcpcs.is_null()
            || ppszoptionalstatustext.is_null()
            || pcpsioptionalstatusicon.is_null()
        {
            return Err(E_INVALIDARG.into());
        }

        if let Some(credential_material) = self.state.credential_material() {
            write_provider_event_detail(
                "Credential.GetSerializationMaterial",
                format!(
                    "domain={} username={}",
                    credential_material.domain, credential_material.username
                ),
            );
            let auth_package_id = retrieve_negotiate_auth_package_id()?;
            let serialization = pack_credential_material(auth_package_id, &credential_material)?;
            self.state.mark_credential_material_serialized();
            write_provider_event_detail(
                "Credential.GetSerializationPacked",
                format!(
                    "auth_package_id={} bytes={}",
                    auth_package_id, serialization.cbSerialization
                ),
            );
            unsafe {
                *pcpgsr = CPGSR_RETURN_CREDENTIAL_FINISHED;
                *pcpcs = serialization;
                *ppszoptionalstatustext = PWSTR::null();
                *pcpsioptionalstatusicon = CPSI_NONE;
            }
        } else {
            unsafe {
                *pcpgsr = CPGSR_NO_CREDENTIAL_NOT_FINISHED;
                *pcpcs = CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION::default();
                *ppszoptionalstatustext =
                    allocate_wide_string("Face authentication has not produced a credential yet")?;
                *pcpsioptionalstatusicon = CPSI_NONE;
            }
        }
        Ok(())
    }

    fn ReportResult(
        &self,
        ntsstatus: NTSTATUS,
        ntssubstatus: NTSTATUS,
        ppszoptionalstatustext: *mut PWSTR,
        pcpsioptionalstatusicon: *mut CREDENTIAL_PROVIDER_STATUS_ICON,
    ) -> Result<()> {
        write_provider_event_detail(
            "Credential.ReportResult",
            format!(
                "ntstatus=0x{:08X} substatus=0x{:08X}",
                ntsstatus.0 as u32, ntssubstatus.0 as u32
            ),
        );
        self.state.mark_report_result_received();
        if !ppszoptionalstatustext.is_null() {
            unsafe {
                *ppszoptionalstatustext = PWSTR::null();
            }
        }
        if !pcpsioptionalstatusicon.is_null() {
            unsafe {
                *pcpsioptionalstatusicon = CPSI_NONE;
            }
        }
        Ok(())
    }
}

pub fn create_credential(state: Arc<ProviderState>) -> ICredentialProviderCredential {
    WinFaceUnlockCredential::new(state).into()
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::Shell::{CPFIS_NONE, CPFS_DISPLAY_IN_BOTH};

    use super::*;

    #[test]
    fn credential_can_be_created_as_com_interface() {
        let state = Arc::new(ProviderState::new());
        let _credential = create_credential(state);
    }

    #[test]
    fn known_text_fields_are_displayed_in_both_states() -> Result<()> {
        let spec = field_spec(FIELD_ID_STATUS)?;

        assert_eq!(spec.field_state, CPFS_DISPLAY_IN_BOTH);
        assert_eq!(spec.interactive_state, CPFIS_NONE);
        Ok(())
    }
}
