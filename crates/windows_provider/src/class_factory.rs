#![allow(unsafe_code)]

use std::{
    ffi::c_void,
    sync::atomic::{AtomicI32, Ordering},
};

use windows::Win32::{
    Foundation::{CLASS_E_NOAGGREGATION, E_INVALIDARG},
    System::Com::{IClassFactory, IClassFactory_Impl},
};
use windows_core::{BOOL, GUID, IUnknown, Interface, Ref, Result, implement};

use crate::{provider::create_provider, provider_log::write_provider_event};

static DLL_LOCK_COUNT: AtomicI32 = AtomicI32::new(0);

#[implement(IClassFactory)]
pub struct WinFaceUnlockClassFactory;

#[allow(non_snake_case)]
impl IClassFactory_Impl for WinFaceUnlockClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Ref<IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> Result<()> {
        write_provider_event("ClassFactory.CreateInstance");
        if punkouter.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        if riid.is_null() || ppvobject.is_null() {
            return Err(E_INVALIDARG.into());
        }

        unsafe {
            *ppvobject = std::ptr::null_mut();
        }
        let provider = create_provider();
        unsafe { provider.query(riid, ppvobject).ok() }
    }

    fn LockServer(&self, flock: BOOL) -> Result<()> {
        write_provider_event("ClassFactory.LockServer");
        if flock.as_bool() {
            DLL_LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
        } else {
            DLL_LOCK_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

pub fn create_class_factory() -> IClassFactory {
    WinFaceUnlockClassFactory.into()
}

pub fn dll_server_lock_count() -> i32 {
    DLL_LOCK_COUNT.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_factory_can_be_created_as_com_interface() {
        let _factory = create_class_factory();
    }
}
