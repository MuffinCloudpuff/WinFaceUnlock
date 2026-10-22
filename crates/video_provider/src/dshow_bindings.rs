#![cfg(windows)]
#![allow(unsafe_code)]

use crate::{VideoError, camera_identity::CameraDevice, com_bindings::ComApartment};
use windows::{
    Win32::{
        Media::{
            DirectShow::ICreateDevEnum,
            MediaFoundation::{CLSID_SystemDeviceEnum, CLSID_VideoInputDeviceCategory},
        },
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, StructuredStorage::IPropertyBag},
            Variant::{VARIANT, VT_BSTR, VariantClear},
        },
    },
    core::{PCWSTR, w},
};

/// Enumerates property bags in the same DirectShow category/order OpenCV uses.
/// Binding storage reads metadata; it does not instantiate the capture filter.
pub(crate) fn windows_directshow_devices() -> Result<Vec<CameraDevice>, VideoError> {
    let _apartment = ComApartment::initialize()?;
    // SAFETY: known COM class and interface; windows-rs owns all returned refs.
    let enumerator: ICreateDevEnum =
        unsafe { CoCreateInstance(&CLSID_SystemDeviceEnum, None, CLSCTX_INPROC_SERVER) }
            .map_err(|_| VideoError::ProviderUnavailable)?;
    let mut monikers = None;
    // SAFETY: output is a valid Option for the category enumerator.
    unsafe { enumerator.CreateClassEnumerator(&CLSID_VideoInputDeviceCategory, &mut monikers, 0) }
        .map_err(|_| VideoError::ProviderUnavailable)?;
    let Some(monikers) = monikers else {
        return Ok(Vec::new());
    };
    let mut devices = Vec::new();
    loop {
        let mut next = [None];
        let mut fetched = 0;
        // SAFETY: one-element initialized buffer and fetched count out-parameter.
        unsafe { monikers.Next(&mut next, Some(&mut fetched)) }
            .ok()
            .map_err(|_| VideoError::ProviderUnavailable)?;
        if fetched == 0 {
            break;
        }
        let index = i32::try_from(devices.len()).map_err(|_| VideoError::ProviderUnavailable)?;
        let bag: Option<IPropertyBag> = next[0].as_ref().and_then(|moniker| {
            // SAFETY: requests only the standard metadata property bag.
            unsafe { moniker.BindToStorage(None, None) }.ok()
        });
        let id = bag
            .as_ref()
            .and_then(|bag| property_string(bag, w!("DevicePath")))
            .and_then(|path| crate::device_path_bindings::directshow_camera_id(&path).ok());
        // Keep missing-identity entries so later indices still match DirectShow.
        devices.push(CameraDevice {
            index,
            id,
            // Resolution needs no display metadata; names never bind devices.
            display_name: String::new(),
        });
    }
    Ok(devices)
}

fn property_string(bag: &IPropertyBag, name: PCWSTR) -> Option<String> {
    let mut value = VARIANT::default();
    // SAFETY: initialized VARIANT output, static UTF-16 key and no error logger.
    let result = unsafe { bag.Read(name, &mut value, None) };
    // SAFETY: inspect the discriminant before reading its BSTR union member.
    let text = unsafe {
        if result.is_ok() && value.Anonymous.Anonymous.vt == VT_BSTR {
            Some(value.Anonymous.Anonymous.Anonymous.bstrVal.to_string())
        } else {
            None
        }
    };
    // SAFETY: releases any allocated variant contents, including failed reads.
    let _ = unsafe { VariantClear(&mut value) };
    text
}
