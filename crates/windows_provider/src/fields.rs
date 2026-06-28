#![allow(unsafe_code)]

use std::{ffi::c_void, ptr};

use windows::Win32::{
    Foundation::{E_INVALIDARG, E_OUTOFMEMORY},
    System::Com::CoTaskMemAlloc,
    UI::Shell::{
        CPFIS_NONE, CPFS_DISPLAY_IN_BOTH, CPFT_LARGE_TEXT, CPFT_SMALL_TEXT,
        CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE,
        CREDENTIAL_PROVIDER_FIELD_STATE, CREDENTIAL_PROVIDER_FIELD_TYPE,
    },
};
use windows_core::{GUID, PWSTR, Result};

pub const FIELD_COUNT: u32 = 2;
pub const FIELD_ID_TITLE: u32 = 0;
pub const FIELD_ID_STATUS: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderFieldSpec {
    pub field_id: u32,
    pub field_type: CREDENTIAL_PROVIDER_FIELD_TYPE,
    pub label: &'static str,
    pub field_state: CREDENTIAL_PROVIDER_FIELD_STATE,
    pub interactive_state: CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE,
}

pub const FIELD_SPECS: [ProviderFieldSpec; FIELD_COUNT as usize] = [
    ProviderFieldSpec {
        field_id: FIELD_ID_TITLE,
        field_type: CPFT_LARGE_TEXT,
        label: "WinFaceUnlock",
        field_state: CPFS_DISPLAY_IN_BOTH,
        interactive_state: CPFIS_NONE,
    },
    ProviderFieldSpec {
        field_id: FIELD_ID_STATUS,
        field_type: CPFT_SMALL_TEXT,
        label: "Waiting for local face authentication",
        field_state: CPFS_DISPLAY_IN_BOTH,
        interactive_state: CPFIS_NONE,
    },
];

pub fn field_spec(field_id: u32) -> Result<&'static ProviderFieldSpec> {
    FIELD_SPECS
        .iter()
        .find(|spec| spec.field_id == field_id)
        .ok_or_else(|| E_INVALIDARG.into())
}

pub fn allocate_field_descriptor(
    field_id: u32,
) -> Result<*mut CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR> {
    let spec = field_spec(field_id)?;
    let descriptor = allocate_com_struct::<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR>()?;
    let label = allocate_wide_string(spec.label)?;

    unsafe {
        (*descriptor).dwFieldID = spec.field_id;
        (*descriptor).cpft = spec.field_type;
        (*descriptor).pszLabel = label;
        (*descriptor).guidFieldType = GUID::zeroed();
    }

    Ok(descriptor)
}

pub fn allocate_wide_string(value: &str) -> Result<PWSTR> {
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * size_of::<u16>();
    let destination = unsafe { CoTaskMemAlloc(bytes) as *mut u16 };
    if destination.is_null() {
        return Err(E_OUTOFMEMORY.into());
    }
    unsafe {
        ptr::copy_nonoverlapping(wide.as_ptr(), destination, wide.len());
    }
    Ok(PWSTR(destination))
}

fn allocate_com_struct<T>() -> Result<*mut T> {
    let ptr = unsafe { CoTaskMemAlloc(size_of::<T>()) as *mut T };
    if ptr.is_null() {
        return Err(E_OUTOFMEMORY.into());
    }
    unsafe {
        ptr::write_bytes(ptr.cast::<c_void>(), 0, size_of::<T>());
    }
    Ok(ptr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_ids_are_stable_and_descriptive() {
        assert_eq!(FIELD_SPECS[0].field_id, FIELD_ID_TITLE);
        assert_eq!(FIELD_SPECS[0].label, "WinFaceUnlock");
        assert_eq!(FIELD_SPECS[1].field_id, FIELD_ID_STATUS);
    }

    #[test]
    fn unknown_field_id_is_rejected() {
        assert!(field_spec(99).is_err());
    }
}
