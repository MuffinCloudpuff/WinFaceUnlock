use std::path::PathBuf;

use face_pose_mediapipe::{
    MediaPipePresencePoseEstimate, MediaPipePresencePoseProvider,
    MediaPipePresencePoseProviderConfig,
};
use video_provider::VideoFrame;

#[derive(Clone, Debug, PartialEq)]
pub struct MediaPipePosePresenceDetectorConfig {
    pub bridge_dll_path: PathBuf,
    pub pose_landmarker_task_path: PathBuf,
    pub min_landmark_visibility: f32,
    pub min_landmark_presence: f32,
}

pub struct MediaPipePosePresenceDetector {
    config: MediaPipePosePresenceDetectorConfig,
    provider: Option<MediaPipePresencePoseProvider>,
}

impl MediaPipePosePresenceDetector {
    pub fn new(config: MediaPipePosePresenceDetectorConfig) -> Self {
        Self {
            config,
            provider: None,
        }
    }

    pub fn load_model(&mut self) -> Result<(), MediaPipePosePresenceDetectorError> {
        let mut provider_config = MediaPipePresencePoseProviderConfig::new(
            self.config.bridge_dll_path.clone(),
            self.config.pose_landmarker_task_path.clone(),
        );
        provider_config.min_landmark_visibility = self.config.min_landmark_visibility;
        provider_config.min_landmark_presence = self.config.min_landmark_presence;
        let provider = MediaPipePresencePoseProvider::load(provider_config)
            .map_err(|_| MediaPipePosePresenceDetectorError::ModelLoadFailed)?;
        self.provider = Some(provider);
        Ok(())
    }

    pub fn unload_model(&mut self) {
        self.provider = None;
    }

    pub fn detect_presence(
        &mut self,
        frame: &VideoFrame,
    ) -> Result<Option<MediaPipePresencePoseEstimate>, MediaPipePosePresenceDetectorError> {
        let provider = self
            .provider
            .as_mut()
            .ok_or(MediaPipePosePresenceDetectorError::ModelNotLoaded)?;
        provider
            .estimate_presence(frame)
            .map_err(|_| MediaPipePosePresenceDetectorError::InferenceFailed)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum MediaPipePosePresenceDetectorError {
    ModelLoadFailed,
    ModelNotLoaded,
    InferenceFailed,
}
