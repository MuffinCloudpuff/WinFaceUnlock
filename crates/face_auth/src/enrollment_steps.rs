use face_engine::FacePoseGroup;
use face_pose::{FacePoseCapabilities, FacePoseTarget};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuidedEnrollmentStep {
    FrontalPrimary,
    YawLeftMild,
    YawRightMild,
    PitchDownMild,
    PitchUpMild,
    BlinkMotion,
}

impl GuidedEnrollmentStep {
    pub fn ordered_steps() -> &'static [Self] {
        &[
            Self::FrontalPrimary,
            Self::YawLeftMild,
            Self::YawRightMild,
            Self::PitchDownMild,
            Self::PitchUpMild,
            Self::BlinkMotion,
        ]
    }

    pub fn supported_ordered_steps(capabilities: FacePoseCapabilities) -> Vec<Self> {
        Self::ordered_steps()
            .iter()
            .copied()
            .filter(|step| capabilities.supports_target(step.pose_target()))
            .collect()
    }

    pub fn pose_group(self) -> FacePoseGroup {
        match self {
            Self::FrontalPrimary => FacePoseGroup::FrontalPrimary,
            Self::YawLeftMild => FacePoseGroup::YawLeftMild,
            Self::YawRightMild => FacePoseGroup::YawRightMild,
            Self::PitchDownMild => FacePoseGroup::PitchDownMild,
            Self::PitchUpMild => FacePoseGroup::PitchUpMild,
            Self::BlinkMotion => FacePoseGroup::BlinkMotion,
        }
    }

    pub fn pose_target(self) -> FacePoseTarget {
        match self {
            Self::FrontalPrimary => FacePoseTarget::Frontal,
            Self::YawLeftMild => FacePoseTarget::YawLeftMild,
            Self::YawRightMild => FacePoseTarget::YawRightMild,
            Self::PitchDownMild => FacePoseTarget::PitchDownMild,
            Self::PitchUpMild => FacePoseTarget::PitchUpMild,
            Self::BlinkMotion => FacePoseTarget::BlinkMotion,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::FrontalPrimary => "frontal_primary",
            Self::YawLeftMild => "yaw_left_mild",
            Self::YawRightMild => "yaw_right_mild",
            Self::PitchDownMild => "pitch_down_mild",
            Self::PitchUpMild => "pitch_up_mild",
            Self::BlinkMotion => "blink_motion",
        }
    }

    pub fn prompt(self) -> &'static str {
        match self {
            Self::FrontalPrimary => "请正脸看摄像头",
            Self::YawLeftMild => "请缓慢向左转头",
            Self::YawRightMild => "请缓慢向右转头",
            Self::PitchDownMild => "请稍微低头",
            Self::PitchUpMild => "请稍微抬头",
            Self::BlinkMotion => "请眨眼一次",
        }
    }
}
