use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use common_protocol::{
    AuthFailureReason, AuthGrant, AuthScore, AuthSource, AuthTriggerSource, DEFAULT_GRANT_TTL,
    GrantId, Nonce, ProtocolError, SessionId, UserId,
};
use face_auth::{AttemptPolicy, AttemptPolicyConfig, FaceAuthenticator, RecognitionTemplates};
use face_engine::{
    FaceTemplate, FaceTemplateCodecError, FaceTemplateMatcher, FaceTemplateSet,
    HybridFaceModelConfig, HybridFaceModelProvider,
};
use face_liveness::{
    LivenessDecision, LivenessProviderError, LivenessResult, MiniFasNetLivenessProvider,
};
use ipc::{AuthGrantIssueResult, AuthGrantIssuer};
use video_provider::{
    CameraId, OpenCvCameraProvider, OpenCvCameraProviderConfig, VideoError, VideoFrameProvider,
};

use crate::{
    auth_orchestrator::CameraAuthOrchestrator,
    camera_backend_profiles::apply_profile_to_config,
    camera_frame_recovery::{
        TransientFrameFailureDecision, TransientFrameFailureKind, TransientFrameFailureTolerance,
        validate_frame_for_camera_stream,
    },
    camera_runtime::{CameraLeaseKind, acquire_camera_lease_until},
    service_config::{LocalCameraAuthConfig, ServiceAuthConfig, ServiceAuthMode},
    service_log::{write_service_event, write_service_event_detail},
    simulated_auth::SimulatedAuthGrantIssuer,
};
use std::time::Duration;

pub struct DevelopmentAuthGrantIssuer {
    manual_test_user_id: UserId,
    manual_test_issuer: SimulatedAuthGrantIssuer,
    local_camera_issuer: Option<CameraAuthOrchestrator<LocalCameraAuthGrantIssuer>>,
}

impl DevelopmentAuthGrantIssuer {
    pub fn from_environment(manual_test_user_id: UserId) -> Result<Self, ProtocolError> {
        Self::from_config(manual_test_user_id, ServiceAuthConfig::from_environment()?)
    }

    pub fn from_config(
        manual_test_user_id: UserId,
        config: ServiceAuthConfig,
    ) -> Result<Self, ProtocolError> {
        let local_camera_issuer = local_camera_issuer_from_config(config)?;

        Ok(Self {
            manual_test_issuer: SimulatedAuthGrantIssuer::for_user(manual_test_user_id.clone()),
            manual_test_user_id,
            local_camera_issuer,
        })
    }

    fn reload_from_environment(&mut self) -> Result<(), ProtocolError> {
        let previous_camera_id = self.current_local_camera_id();
        self.local_camera_issuer =
            local_camera_issuer_from_config(ServiceAuthConfig::from_environment()?)?;
        let current_camera_id = self.current_local_camera_id();
        write_service_event_detail(
            "AuthIssuer.Reloaded",
            format!(
                "previous_camera_id={} current_camera_id={} manual_test_user_id={}",
                optional_camera_id_for_log(previous_camera_id.as_deref()),
                optional_camera_id_for_log(current_camera_id.as_deref()),
                self.manual_test_user_id.0
            ),
        );
        Ok(())
    }

    fn current_local_camera_id(&self) -> Option<String> {
        self.local_camera_issuer
            .as_ref()
            .and_then(|issuer| issuer.try_with_issuer(|inner| inner.camera_id().0.clone()))
    }
}

impl AuthGrantIssuer for DevelopmentAuthGrantIssuer {
    fn issue_auth_grant(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        trigger_source: AuthTriggerSource,
        issued_at_unix_ms: i64,
    ) -> AuthGrantIssueResult {
        write_service_event_detail(
            "AuthTrigger.Received",
            format!(
                "session_id={} auth_source={source:?} auth_trigger_source={}",
                session_id.0,
                auth_trigger_source_name(trigger_source)
            ),
        );
        match source {
            AuthSource::ManualTest => self.manual_test_issuer.issue_auth_grant(
                session_id,
                source,
                trigger_source,
                issued_at_unix_ms,
            ),
            AuthSource::LocalCamera => {
                let Some(local_camera_issuer) = self.local_camera_issuer.as_mut() else {
                    return AuthGrantIssueResult::Failed(AuthFailureReason::InternalError);
                };
                local_camera_issuer.issue_auth_grant(
                    session_id,
                    source,
                    trigger_source,
                    issued_at_unix_ms,
                )
            }
            AuthSource::VehicleCamera => {
                AuthGrantIssueResult::Failed(AuthFailureReason::InternalError)
            }
        }
    }

    fn fetch_auth_result(
        &mut self,
        session_id: &SessionId,
        issued_at_unix_ms: i64,
    ) -> Option<Result<AuthGrant, AuthFailureReason>> {
        self.local_camera_issuer
            .as_mut()
            .and_then(|issuer| issuer.fetch_auth_result(session_id, issued_at_unix_ms))
    }

    fn cancel_auth(&mut self, session_id: &SessionId) {
        if let Some(issuer) = self.local_camera_issuer.as_mut() {
            issuer.cancel_auth(session_id);
        }
    }

    fn reload_auth_config(&mut self) -> Result<(), ProtocolError> {
        self.reload_from_environment()
    }
}

fn local_camera_issuer_from_config(
    config: ServiceAuthConfig,
) -> Result<Option<CameraAuthOrchestrator<LocalCameraAuthGrantIssuer>>, ProtocolError> {
    match config.auth_mode {
        ServiceAuthMode::ManualTestOnly => Ok(None),
        ServiceAuthMode::LocalCamera(local_camera_config) => Ok(Some(CameraAuthOrchestrator::new(
            LocalCameraAuthGrantIssuer::from_config(*local_camera_config)?,
        ))),
    }
}

fn optional_camera_id_for_log(camera_id: Option<&str>) -> &str {
    camera_id.unwrap_or("<none>")
}

struct LocalCameraAuthGrantIssuer {
    camera_id: CameraId,
    camera_config: OpenCvCameraProviderConfig,
    max_auth_frames: u32,
    templates: RecognitionTemplates,
    authenticator: FaceAuthenticator<HybridFaceModelProvider>,
    liveness_provider: MiniFasNetLivenessProvider,
    max_spoof_frame_ratio: f32,
    next_grant_sequence: u64,
}

struct LocalCameraAuthenticationOutcome {
    face_authentication: face_auth::AuthenticationOutcome,
    liveness_score: f32,
}

impl LocalCameraAuthGrantIssuer {
    fn from_config(config: LocalCameraAuthConfig) -> Result<Self, ProtocolError> {
        let templates = RecognitionTemplates::new(read_face_templates(&config.face_template_path)?);
        let mut camera_config = config.camera_config;
        apply_profile_to_config(&config.camera_id, &mut camera_config);

        let mut model_config =
            HybridFaceModelConfig::new(config.yunet_model_path, config.sface_model_path);
        model_config.recognizer.match_threshold = config.match_threshold;

        let model_provider = HybridFaceModelProvider::new(model_config);
        let matcher = FaceTemplateMatcher::new(config.match_threshold);
        let attempt_policy = AttemptPolicy::new(AttemptPolicyConfig {
            required_consecutive_match_count: config.required_consecutive_match_count,
            ..AttemptPolicyConfig::default()
        });
        let authenticator = FaceAuthenticator::new(model_provider, matcher, attempt_policy);
        let liveness_provider = MiniFasNetLivenessProvider::new(config.minifasnet_config);

        Ok(Self {
            camera_id: config.camera_id,
            camera_config,
            max_auth_frames: config.max_auth_frames,
            templates,
            authenticator,
            liveness_provider,
            max_spoof_frame_ratio: config.minifasnet_max_spoof_frame_ratio,
            next_grant_sequence: 1,
        })
    }

    fn camera_id(&self) -> &CameraId {
        &self.camera_id
    }

    fn issue_auth_grant_blocking(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        _request_started_at_unix_ms: i64,
    ) -> Result<AuthGrant, AuthFailureReason> {
        self.issue_auth_grant_blocking_with_observer(
            session_id,
            source,
            _request_started_at_unix_ms,
            &mut |_| {},
        )
    }

    fn issue_auth_grant_blocking_with_observer(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        _request_started_at_unix_ms: i64,
        on_grant_issued: &mut dyn FnMut(&AuthGrant),
    ) -> Result<AuthGrant, AuthFailureReason> {
        write_service_event_detail(
            "LocalCameraAuth.Started",
            format!("session_id={} camera_id={}", session_id.0, self.camera_id.0),
        );
        let grant_sequence = self.next_grant_sequence;
        self.next_grant_sequence = self.next_grant_sequence.saturating_add(1);
        let mut issued_grant = None;
        self.authenticate_from_camera(&mut |outcome| {
            let grant = build_auth_grant(session_id, source, grant_sequence, outcome);
            write_service_event_detail(
                "LocalCameraAuth.AuthorizationIssued",
                format!(
                    "session_id={} match_score={} liveness_score={}",
                    session_id.0, outcome.face_authentication.match_score, outcome.liveness_score
                ),
            );
            on_grant_issued(&grant);
            issued_grant = Some(grant);
        })?;

        let Some(grant) = issued_grant else {
            return Err(AuthFailureReason::InternalError);
        };
        write_service_event_detail(
            "LocalCameraAuth.Succeeded",
            format!("session_id={}", session_id.0),
        );

        Ok(grant)
    }

    fn authenticate_from_camera(
        &mut self,
        on_authorization_accepted: &mut dyn FnMut(&LocalCameraAuthenticationOutcome),
    ) -> Result<LocalCameraAuthenticationOutcome, AuthFailureReason> {
        write_service_event("LocalCameraAuth.LoadModelsStarted");
        self.authenticator
            .load_models()
            .map_err(|_| AuthFailureReason::InternalError)?;
        if self.liveness_provider.load_model().is_err() {
            self.authenticator.unload_models();
            write_service_event("LocalCameraAuth.LoadLivenessModelFailed");
            return Err(AuthFailureReason::InternalError);
        }

        let auth_result =
            self.authenticate_from_camera_with_loaded_runtime(on_authorization_accepted);

        self.liveness_provider.unload_model();
        self.authenticator.unload_models();
        write_service_event("LocalCameraAuth.ModelsUnloaded");
        if let Err(reason) = &auth_result {
            write_service_event_detail("LocalCameraAuth.Failed", format!("reason={reason:?}"));
        }

        auth_result
    }

    fn authenticate_from_camera_with_loaded_runtime(
        &mut self,
        on_authorization_accepted: &mut dyn FnMut(&LocalCameraAuthenticationOutcome),
    ) -> Result<LocalCameraAuthenticationOutcome, AuthFailureReason> {
        let _camera_lease = acquire_camera_lease_until(
            CameraLeaseKind::LogonAuthentication,
            Duration::from_secs(2),
        )
        .map_err(|reason| {
            write_service_event_detail(
                "LocalCameraAuth.CameraLeaseDenied",
                format!("reason={reason:?}"),
            );
            AuthFailureReason::InternalError
        })?;
        let mut camera_provider = OpenCvCameraProvider::new(self.camera_config.clone());
        let auth_result = (|| {
            write_service_event_detail(
                "LocalCameraAuth.OpenCameraStarted",
                format!("camera_id={}", self.camera_id.0),
            );
            camera_provider
                .open(&self.camera_id)
                .map_err(video_error_to_auth_failure)?;
            write_service_event("LocalCameraAuth.OpenCameraSucceeded");
            let mut last_rejection = AuthFailureReason::Timeout;
            let mut frame_failure_tolerance =
                TransientFrameFailureTolerance::default_for_camera_stream();
            let mut liveness_window = MiniFasNetWindowEvidence::new(self.max_spoof_frame_ratio);

            for frame_index in 0..self.max_auth_frames {
                let frame = match camera_provider.read_frame() {
                    Ok(frame) => match validate_frame_for_camera_stream(&frame) {
                        Ok(()) => {
                            frame_failure_tolerance.record_valid_frame();
                            frame
                        }
                        Err(kind) => {
                            last_rejection = AuthFailureReason::NoFaceDetected;
                            handle_auth_transient_frame_failure(
                                frame_index,
                                kind,
                                &mut frame_failure_tolerance,
                            )?;
                            continue;
                        }
                    },
                    Err(error) => {
                        let Some(kind) = TransientFrameFailureKind::from_video_error(error.clone())
                        else {
                            return Err(video_error_to_auth_failure(error));
                        };
                        last_rejection = AuthFailureReason::NoFaceDetected;
                        handle_auth_transient_frame_failure(
                            frame_index,
                            kind,
                            &mut frame_failure_tolerance,
                        )?;
                        continue;
                    }
                };
                let detected_face = match self.authenticator.detect_single_face(&frame) {
                    Ok(detected_face) => detected_face,
                    Err(reason) => {
                        self.authenticator.reset_consecutive_matches();
                        last_rejection = reason;
                        write_service_event_detail(
                            "LocalCameraAuth.FrameRejected",
                            format!("frame_index={frame_index} reason={reason:?}"),
                        );
                        continue;
                    }
                };
                let liveness_result = self
                    .liveness_provider
                    .evaluate(&frame, Some(&detected_face))
                    .map_err(liveness_error_to_auth_failure)?;
                match liveness_window.record(&liveness_result)? {
                    Some(liveness_score) => {
                        match self
                            .authenticator
                            .authenticate_detected_face_without_failure_cooldown(
                                &frame,
                                &detected_face,
                                &self.templates,
                                current_time_unix_ms(),
                            ) {
                            Ok(face_authentication) => {
                                liveness_window
                                    .reject_unlock_candidate_if_spoof_ratio_exceeded()?;
                                write_service_event_detail(
                                    "LocalCameraAuth.FrameAccepted",
                                    format!(
                                        "frame_index={frame_index} match_score={} liveness_score={liveness_score}",
                                        face_authentication.match_score
                                    ),
                                );
                                let outcome = LocalCameraAuthenticationOutcome {
                                    face_authentication,
                                    liveness_score,
                                };
                                on_authorization_accepted(&outcome);
                                return Ok(outcome);
                            }
                            Err(reason) => {
                                last_rejection = reason;
                                write_service_event_detail(
                                    "LocalCameraAuth.FrameRejected",
                                    format!("frame_index={frame_index} reason={reason:?}"),
                                );
                            }
                        }
                    }
                    None => {
                        self.authenticator.reset_consecutive_matches();
                        last_rejection = AuthFailureReason::LivenessFailed;
                        write_service_event_detail(
                            "LocalCameraAuth.FrameRejected",
                            format!("frame_index={frame_index} reason=LivenessFailed"),
                        );
                    }
                }
            }

            Err(last_rejection)
        })();
        write_service_event("LocalCameraAuth.CloseCameraStarted");
        camera_provider.close();
        write_service_event("LocalCameraAuth.CloseCameraFinished");
        auth_result
    }
}

impl AuthGrantIssuer for LocalCameraAuthGrantIssuer {
    fn issue_auth_grant(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        _trigger_source: AuthTriggerSource,
        issued_at_unix_ms: i64,
    ) -> AuthGrantIssueResult {
        match self.issue_auth_grant_blocking(session_id, source, issued_at_unix_ms) {
            Ok(grant) => AuthGrantIssueResult::Issued(grant),
            Err(reason) => AuthGrantIssueResult::Failed(reason),
        }
    }

    fn issue_auth_grant_with_observer(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        _trigger_source: AuthTriggerSource,
        issued_at_unix_ms: i64,
        on_grant_issued: &mut dyn FnMut(&AuthGrant),
    ) -> AuthGrantIssueResult {
        match self.issue_auth_grant_blocking_with_observer(
            session_id,
            source,
            issued_at_unix_ms,
            on_grant_issued,
        ) {
            Ok(grant) => AuthGrantIssueResult::Issued(grant),
            Err(reason) => AuthGrantIssueResult::Failed(reason),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MiniFasNetWindowEvidence {
    evaluated_frame_count: u32,
    spoof_frame_count: u32,
    max_spoof_frame_ratio: f32,
}

impl MiniFasNetWindowEvidence {
    fn new(max_spoof_frame_ratio: f32) -> Self {
        Self {
            evaluated_frame_count: 0,
            spoof_frame_count: 0,
            max_spoof_frame_ratio,
        }
    }

    fn record(&mut self, result: &LivenessResult) -> Result<Option<f32>, AuthFailureReason> {
        self.evaluated_frame_count = self.evaluated_frame_count.saturating_add(1);
        match result.liveness_decision {
            LivenessDecision::LiveAccepted => result
                .liveness_score
                .map(Some)
                .ok_or(AuthFailureReason::InternalError),
            LivenessDecision::SpoofRejected => {
                self.spoof_frame_count = self.spoof_frame_count.saturating_add(1);
                Ok(None)
            }
            LivenessDecision::Inconclusive => Ok(None),
            LivenessDecision::ProviderUnavailable => Err(AuthFailureReason::InternalError),
        }
    }

    fn spoof_frame_ratio(&self) -> f32 {
        if self.evaluated_frame_count == 0 {
            return 0.0;
        }

        self.spoof_frame_count as f32 / self.evaluated_frame_count as f32
    }

    fn reject_unlock_candidate_if_spoof_ratio_exceeded(&self) -> Result<(), AuthFailureReason> {
        if self.spoof_frame_ratio() > self.max_spoof_frame_ratio {
            return Err(AuthFailureReason::LivenessFailed);
        }

        Ok(())
    }
}

fn liveness_error_to_auth_failure(_error: LivenessProviderError) -> AuthFailureReason {
    AuthFailureReason::InternalError
}

fn read_face_templates(
    template_path: &std::path::Path,
) -> Result<Vec<FaceTemplate>, ProtocolError> {
    let bytes = fs::read(template_path).map_err(|_| ProtocolError::InvalidMessage)?;
    if let Ok(template_set) = FaceTemplateSet::from_json_bytes(&bytes) {
        return Ok(template_set.selected_templates());
    }

    FaceTemplate::from_json_bytes(&bytes)
        .map(|template| vec![template])
        .map_err(template_codec_to_protocol_error)
}

fn template_codec_to_protocol_error(_error: FaceTemplateCodecError) -> ProtocolError {
    ProtocolError::InvalidMessage
}

fn video_error_to_auth_failure(error: VideoError) -> AuthFailureReason {
    match error {
        VideoError::EmptyFrame | VideoError::ReadFailed => AuthFailureReason::NoFaceDetected,
        VideoError::ProviderUnavailable
        | VideoError::CameraNotFound
        | VideoError::CameraAlreadyOpen
        | VideoError::CameraNotOpen
        | VideoError::OpenFailed
        | VideoError::UnsupportedFormat => AuthFailureReason::InternalError,
    }
}

fn handle_auth_transient_frame_failure(
    frame_index: u32,
    kind: TransientFrameFailureKind,
    tolerance: &mut TransientFrameFailureTolerance,
) -> Result<(), AuthFailureReason> {
    match tolerance.record_transient_failure(kind) {
        TransientFrameFailureDecision::RetryNextFrame {
            consecutive_failures,
            max_consecutive_failures,
        } => {
            write_service_event_detail(
                "LocalCameraAuth.TransientFrameSkipped",
                format!(
                    "frame_index={frame_index} reason={kind:?} consecutive_failures={consecutive_failures} max_consecutive_failures={max_consecutive_failures}"
                ),
            );
            Ok(())
        }
        TransientFrameFailureDecision::Escalate {
            consecutive_failures,
            max_consecutive_failures,
        } => {
            write_service_event_detail(
                "LocalCameraAuth.TransientFrameEscalated",
                format!(
                    "frame_index={frame_index} reason={kind:?} consecutive_failures={consecutive_failures} max_consecutive_failures={max_consecutive_failures}"
                ),
            );
            Err(AuthFailureReason::NoFaceDetected)
        }
    }
}

fn build_auth_grant(
    session_id: &SessionId,
    source: AuthSource,
    grant_sequence: u64,
    outcome: &LocalCameraAuthenticationOutcome,
) -> AuthGrant {
    let issued_at_unix_ms = current_time_unix_ms();
    AuthGrant {
        grant_id: GrantId(format!("camera-grant-{grant_sequence}")),
        nonce: Nonce(format!("camera-nonce-{grant_sequence}")),
        session_id: session_id.clone(),
        user_id: outcome.face_authentication.matched_user_id.clone(),
        source,
        score: AuthScore {
            match_score: outcome.face_authentication.match_score,
            liveness_score: Some(outcome.liveness_score),
        },
        issued_at_unix_ms,
        expires_at_unix_ms: issued_at_unix_ms + DEFAULT_GRANT_TTL.as_millis() as i64,
    }
}

fn current_time_unix_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    millis.min(i64::MAX as u128) as i64
}

fn auth_trigger_source_name(trigger_source: AuthTriggerSource) -> &'static str {
    match trigger_source {
        AuthTriggerSource::CredentialScreenEntered => "credential-screen-entered",
        AuthTriggerSource::BackgroundSilentMonitor => "background-silent-monitor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_accepted_exposes_score_for_template_matching() {
        let mut window = MiniFasNetWindowEvidence::new(0.40);
        let result = LivenessResult {
            liveness_decision: LivenessDecision::LiveAccepted,
            liveness_score: Some(0.98),
            evidence: Vec::new(),
        };

        assert_eq!(window.record(&result), Ok(Some(0.98)));
    }

    #[test]
    fn single_spoof_frame_is_recorded_without_stopping_authentication_window() {
        let mut window = MiniFasNetWindowEvidence::new(0.40);
        let result = LivenessResult {
            liveness_decision: LivenessDecision::SpoofRejected,
            liveness_score: Some(0.01),
            evidence: Vec::new(),
        };

        assert_eq!(window.record(&result), Ok(None));
        assert_eq!(window.evaluated_frame_count, 1);
        assert_eq!(window.spoof_frame_count, 1);
    }

    #[test]
    fn inconclusive_collects_another_frame() {
        let mut window = MiniFasNetWindowEvidence::new(0.40);
        let result = LivenessResult {
            liveness_decision: LivenessDecision::Inconclusive,
            liveness_score: Some(0.50),
            evidence: Vec::new(),
        };

        assert_eq!(window.record(&result), Ok(None));
    }

    #[test]
    fn unlock_candidate_is_rejected_when_dynamic_spoof_ratio_exceeds_limit()
    -> Result<(), AuthFailureReason> {
        let mut window = MiniFasNetWindowEvidence::new(0.40);

        for _ in 0..7 {
            window.record(&spoof_result())?;
        }
        for _ in 0..5 {
            window.record(&live_result())?;
        }

        assert_eq!(window.evaluated_frame_count, 12);
        assert_eq!(window.spoof_frame_count, 7);
        assert_eq!(
            window.reject_unlock_candidate_if_spoof_ratio_exceeded(),
            Err(AuthFailureReason::LivenessFailed)
        );
        Ok(())
    }

    #[test]
    fn unlock_candidate_passes_without_waiting_for_full_window_when_dynamic_ratio_is_allowed()
    -> Result<(), AuthFailureReason> {
        let mut window = MiniFasNetWindowEvidence::new(0.40);

        window.record(&spoof_result())?;
        for _ in 0..4 {
            window.record(&live_result())?;
        }

        assert_eq!(window.evaluated_frame_count, 5);
        assert_eq!(window.spoof_frame_count, 1);
        assert_eq!(
            window.reject_unlock_candidate_if_spoof_ratio_exceeded(),
            Ok(())
        );
        Ok(())
    }

    #[test]
    fn spoof_ratio_equal_to_limit_is_allowed() -> Result<(), AuthFailureReason> {
        let mut window = MiniFasNetWindowEvidence::new(0.40);

        for _ in 0..2 {
            window.record(&spoof_result())?;
        }
        for _ in 0..3 {
            window.record(&live_result())?;
        }

        assert_eq!(window.spoof_frame_ratio(), 0.40);
        assert_eq!(
            window.reject_unlock_candidate_if_spoof_ratio_exceeded(),
            Ok(())
        );
        Ok(())
    }

    fn live_result() -> LivenessResult {
        LivenessResult {
            liveness_decision: LivenessDecision::LiveAccepted,
            liveness_score: Some(0.98),
            evidence: Vec::new(),
        }
    }

    fn spoof_result() -> LivenessResult {
        LivenessResult {
            liveness_decision: LivenessDecision::SpoofRejected,
            liveness_score: Some(0.01),
            evidence: Vec::new(),
        }
    }
}
