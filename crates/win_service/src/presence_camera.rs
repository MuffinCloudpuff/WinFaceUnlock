use std::path::PathBuf;

use face_auth::RecognitionTemplates;
use face_engine::{
    FaceMatchDecision, FaceModelProvider, FaceTemplateMatcher, OpenCvFaceModelConfig,
    OpenCvFaceModelProvider,
};
use video_provider::{
    CameraId, OpenCvCameraProvider, OpenCvCameraProviderConfig, VideoFrameProvider,
};

use crate::{
    presence_monitor::{PresenceMonitorError, PresenceObservationSource},
    presence_policy::PresenceObservation,
};

pub struct CameraPresenceObservationConfig {
    pub camera_id: CameraId,
    pub camera_config: OpenCvCameraProviderConfig,
    pub model_config: OpenCvFaceModelConfig,
    pub templates: RecognitionTemplates,
    pub presence_owner_match_threshold: f32,
    pub pending_unknown_face_crop_path: Option<PathBuf>,
}

pub struct CameraPresenceObservationSource {
    config: CameraPresenceObservationConfig,
    model_provider: OpenCvFaceModelProvider,
}

impl CameraPresenceObservationSource {
    pub fn new(config: CameraPresenceObservationConfig) -> Result<Self, PresenceMonitorError> {
        let mut model_provider = OpenCvFaceModelProvider::new(config.model_config.clone());
        model_provider
            .load_models()
            .map_err(|_| PresenceMonitorError::ObservationFailed)?;
        Ok(Self {
            config,
            model_provider,
        })
    }

    fn observe_once(&mut self) -> Result<PresenceObservation, PresenceMonitorError> {
        let mut camera_provider = OpenCvCameraProvider::new(self.config.camera_config.clone());
        if camera_provider.open(&self.config.camera_id).is_err() {
            return Ok(PresenceObservation::CameraUnavailable);
        }

        let frame = match camera_provider.read_frame() {
            Ok(frame) => frame,
            Err(_) => {
                camera_provider.close();
                return Ok(PresenceObservation::CameraUnavailable);
            }
        };
        camera_provider.close();

        let faces = self
            .model_provider
            .detect(&frame)
            .map_err(|_| PresenceMonitorError::ObservationFailed)?;
        match faces.len() {
            0 => Ok(PresenceObservation::NoFaceDetected),
            1 => {
                let face = &faces[0];
                let candidate = self
                    .model_provider
                    .extract(&frame, face)
                    .map_err(|_| PresenceMonitorError::ObservationFailed)?;
                let matcher = FaceTemplateMatcher::new(self.config.presence_owner_match_threshold);
                let recognition_model = self.model_provider.recognition_model().clone();
                let Some(best_match) = matcher.best_compatible_match(
                    self.config.templates.as_slice(),
                    &recognition_model,
                    &candidate,
                ) else {
                    return Err(PresenceMonitorError::ObservationFailed);
                };

                let observation =
                    presence_observation_from_match_decision(best_match.score, best_match.decision);
                if matches!(observation, PresenceObservation::UnknownFace { .. })
                    && let Some(path) = &self.config.pending_unknown_face_crop_path
                {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = self.model_provider.write_aligned_face(&frame, face, path);
                }
                Ok(observation)
            }
            _ => Ok(PresenceObservation::UnknownFace {
                owner_match_score: 0.0,
            }),
        }
    }
}

impl PresenceObservationSource for CameraPresenceObservationSource {
    fn next_observation(&mut self) -> Result<Option<PresenceObservation>, PresenceMonitorError> {
        self.observe_once().map(Some)
    }
}

fn presence_observation_from_match_decision(
    owner_match_score: f32,
    decision: FaceMatchDecision,
) -> PresenceObservation {
    match decision {
        FaceMatchDecision::MatchAccepted => PresenceObservation::OwnerPresent { owner_match_score },
        FaceMatchDecision::MatchRejectedBelowThreshold => {
            PresenceObservation::UnknownFace { owner_match_score }
        }
    }
}

#[cfg(test)]
mod tests {
    use face_engine::FaceMatchDecision;

    use super::*;

    #[test]
    fn accepted_match_maps_to_owner_present_observation() {
        let observation =
            presence_observation_from_match_decision(0.62, FaceMatchDecision::MatchAccepted);

        assert_eq!(
            observation,
            PresenceObservation::OwnerPresent {
                owner_match_score: 0.62
            }
        );
    }

    #[test]
    fn rejected_match_maps_to_unknown_face_observation() {
        let observation = presence_observation_from_match_decision(
            0.31,
            FaceMatchDecision::MatchRejectedBelowThreshold,
        );

        assert_eq!(
            observation,
            PresenceObservation::UnknownFace {
                owner_match_score: 0.31
            }
        );
    }
}
