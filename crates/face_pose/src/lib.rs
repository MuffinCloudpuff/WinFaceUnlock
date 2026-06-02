use face_engine::{DetectedFace, FaceLandmark};
use video_provider::VideoFrame;

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FacePoseEstimate {
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub roll_deg: f32,
    pub left_eye_blink_score: Option<f32>,
    pub right_eye_blink_score: Option<f32>,
}

impl FacePoseEstimate {
    pub fn without_blink(yaw_deg: f32, pitch_deg: f32, roll_deg: f32) -> Self {
        Self {
            yaw_deg,
            pitch_deg,
            roll_deg,
            left_eye_blink_score: None,
            right_eye_blink_score: None,
        }
    }

    pub fn with_blink_scores(
        yaw_deg: f32,
        pitch_deg: f32,
        roll_deg: f32,
        left_eye_blink_score: f32,
        right_eye_blink_score: f32,
    ) -> Self {
        Self {
            yaw_deg,
            pitch_deg,
            roll_deg,
            left_eye_blink_score: Some(left_eye_blink_score.clamp(0.0, 1.0)),
            right_eye_blink_score: Some(right_eye_blink_score.clamp(0.0, 1.0)),
        }
    }

    pub fn both_eye_blink_score(self) -> Option<f32> {
        Some(
            self.left_eye_blink_score?
                .min(self.right_eye_blink_score?)
                .clamp(0.0, 1.0),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FacePoseError {
    NoFace,
    MultipleFaces,
    InsufficientLandmarks,
    ProviderUnavailable,
    InferenceFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FacePoseTarget {
    Frontal,
    YawLeftMild,
    YawRightMild,
    PitchDownMild,
    PitchUpMild,
    BlinkMotion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FacePoseCapabilities {
    pub estimates_head_pose: bool,
    pub estimates_blink: bool,
}

impl FacePoseCapabilities {
    pub const HEAD_POSE_ONLY: Self = Self {
        estimates_head_pose: true,
        estimates_blink: false,
    };

    pub const HEAD_POSE_AND_BLINK: Self = Self {
        estimates_head_pose: true,
        estimates_blink: true,
    };

    pub fn supports_target(self, target: FacePoseTarget) -> bool {
        match target {
            FacePoseTarget::BlinkMotion => self.estimates_blink,
            FacePoseTarget::Frontal
            | FacePoseTarget::YawLeftMild
            | FacePoseTarget::YawRightMild
            | FacePoseTarget::PitchDownMild
            | FacePoseTarget::PitchUpMild => self.estimates_head_pose,
        }
    }
}

pub trait FacePoseProvider {
    fn provider_name(&self) -> &'static str;

    fn capabilities(&self) -> FacePoseCapabilities;

    fn estimate_pose(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FacePoseEstimate, FacePoseError>;
}

impl<T> FacePoseProvider for Box<T>
where
    T: FacePoseProvider + ?Sized,
{
    fn provider_name(&self) -> &'static str {
        (**self).provider_name()
    }

    fn capabilities(&self) -> FacePoseCapabilities {
        (**self).capabilities()
    }

    fn estimate_pose(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FacePoseEstimate, FacePoseError> {
        (**self).estimate_pose(frame, face)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LandmarkFacePoseProvider;

impl FacePoseProvider for LandmarkFacePoseProvider {
    fn provider_name(&self) -> &'static str {
        "landmark"
    }

    fn capabilities(&self) -> FacePoseCapabilities {
        FacePoseCapabilities::HEAD_POSE_ONLY
    }

    fn estimate_pose(
        &mut self,
        _frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FacePoseEstimate, FacePoseError> {
        estimate_pose_from_five_landmarks(face).ok_or(FacePoseError::InsufficientLandmarks)
    }
}

pub fn pose_fit_score(target: FacePoseTarget, estimate: Option<FacePoseEstimate>) -> f32 {
    let Some(estimate) = estimate else {
        return 0.0;
    };

    let yaw = estimate.yaw_deg;
    let pitch = estimate.pitch_deg;
    let roll = estimate.roll_deg.abs();
    let roll_score = linear_score(roll, 0.0, 18.0);

    let target_score = match target {
        FacePoseTarget::Frontal => {
            linear_score(yaw.abs(), 0.0, 24.0) * linear_score(pitch.abs(), 0.0, 24.0)
        }
        FacePoseTarget::BlinkMotion => estimate.both_eye_blink_score().unwrap_or(0.0),
        FacePoseTarget::YawLeftMild => {
            range_score(yaw, -40.0, -4.0, -18.0) * linear_score(pitch.abs(), 0.0, 28.0)
        }
        FacePoseTarget::YawRightMild => {
            range_score(yaw, 4.0, 40.0, 18.0) * linear_score(pitch.abs(), 0.0, 28.0)
        }
        FacePoseTarget::PitchDownMild => {
            range_score(pitch, 4.0, 34.0, 16.0) * linear_score(yaw.abs(), 0.0, 30.0)
        }
        FacePoseTarget::PitchUpMild => {
            range_score(pitch, -34.0, -4.0, -16.0) * linear_score(yaw.abs(), 0.0, 30.0)
        }
    };

    (target_score * roll_score).clamp(0.0, 1.0)
}

pub fn estimate_pose_from_five_landmarks(face: &DetectedFace) -> Option<FacePoseEstimate> {
    if face.landmarks.len() < 5 {
        return None;
    }

    let left_eye = &face.landmarks[0];
    let right_eye = &face.landmarks[1];
    let nose = &face.landmarks[2];
    let left_mouth = &face.landmarks[3];
    let right_mouth = &face.landmarks[4];

    let eye_distance = distance(left_eye, right_eye);
    if eye_distance <= f32::EPSILON {
        return None;
    }

    let eye_mid_y = (left_eye.y + right_eye.y) * 0.5;
    let mouth_mid_y = (left_mouth.y + right_mouth.y) * 0.5;
    let eye_to_mouth = (mouth_mid_y - eye_mid_y).abs().max(eye_distance);

    let left_nose_distance = distance(left_eye, nose);
    let right_nose_distance = distance(right_eye, nose);
    let yaw_ratio = (right_nose_distance - left_nose_distance) / eye_distance;
    let yaw_deg = (yaw_ratio * 45.0).clamp(-45.0, 45.0);

    let nose_vertical_ratio = (nose.y - eye_mid_y) / eye_to_mouth;
    let pitch_deg = ((nose_vertical_ratio - 0.35) * 70.0).clamp(-35.0, 35.0);

    let roll_deg =
        (right_eye.y - left_eye.y).atan2(right_eye.x - left_eye.x) * 180.0 / std::f32::consts::PI;

    Some(FacePoseEstimate::without_blink(
        yaw_deg, pitch_deg, roll_deg,
    ))
}

fn distance(left: &FaceLandmark, right: &FaceLandmark) -> f32 {
    let dx = left.x - right.x;
    let dy = left.y - right.y;
    (dx.mul_add(dx, dy * dy)).sqrt()
}

fn linear_score(value: f32, best: f32, reject_at: f32) -> f32 {
    if value <= best {
        return 1.0;
    }
    if value >= reject_at {
        return 0.0;
    }
    1.0 - ((value - best) / (reject_at - best))
}

fn range_score(value: f32, min: f32, max: f32, ideal: f32) -> f32 {
    if value < min || value > max {
        return 0.0;
    }
    let distance_from_ideal = (value - ideal).abs();
    let max_distance = (ideal - min).abs().max((max - ideal).abs());
    1.0 - (distance_from_ideal / max_distance).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use face_engine::{DetectedFace, FaceBox, FaceLandmark};

    use super::*;

    #[test]
    fn landmark_provider_scores_frontal_face() {
        let face = DetectedFace {
            bounds: FaceBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            landmarks: vec![
                FaceLandmark { x: 30.0, y: 30.0 },
                FaceLandmark { x: 70.0, y: 30.0 },
                FaceLandmark { x: 50.0, y: 45.0 },
                FaceLandmark { x: 38.0, y: 68.0 },
                FaceLandmark { x: 62.0, y: 68.0 },
            ],
            confidence: 0.99,
        };

        let estimate = estimate_pose_from_five_landmarks(&face);

        assert!(pose_fit_score(FacePoseTarget::Frontal, estimate) > 0.6);
    }

    #[test]
    fn landmark_provider_does_not_claim_blink_support() {
        let provider = LandmarkFacePoseProvider;

        assert!(
            !provider
                .capabilities()
                .supports_target(FacePoseTarget::BlinkMotion)
        );
    }

    #[test]
    fn blink_target_uses_explicit_blink_signal() {
        let no_blink = FacePoseEstimate::without_blink(0.0, 0.0, 0.0);
        let blink = FacePoseEstimate::with_blink_scores(0.0, 0.0, 0.0, 0.8, 0.75);

        assert_eq!(
            pose_fit_score(FacePoseTarget::BlinkMotion, Some(no_blink)),
            0.0
        );
        assert!(pose_fit_score(FacePoseTarget::BlinkMotion, Some(blink)) > 0.7);
    }
}
