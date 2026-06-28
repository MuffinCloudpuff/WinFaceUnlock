use crate::{FaceEmbedding, FaceMatch, FaceMatchDecision, FaceModelDescriptor, cosine_similarity};

#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceTemplateRef(pub String);

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize,
)]
pub enum FacePoseGroup {
    #[default]
    FrontalPrimary,
    FrontalVariant,
    YawLeftMild,
    YawRightMild,
    PitchDownMild,
    PitchUpMild,
    BlinkMotion,
    HardPoseDiagnostic,
    RejectedQuality,
}

impl FacePoseGroup {
    pub fn selected_for_unlock_by_default(self) -> bool {
        matches!(
            self,
            Self::FrontalPrimary
                | Self::FrontalVariant
                | Self::YawLeftMild
                | Self::YawRightMild
                | Self::PitchDownMild
                | Self::PitchUpMild
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum FaceSampleRejectReason {
    NoFaceDetected,
    MultipleFacesDetected,
    FaceTooSmall,
    FaceTooLargeOrClipped,
    BlurTooHigh,
    UnderExposed,
    OverExposed,
    BacklightTooStrong,
    LandmarkUnstable,
    PoseOutOfExpectedRange,
    AlignmentFailed,
    EmbeddingInconsistentWithPrimary,
    DuplicateTooSimilar,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceTemplateQualityScores {
    pub quality_score: f32,
    pub blur_score: f32,
    pub illumination_score: f32,
    pub face_size_score: f32,
    pub alignment_score: f32,
    pub pose_fit_score: f32,
    pub embedding_consistency_score: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceTemplateSampleMetadata {
    pub sample_id: String,
    pub pose_group: FacePoseGroup,
    pub source_step: String,
    pub frame_index: u32,
    pub frame_timestamp_ms: i64,
    pub face_box: crate::FaceBox,
    pub landmarks: Vec<crate::FaceLandmark>,
    pub detection_confidence: f32,
    pub pose_yaw_deg: f32,
    pub pose_pitch_deg: f32,
    pub pose_roll_deg: f32,
    pub quality: FaceTemplateQualityScores,
    pub selected_for_unlock: bool,
    pub reject_reason: Option<FaceSampleRejectReason>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceTemplate {
    pub template_ref: FaceTemplateRef,
    pub user_id: String,
    pub model_family: String,
    pub model_version: String,
    #[serde(default)]
    pub pose_group: FacePoseGroup,
    #[serde(default = "default_selected_for_unlock")]
    pub selected_for_unlock: bool,
    #[serde(default)]
    pub quality_score: Option<f32>,
    pub embedding: FaceEmbedding,
}

impl FaceTemplate {
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, FaceTemplateCodecError> {
        serde_json::to_vec(self).map_err(|_| FaceTemplateCodecError::SerializeFailed)
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, FaceTemplateCodecError> {
        serde_json::from_slice(bytes).map_err(|_| FaceTemplateCodecError::DeserializeFailed)
    }

    pub fn is_compatible_with(&self, recognition_model: &FaceModelDescriptor) -> bool {
        self.model_family == recognition_model.model_family
            && self.model_version == recognition_model.model_version
    }
}

fn default_selected_for_unlock() -> bool {
    true
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FaceTemplateCodecError {
    SerializeFailed,
    DeserializeFailed,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceTemplateThresholdProfile {
    pub template_acceptance_threshold: f32,
    pub frame_match_threshold: f32,
    pub required_consecutive_match_count: u32,
    pub min_quality_score: f32,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceTemplateSetQualitySummary {
    pub selected_template_count: usize,
    pub rejected_sample_count: usize,
    pub average_selected_quality_score: Option<f32>,
    pub minimum_selected_quality_score: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceTemplateSet {
    pub user_id: String,
    pub detector_model_family: String,
    pub detector_model_version: String,
    pub recognizer_model_family: String,
    pub recognizer_model_version: String,
    pub enrollment_id: String,
    pub enrollment_created_at_unix_ms: i64,
    pub threshold_profile: FaceTemplateThresholdProfile,
    pub templates: Vec<FaceTemplate>,
    pub sample_metadata: Vec<FaceTemplateSampleMetadata>,
    pub quality_summary: FaceTemplateSetQualitySummary,
}

impl FaceTemplateSet {
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, FaceTemplateCodecError> {
        serde_json::to_vec_pretty(self).map_err(|_| FaceTemplateCodecError::SerializeFailed)
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, FaceTemplateCodecError> {
        serde_json::from_slice(bytes).map_err(|_| FaceTemplateCodecError::DeserializeFailed)
    }

    pub fn selected_templates(&self) -> Vec<FaceTemplate> {
        self.templates
            .iter()
            .filter(|template| template.selected_for_unlock)
            .cloned()
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceTemplateMatch {
    pub template_ref: FaceTemplateRef,
    pub user_id: String,
    pub pose_group: FacePoseGroup,
    pub score: f32,
    pub decision: FaceMatchDecision,
}

pub struct FaceTemplateMatcher {
    threshold: f32,
}

impl FaceTemplateMatcher {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }

    pub fn compare_embeddings(
        &self,
        enrolled: &FaceEmbedding,
        candidate: &FaceEmbedding,
    ) -> FaceMatch {
        let score = cosine_similarity(&enrolled.values, &candidate.values).unwrap_or(0.0);
        let decision = if score >= self.threshold {
            FaceMatchDecision::MatchAccepted
        } else {
            FaceMatchDecision::MatchRejectedBelowThreshold
        };

        FaceMatch { score, decision }
    }

    pub fn best_match(
        &self,
        templates: &[FaceTemplate],
        candidate: &FaceEmbedding,
    ) -> Option<FaceTemplateMatch> {
        templates
            .iter()
            .map(|template| {
                let face_match = self.compare_embeddings(&template.embedding, candidate);
                FaceTemplateMatch {
                    template_ref: template.template_ref.clone(),
                    user_id: template.user_id.clone(),
                    pose_group: template.pose_group,
                    score: face_match.score,
                    decision: face_match.decision,
                }
            })
            .max_by(|left, right| left.score.total_cmp(&right.score))
    }

    pub fn best_compatible_match(
        &self,
        templates: &[FaceTemplate],
        recognition_model: &FaceModelDescriptor,
        candidate: &FaceEmbedding,
    ) -> Option<FaceTemplateMatch> {
        let compatible_templates = templates
            .iter()
            .filter(|template| template.is_compatible_with(recognition_model));

        compatible_templates
            .map(|template| {
                let face_match = self.compare_embeddings(&template.embedding, candidate);
                FaceTemplateMatch {
                    template_ref: template.template_ref.clone(),
                    user_id: template.user_id.clone(),
                    pose_group: template.pose_group,
                    score: face_match.score,
                    decision: face_match.decision,
                }
            })
            .max_by(|left, right| left.score.total_cmp(&right.score))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_round_trips_as_json_bytes() -> Result<(), FaceTemplateCodecError> {
        let template = FaceTemplate {
            template_ref: FaceTemplateRef("face-1".to_owned()),
            user_id: "user-1".to_owned(),
            model_family: "sface".to_owned(),
            model_version: "2021dec".to_owned(),
            pose_group: FacePoseGroup::FrontalPrimary,
            selected_for_unlock: true,
            quality_score: Some(0.95),
            embedding: FaceEmbedding {
                values: vec![1.0, 0.0, 1.0],
            },
        };

        let encoded = template.to_json_bytes()?;
        let decoded = FaceTemplate::from_json_bytes(&encoded)?;

        assert_eq!(decoded, template);
        Ok(())
    }

    #[test]
    fn best_match_uses_explicit_threshold_decision() -> Result<(), &'static str> {
        let matcher = FaceTemplateMatcher::new(0.82);
        let templates = vec![FaceTemplate {
            template_ref: FaceTemplateRef("face-1".to_owned()),
            user_id: "user-1".to_owned(),
            model_family: "sface".to_owned(),
            model_version: "2021dec".to_owned(),
            pose_group: FacePoseGroup::FrontalPrimary,
            selected_for_unlock: true,
            quality_score: None,
            embedding: FaceEmbedding {
                values: vec![1.0, 0.0, 1.0],
            },
        }];

        let matched = matcher.best_match(
            &templates,
            &FaceEmbedding {
                values: vec![1.0, 0.0, 1.0],
            },
        );
        let matched = matched.ok_or("test template should match")?;

        assert_eq!(matched.decision, FaceMatchDecision::MatchAccepted);
        assert_eq!(matched.template_ref, FaceTemplateRef("face-1".to_owned()));
        Ok(())
    }

    #[test]
    fn compatible_match_ignores_template_from_another_recognition_model() {
        let matcher = FaceTemplateMatcher::new(0.82);
        let templates = vec![FaceTemplate {
            template_ref: FaceTemplateRef("face-1".to_owned()),
            user_id: "user-1".to_owned(),
            model_family: "other-recognizer".to_owned(),
            model_version: "v2".to_owned(),
            pose_group: FacePoseGroup::FrontalPrimary,
            selected_for_unlock: true,
            quality_score: None,
            embedding: FaceEmbedding {
                values: vec![1.0, 0.0, 1.0],
            },
        }];

        let matched = matcher.best_compatible_match(
            &templates,
            &FaceModelDescriptor {
                model_family: "sface".to_owned(),
                model_version: "2021dec".to_owned(),
            },
            &FaceEmbedding {
                values: vec![1.0, 0.0, 1.0],
            },
        );

        assert_eq!(matched, None);
    }
}
