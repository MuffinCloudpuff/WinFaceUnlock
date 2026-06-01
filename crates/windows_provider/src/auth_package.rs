#![allow(unsafe_code)]

use windows::Win32::{
    Foundation::{E_FAIL, HANDLE, STATUS_SUCCESS},
    Security::Authentication::Identity::{
        LSA_STRING, LsaConnectUntrusted, LsaDeregisterLogonProcess, LsaLookupAuthenticationPackage,
    },
};
use windows_core::{PSTR, Result};

pub fn retrieve_negotiate_auth_package_id() -> Result<u32> {
    let mut lsa_handle = HANDLE::default();
    let connect_status = unsafe { LsaConnectUntrusted(&mut lsa_handle) };
    if connect_status != STATUS_SUCCESS {
        return Err(E_FAIL.into());
    }

    let lookup_result = lookup_negotiate_package(lsa_handle);
    unsafe {
        let _ = LsaDeregisterLogonProcess(lsa_handle);
    }
    lookup_result
}

fn lookup_negotiate_package(lsa_handle: HANDLE) -> Result<u32> {
    let package_name = b"Negotiate";
    let lsa_package_name = LSA_STRING {
        Length: package_name.len() as u16,
        MaximumLength: package_name.len() as u16,
        Buffer: PSTR(package_name.as_ptr().cast_mut()),
    };

    let mut package_id = 0_u32;
    let lookup_status =
        unsafe { LsaLookupAuthenticationPackage(lsa_handle, &lsa_package_name, &mut package_id) };
    if lookup_status != STATUS_SUCCESS {
        return Err(E_FAIL.into());
    }

    Ok(package_id)
}
