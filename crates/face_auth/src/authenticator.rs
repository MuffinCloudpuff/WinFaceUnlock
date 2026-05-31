use common_protocol::{AuthFailureReason, UserId};
use face_engine::{
    FaceEngineError, FaceMatchDecision, FaceModelProvider, FaceTemplate, FaceTemplateMatch,
    FaceTemplateMatcher,
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct AuthenticationOutcome {
    pub matched_user_id: UserId,
    pub match_score: f32,
    pub matched_template: FaceTemplateMatch,
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

    pub fn authenticate_frame(
        &mut self,
        frame: &VideoFrame,
        templates: &RecognitionTemplates,
        current_time_unix_ms: i64,
    ) -> Result<AuthenticationOutcome, AuthFailureReason> {
        if templates.is_empty() {
            return Err(AuthFailureReason::InternalError);
        }
        if self.attempt_policy.cooldown_is_active(current_time_unix_ms) {
            return Err(AuthFailureReason::CooldownActive);
        }

        let candidate = self
            .extract_single_face_embedding(frame)
            .map_err(face_engine_error_to_auth_failure)?;
        let Some(best_match) = self.matcher.best_match(templates.as_slice(), &candidate) else {
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

    fn extract_single_face_embedding(
        &mut self,
        frame: &VideoFrame,
    ) -> Result<face_engine::FaceEmbedding, FaceEngineError> {
        let faces = self.model_provider.detect(frame)?;
        if faces.is_empty() {
            return Err(FaceEngineError::NoFaceDetected);
        }
        if faces.len() > 1 {
            return Err(FaceEngineError::MultipleFacesDetected);
        }

        self.model_provider.extract(frame, &faces[0])
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
