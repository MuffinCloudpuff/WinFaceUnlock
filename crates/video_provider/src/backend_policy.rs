//! Shared Windows backend order for foreground capture and background profiling.
use crate::OpenCvCameraBackend;

/// A measured preference wins. Without one, try DirectShow before MSMF so a
/// cold profile (including migration from legacy indices) does not first pay
/// an MSMF initialization timeout when that backend is slow. Keep both fallbacks
/// explicit: CAP_ANY cannot preserve backend-local device identity mapping.
pub fn windows_camera_backend_order(
    preferred: Option<OpenCvCameraBackend>,
) -> [OpenCvCameraBackend; 2] {
    use OpenCvCameraBackend::{Dshow, Msmf};
    match preferred {
        Some(Msmf) => [Msmf, Dshow],
        _ => [Dshow, Msmf],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_or_legacy_any_profile_does_not_wait_on_msmf_first() {
        for preference in [None, Some(OpenCvCameraBackend::Any)] {
            let mut attempted = Vec::new();
            let selected = windows_camera_backend_order(preference)
                .into_iter()
                .find(|backend| {
                    attempted.push(*backend);
                    *backend == OpenCvCameraBackend::Dshow
                });
            assert_eq!(selected, Some(OpenCvCameraBackend::Dshow));
            assert_eq!(attempted, [OpenCvCameraBackend::Dshow]);
        }
    }

    #[test]
    fn measured_preference_and_other_backend_fallback_are_preserved() {
        for preferred in [OpenCvCameraBackend::Dshow, OpenCvCameraBackend::Msmf] {
            let order = windows_camera_backend_order(Some(preferred));
            assert_eq!(order[0], preferred);
            assert_ne!(order[0], order[1]);
            assert!(!order.contains(&OpenCvCameraBackend::Any));
        }
    }
}
