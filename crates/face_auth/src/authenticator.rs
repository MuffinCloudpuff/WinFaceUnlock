use common_protocol::{AuthFailureReason, UserId};
use face_engine::{
    DetectedFace, FaceEngineError, FaceMatchDecision, FaceModelDescriptor, FaceModelProvider,
    FaceTemplate, FaceTemplateMatch, FaceTemplateMatcher,
};
use video_provider::VideoFrame;

use crate::{AttemptPolicy, AttemptPolicyDecision};

#[derive(Clone, Debug, PartialEq)]
pub struct RecognitionTemplates {
    templates: Vec<FaceTemplate>,
}

impl RecognitionTemplates {
    pub fn new(templates: Vec<FaceTemplate>) -> Self {
        Self { templates }
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    pub fn as_slice(&self) -> &[FaceTemplate] {
        &self.templates
    }

    pub fn has_compatible_template(&self, recognition_model: &FaceModelDescriptor) -> bool {
        self.templates
            .iter()
            .any(|template| template.is_compatible_with(recognition_model))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AuthenticationOutcome {
    pub matched_user_id: UserId,
    pub match_score: f32,
    pub matched_template: FaceTemplateMatch,
    pub matched_pose_group: face_engine::FacePoseGroup,
}

pub struct FaceAuthenticator<M> {
    model_provider: M,
    matcher: FaceTemplateMatcher,
    attempt_policy: AttemptPolicy,
}

impl<M> FaceAuthenticator<M>
where
    M: FaceModelProvider,
{
    pub fn new(
        model_provider: M,
        matcher: FaceTemplateMatcher,
        attempt_policy: AttemptPolicy,
    ) -> Self {
        Self {
            model_provider,
            matcher,
            attempt_policy,
        }
    }

    pub fn load_models(&mut self) -> Result<(), FaceEngineError> {
        self.model_provider.load_models()
    }

    pub fn unload_models(&mut self) {
        self.model_provider.unload_models();
    }

    pub fn authenticate_frame(
        &mut self,
        frame: &VideoFrame,
        templates: &RecognitionTemplates,
        current_time_unix_ms: i64,
    ) -> Result<AuthenticationOutcome, AuthFailureReason> {
        self.validate_authentication_preconditions(templates, current_time_unix_ms)?;
        let detected_face = self.detect_single_face(frame)?;
        self.authenticate_detected_face_after_preconditions(
            frame,
            &detected_face,
            templates,
            current_time_unix_ms,
        )
    }

    pub fn detect_single_face(
        &mut self,
        frame: &VideoFrame,
    ) -> Result<DetectedFace, AuthFailureReason> {
        let faces = self
            .model_provider
            .detect(frame)
            .map_err(face_engine_error_to_auth_failure)?;
        if faces.is_empty() {
            return Err(AuthFailureReason::NoFaceDetected);
        }
        if faces.len() > 1 {
            return Err(AuthFailureReason::MultipleFacesDetected);
        }

        Ok(faces[0].clone())
    }

    pub fn authenticate_detected_face(
        &mut self,
        frame: &VideoFrame,
        detected_face: &DetectedFace,
        templates: &RecognitionTemplates,
        current_time_unix_ms: i64,
    ) -> Result<AuthenticationOutcome, AuthFailureReason> {
        self.validate_authentication_preconditions(templates, current_time_unix_ms)?;
        self.authenticate_detected_face_after_preconditions(
            frame,
            detected_face,
            templates,
            current_time_unix_ms,
        )
    }

    pub fn reset_consecutive_matches(&mut self) {
        self.attempt_policy.reset_consecutive_matches();
    }

    fn validate_authentication_preconditions(
        &mut self,
        templates: &RecognitionTemplates,
        current_time_unix_ms: i64,
    ) -> Result<(), AuthFailureReason> {
        if templates.is_empty() {
            return Err(AuthFailureReason::InternalError);
        }
        if self.attempt_policy.cooldown_is_active(current_time_unix_ms) {
            return Err(AuthFailureReason::CooldownActive);
        }
        let recognition_model = self.model_provider.recognition_model().clone();
        if !templates.has_compatible_template(&recognition_model) {
            return Err(AuthFailureReason::TemplateModelMismatch);
        }
        Ok(())
    }

    fn authenticate_detected_face_after_preconditions(
        &mut self,
        frame: &VideoFrame,
        detected_face: &DetectedFace,
        templates: &RecognitionTemplates,
        current_time_unix_ms: i64,
    ) -> Result<AuthenticationOutcome, AuthFailureReason> {
        let recognition_model = self.model_provider.recognition_model().clone();
        let candidate = self
            .model_provider
            .extract(frame, detected_face)
            .map_err(face_engine_error_to_auth_failure)?;
        let Some(best_match) = self.matcher.best_compatible_match(
            templates.as_slice(),
            &recognition_model,
            &candidate,
        ) else {
            self.attempt_policy
                .record_failed_attempt(current_time_unix_ms);
            return Err(AuthFailureReason::MatchBelowThreshold);
        };

        let policy_decision = match best_match.decision {
            FaceMatchDecision::MatchAccepted => self
                .attempt_policy
                .record_match_result(true, current_time_unix_ms),
            FaceMatchDecision::MatchRejectedBelowThreshold => self
                .attempt_policy
                .record_match_result(false, current_time_unix_ms),
        };

        match policy_decision {
            AttemptPolicyDecision::AuthenticationAccepted => Ok(AuthenticationOutcome {
                matched_user_id: UserId(best_match.user_id.clone()),
                match_score: best_match.score,
                matched_pose_group: best_match.pose_group,
                matched_template: best_match,
            }),
            AttemptPolicyDecision::NeedMoreConsecutiveMatches => {
                Err(AuthFailureReason::MatchBelowThreshold)
            }
            AttemptPolicyDecision::MatchRejectedBelowThreshold => {
                Err(AuthFailureReason::MatchBelowThreshold)
            }
            AttemptPolicyDecision::CooldownActivated => Err(AuthFailureReason::CooldownActive),
        }
    }
}

fn face_engine_error_to_auth_failure(error: FaceEngineError) -> AuthFailureReason {
    match error {
        FaceEngineError::NoFaceDetected => AuthFailureReason::NoFaceDetected,
        FaceEngineError::MultipleFacesDetected => AuthFailureReason::MultipleFacesDetected,
        FaceEngineError::ModelNotLoaded
        | FaceEngineError::ModelLoadFailed
        | FaceEngineError::ModelPathMissing
        | FaceEngineError::InvalidFrame
        | FaceEngineError::InvalidEmbedding
        | FaceEngineError::InferenceFailed => AuthFailureReason::InternalError,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use face_engine::{
        DetectedFace, FaceEmbedding, FaceMatch, FaceMatchDecision, FaceModelDescriptor,
        FaceTemplateRef,
    };
    use video_provider::{PixelFormat, VideoFrame};

    use super::*;
    use crate::AttemptPolicyConfig;

    struct StubModelProvider {
        recognition_model: FaceModelDescriptor,
        load_count: Rc<Cell<u32>>,
        unload_count: Rc<Cell<u32>>,
    }

    impl FaceModelProvider for StubModelProvider {
        fn load_models(&mut self) -> Result<(), FaceEngineError> {
            self.load_count.set(self.load_count.get().saturating_add(1));
            Ok(())
        }

        fn unload_models(&mut self) {
            self.unload_count
                .set(self.unload_count.get().saturating_add(1));
        }

        fn recognition_model(&self) -> &FaceModelDescriptor {
            &self.recognition_model
        }

        fn detect(&mut self, _frame: &VideoFrame) -> Result<Vec<DetectedFace>, FaceEngineError> {
            Err(FaceEngineError::InferenceFailed)
        }

        fn extract(
            &mut self,
            _frame: &VideoFrame,
            _face: &DetectedFace,
        ) -> Result<FaceEmbedding, FaceEngineError> {
            Err(FaceEngineError::InferenceFailed)
        }

        fn compare(&self, _enrolled: &FaceEmbedding, _candidate: &FaceEmbedding) -> FaceMatch {
            FaceMatch {
                score: 0.0,
                decision: FaceMatchDecision::MatchRejectedBelowThreshold,
            }
        }
    }

    #[test]
    fn rejects_template_from_another_recognition_model_before_inference() {
        let (provider, _, _) = stub_model_provider(FaceModelDescriptor {
            model_family: "sface".to_owned(),
            model_version: "2021dec".to_owned(),
        });
        let templates = RecognitionTemplates::new(vec![FaceTemplate {
            template_ref: FaceTemplateRef("face-1".to_owned()),
            user_id: "user-1".to_owned(),
            model_family: "other-recognizer".to_owned(),
            model_version: "v2".to_owned(),
            pose_group: face_engine::FacePoseGroup::FrontalPrimary,
            selected_for_unlock: true,
            quality_score: None,
            embedding: FaceEmbedding { values: vec![1.0] },
        }]);
        let matcher = FaceTemplateMatcher::new(0.55);
        let policy = AttemptPolicy::new(AttemptPolicyConfig::default());
        let mut authenticator = FaceAuthenticator::new(provider, matcher, policy);
        let frame = VideoFrame {
            width: 1,
            height: 1,
            format: PixelFormat::Gray8,
            data: vec![0],
        };

        let result = authenticator.authenticate_frame(&frame, &templates, 1_000);

        assert_eq!(result, Err(AuthFailureReason::TemplateModelMismatch));
    }

    #[test]
    fn load_and_unload_delegate_to_model_provider() -> Result<(), FaceEngineError> {
        let (provider, load_count, unload_count) = stub_model_provider(FaceModelDescriptor {
            model_family: "sface".to_owned(),
            model_version: "2021dec".to_owned(),
        });
        let matcher = FaceTemplateMatcher::new(0.55);
        let policy = AttemptPolicy::new(AttemptPolicyConfig::default());
        let mut authenticator = FaceAuthenticator::new(provider, matcher, policy);

        authenticator.load_models()?;
        authenticator.unload_models();

        assert_eq!(load_count.get(), 1);
        assert_eq!(unload_count.get(), 1);

        Ok(())
    }

    fn stub_model_provider(
        recognition_model: FaceModelDescriptor,
    ) -> (StubModelProvider, Rc<Cell<u32>>, Rc<Cell<u32>>) {
        let load_count = Rc::new(Cell::new(0));
        let unload_count = Rc::new(Cell::new(0));
        (
            StubModelProvider {
                recognition_model,
                load_count: Rc::clone(&load_count),
                unload_count: Rc::clone(&unload_count),
            },
            load_count,
            unload_count,
        )
    }
}
