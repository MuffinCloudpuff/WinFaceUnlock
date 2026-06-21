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

use crate::service_log::write_service_event_detail;

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
            .map(|profile| profile.preferred_backend)
    }
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
    let provider = OpenCvCameraProvider::with_default_config();
    let sources = provider
        .list_sources()
        .map_err(|_| ProtocolError::TransportUnavailable)?;
    let mut store = CameraBackendProfileStore::default();

    for source in sources {
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
        write_service_event_detail(
            "CameraBackendProfiles.CameraProfiled",
            format!(
                "camera_id={} backend={} open_ms={} read_ms={}",
                profile.camera_id,
                profile.preferred_backend.as_str(),
                profile.open_ms,
                profile.read_ms
            ),
        );
        store.profiles.push(profile);
    }

    store.save()?;
    write_service_event_detail(
        "CameraBackendProfiles.RefreshCompleted",
        format!("profile_count={}", store.profiles.len()),
    );
    Ok(())
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
        .and_then(|path| path.parent().map(|parent| parent.join("config")))
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData\WinFaceUnlock"))
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
            }],
        };

        assert_eq!(
            store.preferred_backend_for(&CameraId("opencv-index:1".to_owned())),
            Some(OpenCvCameraBackend::Dshow)
        );
    }
}
