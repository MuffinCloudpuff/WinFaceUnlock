#![allow(unsafe_code)]

use std::ffi::c_void;

use windows::Win32::{
    Foundation::{CLASS_E_CLASSNOTAVAILABLE, E_INVALIDARG, HINSTANCE, S_FALSE, S_OK},
    System::SystemServices::DLL_PROCESS_ATTACH,
};
use windows_core::{BOOL, GUID, HRESULT, Interface};

use crate::{
    class_factory::{create_class_factory, dll_server_lock_count},
    dll_lifetime::active_worker_count,
    identifiers::PROVIDER_CLSID,
    provider_log::{write_provider_event, write_provider_event_detail},
};

#[unsafe(no_mangle)]
/// # Safety
///
/// Called by COM. `rclsid`, `riid`, and `ppv` must be valid COM pointers supplied
/// by the Windows loader.
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    write_provider_event("DllGetClassObject");
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_INVALIDARG;
    }

    unsafe {
        *ppv = std::ptr::null_mut();
    }

    if unsafe { *rclsid } != PROVIDER_CLSID {
        return CLASS_E_CLASSNOTAVAILABLE;
    }

    let factory = create_class_factory();
    unsafe { factory.query(riid, ppv) }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// Called by COM during DLL lifetime checks. This function does not dereference
/// caller-provided pointers.
pub unsafe extern "system" fn DllCanUnloadNow() -> HRESULT {
    let server_locks = dll_server_lock_count();
    let active_workers = active_worker_count();
    let can_unload = server_locks == 0 && active_workers == 0;
    write_provider_event_detail(
        "DllCanUnloadNow",
        format!(
            "server_locks={server_locks} active_workers={active_workers} result={}",
            if can_unload { "S_OK" } else { "S_FALSE" }
        ),
    );
    if can_unload { S_OK } else { S_FALSE }
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
/// # Safety
///
/// Called by the Windows loader. Arguments must be the values supplied by the loader.
pub unsafe extern "system" fn DllMain(
    _hinst_dll: HINSTANCE,
    dw_reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match dw_reason {
        DLL_PROCESS_ATTACH => true.into(),
        _ => true.into(),
    }
}
