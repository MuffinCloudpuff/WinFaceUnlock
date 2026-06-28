use std::collections::HashMap;

use common_protocol::UserId;
use face_engine::{
    FaceEngineError, FaceModelProvider, FacePoseGroup, FaceSampleRejectReason, FaceTemplate,
    FaceTemplateRef, FaceTemplateSampleMetadata, FaceTemplateSet, FaceTemplateSetQualitySummary,
    FaceTemplateThresholdProfile,
};
use face_pose::{FacePoseCapabilities, FacePoseEstimate, FacePoseProvider};
use video_provider::VideoFrame;

use crate::{
    GuidedEnrollmentStep,
    quality::{FaceQualityPolicy, reject_reason_for_quality, score_face_sample},
};

const MIN_FRONTAL_POSE_BASELINE_SAMPLES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuidedEnrollmentConfig {
    pub frames_per_step: u32,
    pub duplicate_similarity_threshold: f32,
    pub template_acceptance_threshold: f32,
    pub frame_match_threshold: f32,
    pub required_consecutive_match_count: u32,
    pub quality_policy: FaceQualityPolicy,
}

impl Default for GuidedEnrollmentConfig {
    fn default() -> Self {
        Self {
            frames_per_step: 30,
            duplicate_similarity_threshold: 0.98,
            template_acceptance_threshold: 0.45,
            frame_match_threshold: 0.75,
            required_consecutive_match_count: 3,
            quality_policy: FaceQualityPolicy::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct GuidedEnrollmentReport {
    pub enrollment_id: String,
    pub user_id: String,
    pub selected_template_count: usize,
    pub rejected_sample_count: usize,
    pub pose_group_counts: Vec<PoseGroupCount>,
    pub reject_reason_counts: Vec<RejectReasonCount>,
    pub quality_summary: FaceTemplateSetQualitySummary,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PoseGroupCount {
    pub pose_group: FacePoseGroup,
    pub selected_template_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct RejectReasonCount {
    pub reason: FaceSampleRejectReason,
    pub rejected_sample_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct CandidateSample {
    template: FaceTemplate,
    metadata: FaceTemplateSampleMetadata,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuidedFrameObservation {
    pub step: GuidedEnrollmentStep,
    pub frame_index: u32,
    pub accepted_for_step: bool,
    pub reject_reason: Option<FaceSampleRejectReason>,
    pub quality_score: f32,
    pub pose_fit_score: f32,
    pub pose_estimate: Option<FacePoseEstimate>,
    pub detected_faces: Vec<face_engine::DetectedFace>,
}

pub struct GuidedFaceEnrollmentService<M, P> {
    model_provider: M,
    pose_provider: P,
    config: GuidedEnrollmentConfig,
    candidates_by_group: HashMap<FacePoseGroup, Vec<CandidateSample>>,
    rejected_metadata: Vec<FaceTemplateSampleMetadata>,
    primary_embedding: Option<face_engine::FaceEmbedding>,
    frontal_yaw_baseline_deg: Option<f32>,
    frontal_pitch_baseline_deg: Option<f32>,
    frontal_yaw_samples: Vec<f32>,
    frontal_pitch_samples: Vec<f32>,
}

impl<M, P> GuidedFaceEnrollmentService<M, P>
where
    M: FaceModelProvider,
    P: FacePoseProvider,
{
    pub fn new(model_provider: M, pose_provider: P, config: GuidedEnrollmentConfig) -> Self {
        Self {
            model_provider,
            pose_provider,
            config,
            candidates_by_group: HashMap::new(),
            rejected_metadata: Vec::new(),
            primary_embedding: None,
            frontal_yaw_baseline_deg: None,
            frontal_pitch_baseline_deg: None,
            frontal_yaw_samples: Vec::new(),
            frontal_pitch_samples: Vec::new(),
        }
    }

    pub fn model_provider_mut(&mut self) -> &mut M {
        &mut self.model_provider
    }

    pub fn pose_provider_name(&self) -> &'static str {
        self.pose_provider.provider_name()
    }

    pub fn pose_provider_capabilities(&self) -> FacePoseCapabilities {
        self.pose_provider.capabilities()
    }

    pub fn observe_frame(
        &mut self,
        frame: &VideoFrame,
        user_id: &UserId,
        step: GuidedEnrollmentStep,
        frame_index: u32,
        frame_timestamp_ms: i64,
    ) -> Result<GuidedFrameObservation, FaceEngineError> {
        let faces = self.model_provider.detect(frame)?;
        if faces.len() != 1 {
            let reject_reason = if faces.is_empty() {
                FaceSampleRejectReason::NoFaceDetected
            } else {
                FaceSampleRejectReason::MultipleFacesDetected
            };
            self.rejected_metadata.push(rejected_metadata(
                user_id,
                step,
                frame_index,
                frame_timestamp_ms,
                reject_reason,
            ));
            return Ok(GuidedFrameObservation {
                step,
                frame_index,
                accepted_for_step: false,
                reject_reason: Some(reject_reason),
                quality_score: 0.0,
                pose_fit_score: 0.0,
                pose_estimate: None,
                detected_faces: faces,
            });
        }

        let face = &faces[0];
        let embedding = self.model_provider.extract(frame, face)?;
        let raw_pose_estimate = self.pose_provider.estimate_pose(frame, face).ok();
        let pose_estimate = self.calibrated_pose_estimate(step, raw_pose_estimate);
        let embedding_consistency_score = self.primary_embedding.as_ref().map(|primary| {
            self.model_provider
                .compare(primary, &embedding)
                .score
                .clamp(0.0, 1.0)
        });
        let quality = score_face_sample(
            frame,
            face,
            step,
            pose_estimate,
            embedding_consistency_score,
        );
        let mut reject_reason = reject_reason_for_quality(&quality, &self.config.quality_policy);

        if step != GuidedEnrollmentStep::FrontalPrimary
            && matches!(
                embedding_consistency_score,
                Some(score) if score < self.config.template_acceptance_threshold
            )
        {
            reject_reason = Some(FaceSampleRejectReason::EmbeddingInconsistentWithPrimary);
        }

        let pose =
            raw_pose_estimate.unwrap_or_else(|| FacePoseEstimate::without_blink(0.0, 0.0, 0.0));
        let metadata = FaceTemplateSampleMetadata {
            sample_id: format!("{}-{}-{frame_index}", step.label(), frame_timestamp_ms),
            pose_group: step.pose_group(),
            source_step: step.label().to_owned(),
            frame_index,
            frame_timestamp_ms,
            face_box: face.bounds.clone(),
            landmarks: face.landmarks.clone(),
            detection_confidence: face.confidence,
            pose_yaw_deg: pose.yaw_deg,
            pose_pitch_deg: pose.pitch_deg,
            pose_roll_deg: pose.roll_deg,
            quality: quality.clone(),
            selected_for_unlock: reject_reason.is_none()
                && step.pose_group().selected_for_unlock_by_default(),
            reject_reason,
        };

        if let Some(reason) = metadata.reject_reason {
            let mut rejected = metadata;
            rejected.reject_reason = Some(reason);
            self.rejected_metadata.push(rejected);
            return Ok(GuidedFrameObservation {
                step,
                frame_index,
                accepted_for_step: false,
                reject_reason: Some(reason),
                quality_score: quality.quality_score,
                pose_fit_score: quality.pose_fit_score,
                pose_estimate,
                detected_faces: faces,
            });
        }
        self.record_frontal_pose_baseline(step, raw_pose_estimate);

        let recognition_model = self.model_provider.recognition_model();
        let template = FaceTemplate {
            template_ref: FaceTemplateRef(format!("face-template-{}", metadata.sample_id)),
            user_id: user_id.0.clone(),
            model_family: recognition_model.model_family.clone(),
            model_version: recognition_model.model_version.clone(),
            pose_group: metadata.pose_group,
            selected_for_unlock: metadata.selected_for_unlock,
            quality_score: Some(metadata.quality.quality_score),
            embedding,
        };

        if step == GuidedEnrollmentStep::FrontalPrimary && self.primary_embedding.is_none() {
            self.primary_embedding = Some(template.embedding.clone());
        }
        self.candidates_by_group
            .entry(metadata.pose_group)
            .or_default()
            .push(CandidateSample { template, metadata });

        Ok(GuidedFrameObservation {
            step,
            frame_index,
            accepted_for_step: true,
            reject_reason: None,
            quality_score: quality.quality_score,
            pose_fit_score: quality.pose_fit_score,
            pose_estimate,
            detected_faces: faces,
        })
    }

    pub fn preview_frame_for_step(
        &mut self,
        frame: &VideoFrame,
        step: GuidedEnrollmentStep,
        frame_index: u32,
    ) -> Result<GuidedFrameObservation, FaceEngineError> {
        let faces = self.model_provider.detect(frame)?;
        if faces.len() != 1 {
            let reject_reason = if faces.is_empty() {
                FaceSampleRejectReason::NoFaceDetected
            } else {
                FaceSampleRejectReason::MultipleFacesDetected
            };
            return Ok(GuidedFrameObservation {
                step,
                frame_index,
                accepted_for_step: false,
                reject_reason: Some(reject_reason),
                quality_score: 0.0,
                pose_fit_score: 0.0,
                pose_estimate: None,
                detected_faces: faces,
            });
        }

        let face = &faces[0];
        let raw_pose_estimate = self.pose_provider.estimate_pose(frame, face).ok();
        let pose_estimate = self.calibrated_pose_estimate(step, raw_pose_estimate);
        let embedding_consistency_score =
            if step != GuidedEnrollmentStep::FrontalPrimary && self.primary_embedding.is_some() {
                let embedding = self.model_provider.extract(frame, face)?;
                self.primary_embedding.as_ref().map(|primary| {
                    self.model_provider
                        .compare(primary, &embedding)
                        .score
                        .clamp(0.0, 1.0)
                })
            } else {
                None
            };
        let quality = score_face_sample(
            frame,
            face,
            step,
            pose_estimate,
            embedding_consistency_score,
        );
        let mut reject_reason = reject_reason_for_quality(&quality, &self.config.quality_policy);

        if step != GuidedEnrollmentStep::FrontalPrimary
            && matches!(
                embedding_consistency_score,
                Some(score) if score < self.config.template_acceptance_threshold
            )
        {
            reject_reason = Some(FaceSampleRejectReason::EmbeddingInconsistentWithPrimary);
        }

        Ok(GuidedFrameObservation {
            step,
            frame_index,
            accepted_for_step: reject_reason.is_none(),
            reject_reason,
            quality_score: quality.quality_score,
            pose_fit_score: quality.pose_fit_score,
            pose_estimate,
            detected_faces: faces,
        })
    }

    fn calibrated_pose_estimate(
        &self,
        _step: GuidedEnrollmentStep,
        pose_estimate: Option<FacePoseEstimate>,
    ) -> Option<FacePoseEstimate> {
        let mut pose = pose_estimate?;
        if let Some(baseline) = self.frontal_yaw_baseline_deg {
            pose.yaw_deg -= baseline;
        }
        if let Some(baseline) = self.frontal_pitch_baseline_deg {
            pose.pitch_deg -= baseline;
        }
        Some(pose)
    }

    fn record_frontal_pose_baseline(
        &mut self,
        step: GuidedEnrollmentStep,
        pose_estimate: Option<FacePoseEstimate>,
    ) {
        if step != GuidedEnrollmentStep::FrontalPrimary || self.frontal_yaw_baseline_deg.is_some() {
            return;
        }
        let Some(pose) = pose_estimate else {
            return;
        };
        self.frontal_yaw_samples.push(pose.yaw_deg);
        self.frontal_pitch_samples.push(pose.pitch_deg);
        if self.frontal_yaw_samples.len() >= MIN_FRONTAL_POSE_BASELINE_SAMPLES {
            // ponytail: session median handles camera angle; persist per-camera only if needed.
            self.frontal_yaw_baseline_deg = Some(median(&self.frontal_yaw_samples));
            self.frontal_pitch_baseline_deg = Some(median(&self.frontal_pitch_samples));
        }
    }

    pub fn finish(
        mut self,
        user_id: UserId,
        enrollment_id: String,
        detector_model_family: String,
        detector_model_version: String,
        enrollment_created_at_unix_ms: i64,
    ) -> FaceTemplateSet {
        let mut selected_templates = Vec::new();
        let mut selected_metadata = Vec::new();
        let mut rejected_metadata = self.rejected_metadata;

        for pose_group in [
            FacePoseGroup::FrontalPrimary,
            FacePoseGroup::FrontalVariant,
            FacePoseGroup::YawLeftMild,
            FacePoseGroup::YawRightMild,
            FacePoseGroup::PitchDownMild,
            FacePoseGroup::PitchUpMild,
        ] {
            let Some(mut candidates) = self.candidates_by_group.remove(&pose_group) else {
                continue;
            };
            candidates.sort_by(|left, right| {
                right
                    .metadata
                    .quality
                    .quality_score
                    .total_cmp(&left.metadata.quality.quality_score)
            });
            let target_count = target_template_count(pose_group);
            for candidate in candidates {
                if selected_templates_for_group(&selected_templates, pose_group) >= target_count {
                    let mut rejected = candidate.metadata;
                    rejected.selected_for_unlock = false;
                    rejected.reject_reason = Some(FaceSampleRejectReason::DuplicateTooSimilar);
                    rejected_metadata.push(rejected);
                    continue;
                }
                if is_duplicate(
                    &selected_templates,
                    pose_group,
                    &candidate.template,
                    self.config.duplicate_similarity_threshold,
                ) {
                    let mut rejected = candidate.metadata;
                    rejected.selected_for_unlock = false;
                    rejected.reject_reason = Some(FaceSampleRejectReason::DuplicateTooSimilar);
                    rejected_metadata.push(rejected);
                    continue;
                }

                selected_metadata.push(candidate.metadata);
                selected_templates.push(candidate.template);
            }
        }
        for (_, candidates) in self.candidates_by_group {
            for mut candidate in candidates {
                candidate.metadata.selected_for_unlock = false;
                selected_metadata.push(candidate.metadata);
            }
        }

        let recognition_model = self.model_provider.recognition_model().clone();
        let quality_summary = quality_summary(&selected_metadata, rejected_metadata.len());
        let mut sample_metadata = selected_metadata;
        sample_metadata.extend(rejected_metadata);

        FaceTemplateSet {
            user_id: user_id.0,
            detector_model_family,
            detector_model_version,
            recognizer_model_family: recognition_model.model_family,
            recognizer_model_version: recognition_model.model_version,
            enrollment_id,
            enrollment_created_at_unix_ms,
            threshold_profile: FaceTemplateThresholdProfile {
                template_acceptance_threshold: self.config.template_acceptance_threshold,
                frame_match_threshold: self.config.frame_match_threshold,
                required_consecutive_match_count: self.config.required_consecutive_match_count,
                min_quality_score: self.config.quality_policy.min_quality_score,
            },
            templates: selected_templates,
            sample_metadata,
            quality_summary,
        }
    }
}

pub fn build_guided_enrollment_report(template_set: &FaceTemplateSet) -> GuidedEnrollmentReport {
    let mut pose_counts = HashMap::<FacePoseGroup, usize>::new();
    for template in &template_set.templates {
        if template.selected_for_unlock {
            *pose_counts.entry(template.pose_group).or_default() += 1;
        }
    }
    let mut reject_counts = HashMap::<FaceSampleRejectReason, usize>::new();
    for metadata in &template_set.sample_metadata {
        if let Some(reason) = metadata.reject_reason {
            *reject_counts.entry(reason).or_default() += 1;
        }
    }

    let mut pose_group_counts = pose_counts
        .into_iter()
        .map(|(pose_group, selected_template_count)| PoseGroupCount {
            pose_group,
            selected_template_count,
        })
        .collect::<Vec<_>>();
    pose_group_counts.sort_by_key(|count| pose_group_sort_key(count.pose_group));

    let mut reject_reason_counts = reject_counts
        .into_iter()
        .map(|(reason, rejected_sample_count)| RejectReasonCount {
            reason,
            rejected_sample_count,
        })
        .collect::<Vec<_>>();
    reject_reason_counts.sort_by_key(|count| format!("{:?}", count.reason));

    GuidedEnrollmentReport {
        enrollment_id: template_set.enrollment_id.clone(),
        user_id: template_set.user_id.clone(),
        selected_template_count: template_set.templates.len(),
        rejected_sample_count: template_set
            .sample_metadata
            .iter()
            .filter(|metadata| metadata.reject_reason.is_some())
            .count(),
        pose_group_counts,
        reject_reason_counts,
        quality_summary: template_set.quality_summary.clone(),
    }
}

fn rejected_metadata(
    user_id: &UserId,
    step: GuidedEnrollmentStep,
    frame_index: u32,
    frame_timestamp_ms: i64,
    reject_reason: FaceSampleRejectReason,
) -> FaceTemplateSampleMetadata {
    FaceTemplateSampleMetadata {
        sample_id: format!("{}-{}-{frame_index}", step.label(), frame_timestamp_ms),
        pose_group: step.pose_group(),
        source_step: format!("{}:{}", step.label(), user_id.0),
        frame_index,
        frame_timestamp_ms,
        face_box: face_engine::FaceBox {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        },
        landmarks: Vec::new(),
        detection_confidence: 0.0,
        pose_yaw_deg: 0.0,
        pose_pitch_deg: 0.0,
        pose_roll_deg: 0.0,
        quality: face_engine::FaceTemplateQualityScores {
            quality_score: 0.0,
            blur_score: 0.0,
            illumination_score: 0.0,
            face_size_score: 0.0,
            alignment_score: 0.0,
            pose_fit_score: 0.0,
            embedding_consistency_score: None,
        },
        selected_for_unlock: false,
        reject_reason: Some(reject_reason),
    }
}

fn selected_templates_for_group(templates: &[FaceTemplate], pose_group: FacePoseGroup) -> usize {
    templates
        .iter()
        .filter(|template| template.pose_group == pose_group)
        .count()
}

fn target_template_count(pose_group: FacePoseGroup) -> usize {
    match pose_group {
        FacePoseGroup::FrontalPrimary => 2,
        FacePoseGroup::FrontalVariant => 2,
        FacePoseGroup::YawLeftMild | FacePoseGroup::YawRightMild => 2,
        FacePoseGroup::PitchDownMild | FacePoseGroup::PitchUpMild => 1,
        FacePoseGroup::BlinkMotion
        | FacePoseGroup::HardPoseDiagnostic
        | FacePoseGroup::RejectedQuality => 0,
    }
}

fn is_duplicate(
    selected_templates: &[FaceTemplate],
    pose_group: FacePoseGroup,
    candidate: &FaceTemplate,
    duplicate_similarity_threshold: f32,
) -> bool {
    selected_templates
        .iter()
        .filter(|template| template.pose_group == pose_group)
        .any(|template| {
            face_engine::cosine_similarity(&template.embedding.values, &candidate.embedding.values)
                .map(|score| score >= duplicate_similarity_threshold)
                .unwrap_or(false)
        })
}

fn quality_summary(
    selected_metadata: &[FaceTemplateSampleMetadata],
    rejected_sample_count: usize,
) -> FaceTemplateSetQualitySummary {
    let selected_template_count = selected_metadata.len();
    let mut quality_scores = selected_metadata
        .iter()
        .map(|metadata| metadata.quality.quality_score)
        .collect::<Vec<_>>();
    quality_scores.sort_by(|left, right| left.total_cmp(right));

    let average_selected_quality_score = if quality_scores.is_empty() {
        None
    } else {
        Some(quality_scores.iter().sum::<f32>() / quality_scores.len() as f32)
    };
    let minimum_selected_quality_score = quality_scores.first().copied();

    FaceTemplateSetQualitySummary {
        selected_template_count,
        rejected_sample_count,
        average_selected_quality_score,
        minimum_selected_quality_score,
    }
}

fn pose_group_sort_key(pose_group: FacePoseGroup) -> u8 {
    match pose_group {
        FacePoseGroup::FrontalPrimary => 0,
        FacePoseGroup::FrontalVariant => 1,
        FacePoseGroup::YawLeftMild => 2,
        FacePoseGroup::YawRightMild => 3,
        FacePoseGroup::PitchDownMild => 4,
        FacePoseGroup::PitchUpMild => 5,
        FacePoseGroup::BlinkMotion => 6,
        FacePoseGroup::HardPoseDiagnostic => 7,
        FacePoseGroup::RejectedQuality => 8,
    }
}

fn median(values: &[f32]) -> f32 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[sorted.len() / 2]
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use face_engine::{
        DetectedFace, FaceBox, FaceEmbedding, FaceEngineError, FaceMatch, FaceMatchDecision,
        FaceModelDescriptor, FaceModelProvider, FacePoseGroup, FaceTemplate, FaceTemplateRef,
    };
    use face_pose::{FacePoseCapabilities, FacePoseError, FacePoseProvider};
    use video_provider::{PixelFormat, VideoFrame};

    use super::*;

    #[test]
    fn duplicate_check_only_compares_templates_in_same_pose_group() {
        let selected_templates = vec![FaceTemplate {
            template_ref: FaceTemplateRef("t1".to_owned()),
            user_id: "u1".to_owned(),
            model_family: "sface".to_owned(),
            model_version: "2021dec".to_owned(),
            pose_group: FacePoseGroup::YawLeftMild,
            selected_for_unlock: true,
            quality_score: Some(0.9),
            embedding: FaceEmbedding {
                values: vec![1.0, 0.0],
            },
        }];
        let candidate = FaceTemplate {
            template_ref: FaceTemplateRef("t2".to_owned()),
            user_id: "u1".to_owned(),
            model_family: "sface".to_owned(),
            model_version: "2021dec".to_owned(),
            pose_group: FacePoseGroup::YawRightMild,
            selected_for_unlock: true,
            quality_score: Some(0.9),
            embedding: FaceEmbedding {
                values: vec![1.0, 0.0],
            },
        };

        assert!(!is_duplicate(
            &selected_templates,
            FacePoseGroup::YawRightMild,
            &candidate,
            0.98
        ));
    }

    #[test]
    fn guided_steps_use_frontal_pose_as_session_baseline() -> Result<(), FaceEngineError> {
        let mut enrollment = GuidedFaceEnrollmentService::new(
            StubModelProvider,
            SequencePoseProvider {
                poses: VecDeque::from([
                    FacePoseEstimate::without_blink(1.0, 14.0, 0.0),
                    FacePoseEstimate::without_blink(2.0, 15.0, 0.0),
                    FacePoseEstimate::without_blink(3.0, 16.0, 0.0),
                    FacePoseEstimate::without_blink(20.0, 15.0, 0.0),
                    FacePoseEstimate::without_blink(2.0, 0.0, 0.0),
                ]),
            },
            GuidedEnrollmentConfig::default(),
        );
        let frame = textured_frame();

        for frame_index in 0..3 {
            let frontal = enrollment.observe_frame(
                &frame,
                &UserId("user".to_owned()),
                GuidedEnrollmentStep::FrontalPrimary,
                frame_index,
                i64::from(frame_index),
            )?;
            assert!(frontal.accepted_for_step);
        }

        let yaw_right =
            enrollment.preview_frame_for_step(&frame, GuidedEnrollmentStep::YawRightMild, 1)?;
        assert!(yaw_right.accepted_for_step);
        assert_eq!(yaw_right.pose_estimate.map(|pose| pose.yaw_deg), Some(18.0));

        let pitch_up =
            enrollment.preview_frame_for_step(&frame, GuidedEnrollmentStep::PitchUpMild, 1)?;

        assert!(pitch_up.accepted_for_step);
        assert!(pitch_up.pose_fit_score >= 0.25);
        assert_eq!(
            pitch_up.pose_estimate.map(|pose| pose.pitch_deg),
            Some(-15.0)
        );
        Ok(())
    }

    #[derive(Clone)]
    struct StubModelProvider;

    impl FaceModelProvider for StubModelProvider {
        fn load_models(&mut self) -> Result<(), FaceEngineError> {
            Ok(())
        }

        fn unload_models(&mut self) {}

        fn recognition_model(&self) -> &FaceModelDescriptor {
            static MODEL: std::sync::LazyLock<FaceModelDescriptor> =
                std::sync::LazyLock::new(|| FaceModelDescriptor {
                    model_family: "stub".to_owned(),
                    model_version: "test".to_owned(),
                });
            &MODEL
        }

        fn detect(&mut self, _frame: &VideoFrame) -> Result<Vec<DetectedFace>, FaceEngineError> {
            Ok(vec![DetectedFace {
                bounds: FaceBox {
                    x: 25.0,
                    y: 25.0,
                    width: 50.0,
                    height: 50.0,
                },
                landmarks: vec![
                    face_engine::FaceLandmark { x: 40.0, y: 40.0 },
                    face_engine::FaceLandmark { x: 60.0, y: 40.0 },
                    face_engine::FaceLandmark { x: 50.0, y: 50.0 },
                    face_engine::FaceLandmark { x: 43.0, y: 65.0 },
                    face_engine::FaceLandmark { x: 57.0, y: 65.0 },
                ],
                confidence: 0.99,
            }])
        }

        fn extract(
            &mut self,
            _frame: &VideoFrame,
            _face: &DetectedFace,
        ) -> Result<FaceEmbedding, FaceEngineError> {
            Ok(FaceEmbedding {
                values: vec![1.0, 0.0],
            })
        }

        fn compare(&self, _enrolled: &FaceEmbedding, _candidate: &FaceEmbedding) -> FaceMatch {
            FaceMatch {
                score: 1.0,
                decision: FaceMatchDecision::MatchAccepted,
            }
        }
    }

    struct SequencePoseProvider {
        poses: VecDeque<FacePoseEstimate>,
    }

    impl FacePoseProvider for SequencePoseProvider {
        fn provider_name(&self) -> &'static str {
            "sequence"
        }

        fn capabilities(&self) -> FacePoseCapabilities {
            FacePoseCapabilities::HEAD_POSE_ONLY
        }

        fn estimate_pose(
            &mut self,
            _frame: &VideoFrame,
            _face: &DetectedFace,
        ) -> Result<FacePoseEstimate, FacePoseError> {
            Ok(self
                .poses
                .pop_front()
                .unwrap_or_else(|| FacePoseEstimate::without_blink(0.0, 0.0, 0.0)))
        }
    }

    fn textured_frame() -> VideoFrame {
        let data = (0..10_000)
            .map(|index| if index % 2 == 0 { 80 } else { 180 })
            .collect();
        VideoFrame {
            width: 100,
            height: 100,
            format: PixelFormat::Gray8,
            data,
        }
    }
}
