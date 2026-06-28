use video_provider::{
    CameraId, OpenCvCameraProvider, OpenCvCameraProviderConfig, VideoFrameProvider,
};

use crate::{
    presence_mediapipe_pose_detector::{
        MediaPipePosePresenceDetector, MediaPipePosePresenceDetectorConfig,
    },
    presence_monitor::{PresenceMonitorError, PresenceObservationSource},
    presence_policy::PresenceObservation,
};

pub struct MediaPipePoseCameraPresenceObservationConfig {
    pub camera_id: CameraId,
    pub camera_config: OpenCvCameraProviderConfig,
    pub detector_config: MediaPipePosePresenceDetectorConfig,
}

pub struct MediaPipePoseCameraPresenceObservationSource {
    camera_provider: OpenCvCameraProvider,
    detector: MediaPipePosePresenceDetector,
}

impl MediaPipePoseCameraPresenceObservationSource {
    pub fn new(
        config: MediaPipePoseCameraPresenceObservationConfig,
    ) -> Result<Self, PresenceMonitorError> {
        let mut camera_provider = OpenCvCameraProvider::new(config.camera_config);
        camera_provider
            .open(&config.camera_id)
            .map_err(|_| PresenceMonitorError::ObservationFailed)?;

        let mut detector = MediaPipePosePresenceDetector::new(config.detector_config);
        detector
            .load_model()
            .map_err(|_| PresenceMonitorError::ObservationFailed)?;

        Ok(Self {
            camera_provider,
            detector,
        })
    }
}

impl PresenceObservationSource for MediaPipePoseCameraPresenceObservationSource {
    fn next_observation(&mut self) -> Result<Option<PresenceObservation>, PresenceMonitorError> {
        let frame = match self.camera_provider.read_frame() {
            Ok(frame) => frame,
            Err(_) => return Ok(Some(PresenceObservation::CameraUnavailable)),
        };
        let Some(estimate) = self
            .detector
            .detect_presence(&frame)
            .map_err(|_| PresenceMonitorError::ObservationFailed)?
        else {
            return Ok(Some(PresenceObservation::PersonAbsent));
        };

        Ok(Some(PresenceObservation::PersonPresent {
            confidence: estimate.confidence,
            bbox_center_x_ratio: estimate.bbox_center_x_ratio,
            bbox_area_ratio: estimate.bbox_area_ratio,
        }))
    }
}

impl Drop for MediaPipePoseCameraPresenceObservationSource {
    fn drop(&mut self) {
        self.camera_provider.close();
        self.detector.unload_model();
    }
}
