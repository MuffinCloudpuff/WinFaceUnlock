use std::{
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use common_protocol::ProtocolError;
use opencv::{
    prelude::{MatTraitConst, VideoCaptureTrait, VideoCaptureTraitConst},
    videoio::VideoCapture,
};
use video_provider::{
    CameraId, OpenCvCameraBackend, OpenCvCameraProvider, OpenCvCameraProviderConfig,
    VideoFrameProvider,
};

use crate::{
    camera_runtime::{CameraLeaseKind, try_acquire_camera_lease},
    service_log::write_service_event_detail,
};

const MAX_USABLE_BACKEND_OPEN_MS: u128 = 3_000;

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
    #[serde(default = "default_probe_status")]
    pub last_probe_status: CameraBackendProbeStatus,
    #[serde(default)]
    pub last_probe_reason: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
pub struct CameraBackendProfileStore {
    pub profiles: Vec<CameraBackendProfile>,
}

impl CameraBackendProfileStore {
    pub fn load() -> Self {
        fs::read(profile_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
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

    fn remove_profile_for_camera(&mut self, camera_id: &CameraId) {
        self.profiles
            .retain(|existing| existing.camera_id != camera_id.0);
    }
}

impl CameraBackendProfile {
    fn is_usable(&self) -> bool {
        self.open_ms <= MAX_USABLE_BACKEND_OPEN_MS
            && self.last_probe_status == CameraBackendProbeStatus::Usable
    }
}

#[derive(Clone, Debug, Default, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraBackendProbeStatus {
    #[default]
    Usable,
    Degraded,
}

fn default_probe_status() -> CameraBackendProbeStatus {
    CameraBackendProbeStatus::Usable
}

pub fn apply_profile_to_config(camera_id: &CameraId, config: &mut OpenCvCameraProviderConfig) {
    config.preferred_backend = CameraBackendProfileStore::load().preferred_backend_for(camera_id);
}

pub fn spawn_camera_backend_profile_refresh() {
    let _ = thread::Builder::new()
        .name("winfaceunlock-camera-backend-profiler".to_owned())
        .spawn(|| {
            if let Err(error) = refresh_camera_backend_profiles() {
                write_service_event_detail(
                    "CameraBackendProfiles.RefreshFailed",
                    format!("error={error:?}"),
                );
            }
            poll_camera_device_changes();
        });
}

fn poll_camera_device_changes() {
    let mut last_signature = camera_list_signature();
    loop {
        thread::sleep(Duration::from_secs(30));
        let signature = camera_list_signature();
        if signature == last_signature {
            continue;
        }
        last_signature = signature;
        write_service_event_detail("CameraBackendProfiles.DeviceChangeDetected", "");
        if let Err(error) = refresh_camera_backend_profiles() {
            write_service_event_detail(
                "CameraBackendProfiles.DeviceChangeRefreshFailed",
                format!("error={error:?}"),
            );
        }
    }
}

fn camera_list_signature() -> String {
    let provider = OpenCvCameraProvider::with_default_config();
    provider
        .list_sources()
        .map(|sources| {
            sources
                .into_iter()
                .map(|source| format!("{}={}", source.id.0, source.display_name))
                .collect::<Vec<_>>()
                .join("|")
        })
        .unwrap_or_default()
}

pub fn refresh_camera_backend_profiles() -> Result<(), ProtocolError> {
    write_service_event_detail("CameraBackendProfiles.RefreshStarted", "");
    let _camera_lease = match try_acquire_camera_lease(CameraLeaseKind::BackendProfiling) {
        Ok(lease) => lease,
        Err(reason) => {
            write_service_event_detail(
                "CameraBackendProfiles.RefreshSkipped",
                format!("reason=camera-lease-denied detail={reason:?}"),
            );
            return Ok(());
        }
    };
    let provider = OpenCvCameraProvider::with_default_config();
    let sources = provider
        .list_sources()
        .map_err(|_| ProtocolError::TransportUnavailable)?;
    let mut store = CameraBackendProfileStore::load();

    for source in sources {
        let existing_profile = store
            .profiles
            .iter()
            .find(|profile| profile.camera_id == source.id.0)
            .cloned();
        let Some(profile) = profile_camera(&source.id, &source.display_name) else {
            write_service_event_detail(
                "CameraBackendProfiles.CameraSkipped",
                format!(
                    "camera_id={} display_name={}",
                    source.id.0, source.display_name
                ),
            );
            continue;
        };
        merge_profile_candidate(&mut store, existing_profile.as_ref(), profile);
    }

    store.save()?;
    write_service_event_detail(
        "CameraBackendProfiles.RefreshCompleted",
        format!("profile_count={}", store.profiles.len()),
    );
    Ok(())
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
    store.remove_profile_for_camera(&CameraId(candidate.camera_id.clone()));
}

fn profile_camera(camera_id: &CameraId, display_name: &str) -> Option<CameraBackendProfile> {
    let camera_index = camera_id.camera_index().ok()?;
    OpenCvCameraBackend::all()
        .into_iter()
        .filter_map(|backend| probe_backend(camera_index, backend))
        .min_by_key(|probe| probe.open_ms)
        .map(|probe| CameraBackendProfile {
            camera_id: camera_id.0.clone(),
            display_name: display_name.to_owned(),
            preferred_backend: probe.backend,
            open_ms: probe.open_ms,
            read_ms: probe.read_ms,
            frame_width: probe.frame_width,
            frame_height: probe.frame_height,
            measured_at_unix_ms: timestamp_unix_ms(),
            last_probe_status: if probe.open_ms <= MAX_USABLE_BACKEND_OPEN_MS {
                CameraBackendProbeStatus::Usable
            } else {
                CameraBackendProbeStatus::Degraded
            },
            last_probe_reason: Some(if probe.open_ms <= MAX_USABLE_BACKEND_OPEN_MS {
                "fastest-readable-backend".to_owned()
            } else {
                "fastest-readable-backend-too-slow".to_owned()
            }),
        })
}

struct BackendProbe {
    backend: OpenCvCameraBackend,
    open_ms: u128,
    read_ms: u128,
    frame_width: i32,
    frame_height: i32,
}

fn probe_backend(camera_index: i32, backend: OpenCvCameraBackend) -> Option<BackendProbe> {
    let open_started = Instant::now();
    let mut capture = VideoCapture::new(camera_index, backend.videoio_id()).ok()?;
    let open_ms = open_started.elapsed().as_millis();
    if !capture.is_opened().ok()? {
        let _ = capture.release();
        return None;
    }

    let read_started = Instant::now();
    let mut frame = opencv::core::Mat::default();
    let read_ok = capture.read(&mut frame).ok()?;
    let read_ms = read_started.elapsed().as_millis();
    let frame_width = frame.cols();
    let frame_height = frame.rows();
    let frame_is_empty = frame.empty();
    let _ = capture.release();
    if !read_ok || frame_is_empty || frame_width <= 0 || frame_height <= 0 {
        return None;
    }

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
            last_probe_status: CameraBackendProbeStatus::Degraded,
            last_probe_reason: Some("fastest-readable-backend-too-slow".to_owned()),
        };
        let mut store = CameraBackendProfileStore {
            profiles: vec![existing.clone()],
        };

        merge_profile_candidate(&mut store, Some(&existing), candidate);

        assert_eq!(store.profiles, vec![existing]);
    }

    #[test]
    fn slow_candidate_without_existing_profile_is_not_persisted() {
        let candidate = CameraBackendProfile {
            camera_id: "opencv-index:1".to_owned(),
            display_name: "C920".to_owned(),
            preferred_backend: OpenCvCameraBackend::Any,
            open_ms: 23_745,
            read_ms: 296,
            frame_width: 640,
            frame_height: 480,
            measured_at_unix_ms: 2,
            last_probe_status: CameraBackendProbeStatus::Degraded,
            last_probe_reason: Some("fastest-readable-backend-too-slow".to_owned()),
        };
        let mut store = CameraBackendProfileStore::default();

        merge_profile_candidate(&mut store, None, candidate);

        assert!(store.profiles.is_empty());
    }
}
