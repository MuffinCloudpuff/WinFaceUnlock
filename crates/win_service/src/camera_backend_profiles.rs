use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use common_protocol::ProtocolError;
use video_provider::{
    CameraId, CameraInfo, OpenCvCameraBackend, OpenCvCameraProvider, OpenCvCameraProviderConfig,
    VideoFrameProvider,
};

use crate::{
    camera_runtime::{
        CameraLeaseKind, InterfaceRuntimeState, interface_runtime_state, try_acquire_camera_lease,
    },
    service_log::write_service_event_detail,
};

const MAX_USABLE_BACKEND_OPEN_MS: u128 = 3_000;
const USABLE_PROFILE_TTL_MS: u128 = 24 * 60 * 60 * 1_000;
const FAILED_PROFILE_RETRY_MS: u128 = 5 * 60 * 1_000;

#[derive(Clone, Debug, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
pub struct CameraBackendProfile {
    pub camera_id: String,
    pub display_name: String,
    pub preferred_backend: OpenCvCameraBackend,
    pub open_ms: u128,
    pub read_ms: u128,
    pub frame_width: i32,
    pub frame_height: i32,
    pub measured_at_unix_ms: u128,
    #[serde(default)]
    pub last_attempt_unix_ms: Option<u128>,
    #[serde(default = "default_probe_status")]
    pub last_probe_status: CameraBackendProbeStatus,
    #[serde(default)]
    pub last_probe_reason: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
pub struct CameraBackendProfileStore {
    pub profiles: Vec<CameraBackendProfile>,
}

#[derive(Default)]
struct ProfileCache {
    stamp: Option<(SystemTime, u64)>,
    store: CameraBackendProfileStore,
}

static PROFILE_CACHE: OnceLock<Mutex<ProfileCache>> = OnceLock::new();

impl CameraBackendProfileStore {
    pub fn load() -> Self {
        let path = profile_path();
        let stamp = fs::metadata(&path)
            .ok()
            .and_then(|meta| meta.modified().ok().map(|modified| (modified, meta.len())));
        let mut cache = PROFILE_CACHE
            .get_or_init(|| Mutex::new(ProfileCache::default()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if stamp.is_some() && stamp == cache.stamp {
            return cache.store.clone();
        }
        // No sleep/retry on the authentication path. A concurrent writer or
        // transient file error retains the last readable snapshot.
        if let Ok(bytes) = fs::read(path)
            && let Ok(store) = serde_json::from_slice(&bytes)
        {
            cache.store = store;
            cache.stamp = stamp;
        }
        cache.store.clone()
    }

    pub fn save(&self) -> Result<(), ProtocolError> {
        let path = profile_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| ProtocolError::TransportUnavailable)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| ProtocolError::InvalidMessage)?;
        fs::write(path, bytes).map_err(|_| ProtocolError::TransportUnavailable)
    }

    pub fn preferred_backend_for(&self, camera_id: &CameraId) -> Option<OpenCvCameraBackend> {
        self.profiles
            .iter()
            .find(|profile| profile.camera_id == camera_id.0)
            .filter(|profile| profile.is_usable())
            .map(|profile| profile.preferred_backend)
    }

    fn replace_profile(&mut self, profile: CameraBackendProfile) {
        self.profiles
            .retain(|existing| existing.camera_id != profile.camera_id);
        self.profiles.push(profile);
    }

    fn refresh_due(&self, camera_id: &CameraId, now: u128) -> bool {
        self.profiles
            .iter()
            .find(|profile| profile.camera_id == camera_id.0)
            .is_none_or(|profile| profile.refresh_due(now))
    }
}

impl CameraBackendProfile {
    fn is_usable(&self) -> bool {
        self.open_ms <= MAX_USABLE_BACKEND_OPEN_MS
            && self.last_probe_status == CameraBackendProbeStatus::Usable
    }

    fn refresh_due(&self, now: u128) -> bool {
        let last_attempt = self
            .last_attempt_unix_ms
            .unwrap_or(self.measured_at_unix_ms);
        let retrying = !self.is_usable()
            || self
                .last_probe_reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("retry-"));
        let ttl = if retrying {
            FAILED_PROFILE_RETRY_MS
        } else {
            USABLE_PROFILE_TTL_MS
        };
        // Clock rollback must not make a profile fresh indefinitely.
        now < last_attempt || now.saturating_sub(last_attempt) >= ttl
    }
}

#[derive(Clone, Debug, Default, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraBackendProbeStatus {
    #[default]
    Usable,
    Degraded,
    Unavailable,
}

fn default_probe_status() -> CameraBackendProbeStatus {
    CameraBackendProbeStatus::Usable
}

pub fn apply_profile_to_config(camera_id: &CameraId, config: &mut OpenCvCameraProviderConfig) {
    let store = CameraBackendProfileStore::load();
    config.preferred_backend = store.preferred_backend_for(camera_id);
}

/// Explicit diagnostic refresh only. Reading a profile or unlocking must never
/// start camera enumeration/stream probes behind the user's input sampling gate.
pub fn refresh_camera_backend_profiles() -> Result<(), ProtocolError> {
    refresh_when_desktop_is_unlocked(
        interface_runtime_state(),
        try_refresh_camera_backend_profiles,
    )
}

enum ProfileRefreshOutcome {
    Completed,
    Deferred,
}

fn refresh_when_desktop_is_unlocked(
    state: InterfaceRuntimeState,
    refresh: impl FnOnce() -> Result<ProfileRefreshOutcome, ProtocolError>,
) -> Result<(), ProtocolError> {
    if state != InterfaceRuntimeState::DesktopUnlocked {
        return Err(ProtocolError::TransportUnavailable);
    }
    match refresh()? {
        ProfileRefreshOutcome::Completed => Ok(()),
        ProfileRefreshOutcome::Deferred => Err(ProtocolError::TransportUnavailable),
    }
}
fn try_refresh_camera_backend_profiles() -> Result<ProfileRefreshOutcome, ProtocolError> {
    let sources = OpenCvCameraProvider::with_default_config()
        .list_sources()
        .map_err(|_| ProtocolError::TransportUnavailable)?;
    try_refresh_camera_backend_profiles_for_sources(&sources, true)
}

fn try_refresh_camera_backend_profiles_for_sources(
    sources: &[CameraInfo],
    force: bool,
) -> Result<ProfileRefreshOutcome, ProtocolError> {
    write_service_event_detail("CameraBackendProfiles.RefreshStarted", "");
    let mut store = CameraBackendProfileStore::load();
    let mut changed = false;

    for source in sources {
        if !force && !store.refresh_due(&source.id, timestamp_unix_ms()) {
            continue;
        }
        let existing_profile = store
            .profiles
            .iter()
            .find(|profile| profile.camera_id == source.id.0)
            .cloned();
        let Ok(profile) =
            profile_camera(&source.id, &source.display_name, existing_profile.as_ref())
        else {
            // Save completed cameras, but do not cache a lock transition as a
            // failed or successful probe of the interrupted device.
            if changed {
                store.save()?;
            }
            return Ok(ProfileRefreshOutcome::Deferred);
        };
        merge_profile_candidate(&mut store, existing_profile.as_ref(), profile);
        changed = true;
    }

    if changed {
        store.save()?;
    }
    write_service_event_detail(
        "CameraBackendProfiles.RefreshCompleted",
        format!("profile_count={}", store.profiles.len()),
    );
    // Failed devices have persisted retry deadlines; don't retry them every
    // unlock or every 30-second device poll.
    Ok(ProfileRefreshOutcome::Completed)
}

fn merge_profile_candidate(
    store: &mut CameraBackendProfileStore,
    existing_profile: Option<&CameraBackendProfile>,
    candidate: CameraBackendProfile,
) {
    if candidate.is_usable() {
        write_service_event_detail(
            "CameraBackendProfiles.ProfileUpdated",
            format!(
                "camera_id={} backend={} open_ms={} read_ms={} previous_backend={} previous_open_ms={}",
                candidate.camera_id,
                candidate.preferred_backend.as_str(),
                candidate.open_ms,
                candidate.read_ms,
                existing_profile
                    .map(|profile| profile.preferred_backend.as_str())
                    .unwrap_or("<none>"),
                existing_profile
                    .map(|profile| profile.open_ms.to_string())
                    .unwrap_or_else(|| "<none>".to_owned())
            ),
        );
        store.replace_profile(candidate);
        return;
    }

    if let Some(existing_profile) =
        existing_profile.filter(|profile| CameraBackendProfile::is_usable(profile))
    {
        write_service_event_detail(
            "CameraBackendProfiles.ProfileKept",
            format!(
                "camera_id={} kept_backend={} kept_open_ms={} candidate_backend={} candidate_open_ms={} reason=candidate-too-slow",
                existing_profile.camera_id,
                existing_profile.preferred_backend.as_str(),
                existing_profile.open_ms,
                candidate.preferred_backend.as_str(),
                candidate.open_ms
            ),
        );
        let mut retained = existing_profile.clone();
        retained.last_attempt_unix_ms = Some(candidate.measured_at_unix_ms);
        retained.last_probe_reason = Some("retry-previous-backend-retained".to_owned());
        store.replace_profile(retained);
        return;
    }

    write_service_event_detail(
        "CameraBackendProfiles.ProfileDegraded",
        format!(
            "camera_id={} candidate_backend={} candidate_open_ms={} reason=candidate-too-slow-no-usable-existing",
            candidate.camera_id,
            candidate.preferred_backend.as_str(),
            candidate.open_ms
        ),
    );
    store.replace_profile(candidate);
}

fn profile_camera(
    camera_id: &CameraId,
    display_name: &str,
    existing: Option<&CameraBackendProfile>,
) -> Result<CameraBackendProfile, ProfileProbeDeferred> {
    let backends = video_provider::windows_camera_backend_order(
        existing.map(|profile| profile.preferred_backend),
    );

    let best_probe = first_usable_probe(backends, |backend| {
        // Release the lease after every backend. Locking stops the pass before
        // it can start another backend/device, even after a timed-out driver.
        let _lease = try_acquire_camera_lease(CameraLeaseKind::BackendProfiling)
            .map_err(|_| ProfileProbeDeferred)?;
        let probe = probe_backend(camera_id, backend);
        if interface_runtime_state() != InterfaceRuntimeState::DesktopUnlocked {
            return Err(ProfileProbeDeferred);
        }
        Ok(probe)
    })?;

    let measured_at_unix_ms = timestamp_unix_ms();
    let Some(probe) = best_probe else {
        return Ok(CameraBackendProfile {
            camera_id: camera_id.0.clone(),
            display_name: display_name.to_owned(),
            preferred_backend: backends[0],
            open_ms: 0,
            read_ms: 0,
            frame_width: 0,
            frame_height: 0,
            measured_at_unix_ms,
            last_attempt_unix_ms: Some(measured_at_unix_ms),
            last_probe_status: CameraBackendProbeStatus::Unavailable,
            last_probe_reason: Some("no-readable-backend".to_owned()),
        });
    };
    Ok(CameraBackendProfile {
        camera_id: camera_id.0.clone(),
        display_name: display_name.to_owned(),
        preferred_backend: probe.backend,
        open_ms: probe.open_ms,
        read_ms: probe.read_ms,
        frame_width: probe.frame_width,
        frame_height: probe.frame_height,
        measured_at_unix_ms,
        last_attempt_unix_ms: Some(measured_at_unix_ms),
        last_probe_status: if probe.open_ms <= MAX_USABLE_BACKEND_OPEN_MS {
            CameraBackendProbeStatus::Usable
        } else {
            CameraBackendProbeStatus::Degraded
        },
        last_probe_reason: Some(if probe.open_ms <= MAX_USABLE_BACKEND_OPEN_MS {
            "first-usable-readable-backend".to_owned()
        } else {
            "fastest-readable-backend-too-slow".to_owned()
        }),
    })
}

#[derive(Debug)]
struct ProfileProbeDeferred;

fn first_usable_probe(
    backends: impl IntoIterator<Item = OpenCvCameraBackend>,
    mut probe_backend: impl FnMut(
        OpenCvCameraBackend,
    ) -> Result<Option<BackendProbe>, ProfileProbeDeferred>,
) -> Result<Option<BackendProbe>, ProfileProbeDeferred> {
    let mut best_probe: Option<BackendProbe> = None;
    for backend in backends {
        if let Some(probe) = probe_backend(backend)? {
            let found_usable = probe.open_ms <= MAX_USABLE_BACKEND_OPEN_MS;
            if best_probe
                .as_ref()
                .is_none_or(|existing| probe.open_ms < existing.open_ms)
            {
                best_probe = Some(probe);
            }
            if found_usable {
                break;
            }
        }
    }
    Ok(best_probe)
}

struct BackendProbe {
    backend: OpenCvCameraBackend,
    open_ms: u128,
    read_ms: u128,
    frame_width: i32,
    frame_height: i32,
}

fn probe_backend(camera_id: &CameraId, backend: OpenCvCameraBackend) -> Option<BackendProbe> {
    let open_started = Instant::now();
    let mut capture = OpenCvCameraProvider::with_default_config();
    capture.open_backend(camera_id, backend).ok()?;
    let open_ms = open_started.elapsed().as_millis();

    let read_started = Instant::now();
    let frame = capture.read_frame().ok()?;
    let read_ms = read_started.elapsed().as_millis();
    let frame_width = frame.width as i32;
    let frame_height = frame.height as i32;
    capture.close();

    Some(BackendProbe {
        backend,
        open_ms,
        read_ms,
        frame_width,
        frame_height,
    })
}

fn profile_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("runtime")))
        .unwrap_or_else(|| std::env::temp_dir().join("WinFaceUnlock").join("runtime"))
        .join("camera_backend_profiles.json")
}

fn timestamp_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_refresh_never_enumerates_in_unknown_or_locked_session() {
        for state in [
            InterfaceRuntimeState::Unknown,
            InterfaceRuntimeState::LockedOrLogon,
        ] {
            let mut called = false;
            let result = refresh_when_desktop_is_unlocked(state, || {
                called = true;
                Ok(ProfileRefreshOutcome::Completed)
            });
            assert!(result.is_err());
            assert!(!called);
        }
    }

    #[test]
    fn interrupted_explicit_refresh_does_not_schedule_hidden_retries() {
        let mut calls = 0;
        let result =
            refresh_when_desktop_is_unlocked(InterfaceRuntimeState::DesktopUnlocked, || {
                calls += 1;
                Ok(ProfileRefreshOutcome::Deferred)
            });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn lock_transition_defers_instead_of_probing_another_backend() {
        let mut attempted = Vec::new();
        let result = first_usable_probe(
            [OpenCvCameraBackend::Msmf, OpenCvCameraBackend::Dshow],
            |backend| {
                attempted.push(backend);
                Err(ProfileProbeDeferred)
            },
        );
        assert!(result.is_err());
        assert_eq!(attempted, vec![OpenCvCameraBackend::Msmf]);
    }

    #[test]
    fn first_fast_readable_backend_short_circuits_remaining_probes()
    -> Result<(), ProfileProbeDeferred> {
        let mut attempts = 0;
        let probe = first_usable_probe(
            [OpenCvCameraBackend::Dshow, OpenCvCameraBackend::Msmf],
            |backend| {
                attempts += 1;
                Ok(Some(BackendProbe {
                    backend,
                    open_ms: 200,
                    read_ms: 1,
                    frame_width: 640,
                    frame_height: 480,
                }))
            },
        )?;
        assert_eq!(attempts, 1);
        assert!(matches!(
            probe.map(|probe| probe.backend),
            Some(OpenCvCameraBackend::Dshow)
        ));
        Ok(())
    }

    #[test]
    fn preferred_backend_reads_matching_camera_id() {
        let store = CameraBackendProfileStore {
            profiles: vec![CameraBackendProfile {
                camera_id: "opencv-index:1".to_owned(),
                display_name: "C920".to_owned(),
                preferred_backend: OpenCvCameraBackend::Dshow,
                open_ms: 1,
                read_ms: 1,
                frame_width: 640,
                frame_height: 480,
                measured_at_unix_ms: 1,
                last_attempt_unix_ms: None,
                last_probe_status: CameraBackendProbeStatus::Usable,
                last_probe_reason: Some("fastest-readable-backend".to_owned()),
            }],
        };

        assert_eq!(
            store.preferred_backend_for(&CameraId("opencv-index:1".to_owned())),
            Some(OpenCvCameraBackend::Dshow)
        );
    }

    #[test]
    fn degraded_profile_is_not_used_as_preferred_backend() {
        let store = CameraBackendProfileStore {
            profiles: vec![CameraBackendProfile {
                camera_id: "opencv-index:1".to_owned(),
                display_name: "C920".to_owned(),
                preferred_backend: OpenCvCameraBackend::Any,
                open_ms: MAX_USABLE_BACKEND_OPEN_MS + 1,
                read_ms: 1,
                frame_width: 640,
                frame_height: 480,
                measured_at_unix_ms: 1,
                last_attempt_unix_ms: None,
                last_probe_status: CameraBackendProbeStatus::Degraded,
                last_probe_reason: Some("fastest-readable-backend-too-slow".to_owned()),
            }],
        };

        assert_eq!(
            store.preferred_backend_for(&CameraId("opencv-index:1".to_owned())),
            None
        );
    }

    #[test]
    fn slow_candidate_keeps_existing_usable_profile() {
        let existing = CameraBackendProfile {
            camera_id: "opencv-index:1".to_owned(),
            display_name: "C920".to_owned(),
            preferred_backend: OpenCvCameraBackend::Dshow,
            open_ms: 200,
            read_ms: 100,
            frame_width: 640,
            frame_height: 480,
            measured_at_unix_ms: 1,
            last_attempt_unix_ms: None,
            last_probe_status: CameraBackendProbeStatus::Usable,
            last_probe_reason: Some("fastest-readable-backend".to_owned()),
        };
        let candidate = CameraBackendProfile {
            camera_id: "opencv-index:1".to_owned(),
            display_name: "C920".to_owned(),
            preferred_backend: OpenCvCameraBackend::Any,
            open_ms: 23_745,
            read_ms: 296,
            frame_width: 640,
            frame_height: 480,
            measured_at_unix_ms: 2,
            last_attempt_unix_ms: None,
            last_probe_status: CameraBackendProbeStatus::Degraded,
            last_probe_reason: Some("fastest-readable-backend-too-slow".to_owned()),
        };
        let mut store = CameraBackendProfileStore {
            profiles: vec![existing.clone()],
        };

        merge_profile_candidate(&mut store, Some(&existing), candidate);

        let kept = &store.profiles[0];
        assert_eq!(kept.preferred_backend, existing.preferred_backend);
        assert_eq!(kept.open_ms, existing.open_ms);
        assert_eq!(kept.measured_at_unix_ms, existing.measured_at_unix_ms);
        assert_eq!(kept.last_attempt_unix_ms, Some(2));
        assert!(!kept.refresh_due(2 + FAILED_PROFILE_RETRY_MS - 1));
        assert!(kept.refresh_due(2 + FAILED_PROFILE_RETRY_MS));
    }

    #[test]
    fn slow_candidate_is_cached_without_becoming_a_preferred_backend() {
        let candidate = CameraBackendProfile {
            camera_id: "opencv-index:1".to_owned(),
            display_name: "C920".to_owned(),
            preferred_backend: OpenCvCameraBackend::Any,
            open_ms: 23_745,
            read_ms: 296,
            frame_width: 640,
            frame_height: 480,
            measured_at_unix_ms: 2,
            last_attempt_unix_ms: None,
            last_probe_status: CameraBackendProbeStatus::Degraded,
            last_probe_reason: Some("fastest-readable-backend-too-slow".to_owned()),
        };
        let mut store = CameraBackendProfileStore::default();

        merge_profile_candidate(&mut store, None, candidate);

        assert_eq!(store.profiles.len(), 1);
        let id = CameraId("opencv-index:1".to_owned());
        assert_eq!(store.preferred_backend_for(&id), None);
        assert!(!store.refresh_due(&id, 2 + FAILED_PROFILE_RETRY_MS - 1));
        assert!(store.refresh_due(&id, 2 + FAILED_PROFILE_RETRY_MS));
    }

    #[test]
    fn usable_profile_does_not_refresh_on_each_authentication() {
        let profile = CameraBackendProfile {
            camera_id: "camera".to_owned(),
            display_name: "Camera".to_owned(),
            preferred_backend: OpenCvCameraBackend::Msmf,
            open_ms: 100,
            read_ms: 1,
            frame_width: 640,
            frame_height: 480,
            measured_at_unix_ms: 1_000,
            last_attempt_unix_ms: Some(1_000),
            last_probe_status: CameraBackendProbeStatus::Usable,
            last_probe_reason: Some("first-usable-readable-backend".to_owned()),
        };
        assert!(!profile.refresh_due(1_001));
        assert!(!profile.refresh_due(1_000 + USABLE_PROFILE_TTL_MS - 1));
        assert!(profile.refresh_due(1_000 + USABLE_PROFILE_TTL_MS));
        assert!(profile.refresh_due(999));
    }
}
