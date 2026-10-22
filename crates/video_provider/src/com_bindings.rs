#![cfg(windows)]
#![allow(unsafe_code)]

use crate::VideoError;
use windows::Win32::{
    Foundation::RPC_E_CHANGED_MODE,
    System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
};

pub(crate) struct ComApartment(bool);

impl ComApartment {
    pub(crate) fn initialize() -> Result<Self, VideoError> {
        // SAFETY: initializes this thread; S_OK and S_FALSE both need balancing.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            Ok(Self(true))
        } else if result == RPC_E_CHANGED_MODE {
            // An existing STA is valid for metadata enumeration. Do not release
            // initialization owned by the caller.
            Ok(Self(false))
        } else {
            Err(VideoError::ProviderUnavailable)
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances only this guard's successful CoInitializeEx call.
            unsafe { CoUninitialize() };
        }
    }
}
