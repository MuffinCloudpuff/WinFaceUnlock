use std::{
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use crate::service_log::write_service_event_detail;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InterfaceRuntimeState {
    #[default]
    Unknown,
    LockedOrLogon,
    DesktopUnlocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CameraLeaseKind {
    LogonAuthentication,
    PresenceLock,
    BackendProfiling,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CameraLeaseDeniedReason {
    InterfaceStateDoesNotAllowCamera,
    CameraAlreadyLeased,
    RuntimeStateUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterfaceRuntimeStateSource {
    ServiceSessionLogon,
    ServiceSessionUnlock,
    ServiceSessionLock,
    ServiceSessionEnd,
    DesktopControlReload,
}

#[derive(Debug)]
pub struct CameraLease {
    kind: CameraLeaseKind,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct CameraRuntimeState {
    interface_state: InterfaceRuntimeState,
    active_lease: Option<CameraLeaseKind>,
}

static CAMERA_RUNTIME_STATE: OnceLock<Mutex<CameraRuntimeState>> = OnceLock::new();

pub fn interface_runtime_state() -> InterfaceRuntimeState {
    cached_interface_runtime_state()
}

fn cached_interface_runtime_state() -> InterfaceRuntimeState {
    runtime_state()
        .lock()
        .map(|state| state.interface_state)
        .unwrap_or(InterfaceRuntimeState::Unknown)
}

pub fn try_acquire_camera_lease(
    kind: CameraLeaseKind,
) -> Result<CameraLease, CameraLeaseDeniedReason> {
    let mut state = runtime_state()
        .lock()
        .map_err(|_| CameraLeaseDeniedReason::RuntimeStateUnavailable)?;
    if !lease_allowed_for_interface_state(kind, state.interface_state) {
        return Err(CameraLeaseDeniedReason::InterfaceStateDoesNotAllowCamera);
    }
    if state.active_lease.is_some() {
        return Err(CameraLeaseDeniedReason::CameraAlreadyLeased);
    }
    state.active_lease = Some(kind);
    Ok(CameraLease { kind })
}

pub fn acquire_camera_lease_until(
    kind: CameraLeaseKind,
    timeout: Duration,
) -> Result<CameraLease, CameraLeaseDeniedReason> {
    let deadline = Instant::now() + timeout;
    loop {
        match try_acquire_camera_lease(kind) {
            Ok(lease) => return Ok(lease),
            Err(CameraLeaseDeniedReason::CameraAlreadyLeased) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(reason) => return Err(reason),
        }
    }
}

pub fn update_interface_runtime_state(
    interface_state: InterfaceRuntimeState,
    source: InterfaceRuntimeStateSource,
) {
    if let Ok(mut state) = runtime_state().lock()
        && state.interface_state != interface_state
    {
        write_service_event_detail(
            "CameraRuntime.InterfaceStateChanged",
            format!(
                "previous={:?} current={:?} source={:?}",
                state.interface_state, interface_state, source
            ),
        );
        state.interface_state = interface_state;
    }
}

fn lease_allowed_for_interface_state(
    kind: CameraLeaseKind,
    interface_state: InterfaceRuntimeState,
) -> bool {
    match kind {
        CameraLeaseKind::LogonAuthentication => matches!(
            interface_state,
            InterfaceRuntimeState::LockedOrLogon | InterfaceRuntimeState::Unknown
        ),
        CameraLeaseKind::PresenceLock => {
            matches!(interface_state, InterfaceRuntimeState::DesktopUnlocked)
        }
        CameraLeaseKind::BackendProfiling => {
            matches!(interface_state, InterfaceRuntimeState::DesktopUnlocked)
        }
    }
}

impl Drop for CameraLease {
    fn drop(&mut self) {
        if let Ok(mut state) = runtime_state().lock()
            && state.active_lease == Some(self.kind)
        {
            state.active_lease = None;
        }
    }
}

fn runtime_state() -> &'static Mutex<CameraRuntimeState> {
    CAMERA_RUNTIME_STATE.get_or_init(|| Mutex::new(CameraRuntimeState::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_state_allows_presence_but_not_logon_auth() {
        let mut state = CameraRuntimeState {
            interface_state: InterfaceRuntimeState::DesktopUnlocked,
            active_lease: None,
        };

        assert!(lease_allowed_for_interface_state(
            CameraLeaseKind::PresenceLock,
            state.interface_state
        ));
        assert!(!lease_allowed_for_interface_state(
            CameraLeaseKind::LogonAuthentication,
            state.interface_state
        ));

        state.interface_state = InterfaceRuntimeState::LockedOrLogon;
        assert!(lease_allowed_for_interface_state(
            CameraLeaseKind::LogonAuthentication,
            state.interface_state
        ));
        assert!(!lease_allowed_for_interface_state(
            CameraLeaseKind::PresenceLock,
            state.interface_state
        ));
    }

    #[test]
    fn locked_state_blocks_backend_profiling() {
        assert!(!lease_allowed_for_interface_state(
            CameraLeaseKind::BackendProfiling,
            InterfaceRuntimeState::LockedOrLogon
        ));
        assert!(lease_allowed_for_interface_state(
            CameraLeaseKind::BackendProfiling,
            InterfaceRuntimeState::DesktopUnlocked
        ));
    }
}
