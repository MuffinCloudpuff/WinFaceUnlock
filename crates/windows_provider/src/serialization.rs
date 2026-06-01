#![allow(unsafe_code)]

use windows::Win32::{
    Foundation::E_FAIL,
    Security::Credentials::{CRED_PACK_FLAGS, CredPackAuthenticationBufferW},
    System::Com::CoTaskMemAlloc,
    UI::Shell::CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION,
};
use windows_core::{PCWSTR, Result};

use crate::{identifiers::PROVIDER_CLSID, provider_state::CredentialMaterial};

pub fn pack_credential_material(
    auth_package_id: u32,
    credential_material: &CredentialMaterial,
) -> Result<CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION> {
    let username = credential_username(credential_material);
    let username_wide = to_wide_null(&username);
    let password_wide = to_wide_null(&credential_material.password);
    let mut required_size = 0_u32;

    let size_probe = unsafe {
        CredPackAuthenticationBufferW(
            CRED_PACK_FLAGS(0),
            PCWSTR(username_wide.as_ptr()),
            PCWSTR(password_wide.as_ptr()),
            None,
            &mut required_size,
        )
    };
    if required_size == 0 {
        return size_probe.map(|()| CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION::default());
    }

    let buffer = unsafe { CoTaskMemAlloc(required_size as usize) as *mut u8 };
    if buffer.is_null() {
        return Err(E_FAIL.into());
    }

    unsafe {
        CredPackAuthenticationBufferW(
            CRED_PACK_FLAGS(0),
            PCWSTR(username_wide.as_ptr()),
            PCWSTR(password_wide.as_ptr()),
            Some(buffer),
            &mut required_size,
        )?;
    }

    Ok(CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION {
        ulAuthenticationPackage: auth_package_id,
        clsidCredentialProvider: PROVIDER_CLSID,
        cbSerialization: required_size,
        rgbSerialization: buffer,
    })
}

fn credential_username(credential_material: &CredentialMaterial) -> String {
    if credential_material.domain.is_empty() || credential_material.domain == "." {
        credential_material.username.clone()
    } else {
        format!(
            "{}\\{}",
            credential_material.domain, credential_material.username
        )
    }
}

fn to_wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_username_omits_local_dot_domain() {
        let material = CredentialMaterial {
            domain: ".".to_owned(),
            username: "leo16".to_owned(),
            password: "secret".to_owned(),
        };

        assert_eq!(credential_username(&material), "leo16");
    }

    #[test]
    fn credential_username_includes_explicit_domain() {
        let material = CredentialMaterial {
            domain: "Liu".to_owned(),
            username: "leo16".to_owned(),
            password: "secret".to_owned(),
        };

        assert_eq!(credential_username(&material), r"Liu\leo16");
    }
}
