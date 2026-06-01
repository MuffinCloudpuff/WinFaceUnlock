use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use common_protocol::{
    AuthFailureReason, AuthGrant, AuthScore, AuthSource, DEFAULT_GRANT_TTL, GrantId, Nonce,
    ProtocolError, SessionId, UserId,
};
use face_auth::{AttemptPolicy, AttemptPolicyConfig, FaceAuthenticator, RecognitionTemplates};
use face_engine::{
    FaceModelProvider, FaceTemplate, FaceTemplateCodecError, FaceTemplateMatcher,
    OpenCvFaceModelConfig, OpenCvFaceModelProvider,
};
use ipc::AuthGrantIssuer;
use video_provider::{
    CameraId, OpenCvCameraProvider, OpenCvCameraProviderConfig, VideoError, VideoFrameProvider,
};

use crate::{
    service_config::{LocalCameraAuthConfig, ServiceAuthConfig, ServiceAuthMode},
    simulated_auth::SimulatedAuthGrantIssuer,
};

pub struct DevelopmentAuthGrantIssuer {
    manual_test_issuer: SimulatedAuthGrantIssuer,
    local_camera_issuer: Option<LocalCameraAuthGrantIssuer>,
}

impl DevelopmentAuthGrantIssuer {
    pub fn from_environment(manual_test_user_id: UserId) -> Result<Self, ProtocolError> {
        Self::from_config(manual_test_user_id, ServiceAuthConfig::from_environment()?)
    }

    pub fn from_config(
        manual_test_user_id: UserId,
        config: ServiceAuthConfig,
    ) -> Result<Self, ProtocolError> {
        let local_camera_issuer = match config.auth_mode {
            ServiceAuthMode::ManualTestOnly => None,
            ServiceAuthMode::LocalCamera(local_camera_config) => Some(
                LocalCameraAuthGrantIssuer::from_config(local_camera_config)?,
            ),
        };

        Ok(Self {
            manual_test_issuer: SimulatedAuthGrantIssuer::for_user(manual_test_user_id),
            local_camera_issuer,
        })
    }
}

impl AuthGrantIssuer for DevelopmentAuthGrantIssuer {
    fn issue_auth_grant(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        issued_at_unix_ms: i64,
    ) -> Result<AuthGrant, AuthFailureReason> {
        match source {
            AuthSource::ManualTest => {
                self.manual_test_issuer
                    .issue_auth_grant(session_id, source, issued_at_unix_ms)
            }
            AuthSource::LocalCamera => {
                let Some(local_camera_issuer) = self.local_camera_issuer.as_mut() else {
                    return Err(AuthFailureReason::InternalError);
                };
                local_camera_issuer.issue_auth_grant(session_id, source, issued_at_unix_ms)
            }
            AuthSource::VehicleCamera => Err(AuthFailureReason::InternalError),
        }
    }
}

struct LocalCameraAuthGrantIssuer {
    camera_id: CameraId,
    camera_config: OpenCvCameraProviderConfig,
    max_auth_frames: u32,
    templates: RecognitionTemplates,
    authenticator: FaceAuthenticator<OpenCvFaceModelProvider>,
    next_grant_sequence: u64,
}

impl LocalCameraAuthGrantIssuer {
    fn from_config(config: LocalCameraAuthConfig) -> Result<Self, ProtocolError> {
        let template = read_face_template(&config.face_template_path)?;
        let templates = RecognitionTemplates::new(vec![template]);

        let mut model_config =
            OpenCvFaceModelConfig::new(config.yunet_model_path, config.sface_model_path);
        model_config.recognizer.match_threshold = config.match_threshold;

        let mut model_provider = OpenCvFaceModelProvider::new(model_config);
        model_provider
            .load_models()
            .map_err(|_| ProtocolError::TransportUnavailable)?;

        let matcher = FaceTemplateMatcher::new(config.match_threshold);
        let attempt_policy = AttemptPolicy::new(AttemptPolicyConfig {
            required_consecutive_match_count: config.required_consecutive_match_count,
            ..AttemptPolicyConfig::default()
        });
        let authenticator = FaceAuthenticator::new(model_provider, matcher, attempt_policy);

        Ok(Self {
            camera_id: config.camera_id,
            camera_config: config.camera_config,
            max_auth_frames: config.max_auth_frames,
            templates,
            authenticator,
            next_grant_sequence: 1,
        })
    }

    fn issue_auth_grant(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        _request_started_at_unix_ms: i64,
    ) -> Result<AuthGrant, AuthFailureReason> {
        let outcome = self.authenticate_from_camera()?;
        let grant_sequence = self.next_grant_sequence;
        self.next_grant_sequence = self.next_grant_sequence.saturating_add(1);
        let issued_at_unix_ms = current_time_unix_ms();

        Ok(AuthGrant {
            grant_id: GrantId(format!("camera-grant-{grant_sequence}")),
            nonce: Nonce(format!("camera-nonce-{grant_sequence}")),
            session_id: session_id.clone(),
            user_id: outcome.matched_user_id,
            source,
            score: AuthScore {
                match_score: outcome.match_score,
                liveness_score: None,
            },
            issued_at_unix_ms,
            expires_at_unix_ms: issued_at_unix_ms + DEFAULT_GRANT_TTL.as_millis() as i64,
        })
    }

    fn authenticate_from_camera(
        &mut self,
    ) -> Result<face_auth::AuthenticationOutcome, AuthFailureReason> {
        let mut camera_provider = OpenCvCameraProvider::new(self.camera_config.clone());
        let auth_result = (|| {
            camera_provider
                .open(&self.camera_id)
                .map_err(video_error_to_auth_failure)?;
            let mut last_rejection = AuthFailureReason::Timeout;

            for _ in 0..self.max_auth_frames {
                let frame = camera_provider
                    .read_frame()
                    .map_err(video_error_to_auth_failure)?;
                match self.authenticator.authenticate_frame(
                    &frame,
                    &self.templates,
                    current_time_unix_ms(),
                ) {
                    Ok(outcome) => return Ok(outcome),
                    Err(reason) => last_rejection = reason,
                }
            }

            Err(last_rejection)
        })();
        camera_provider.close();
        auth_result
    }
}

fn read_face_template(template_path: &std::path::Path) -> Result<FaceTemplate, ProtocolError> {
    let bytes = fs::read(template_path).map_err(|_| ProtocolError::InvalidMessage)?;
    FaceTemplate::from_json_bytes(&bytes).map_err(template_codec_to_protocol_error)
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

fn current_time_unix_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    millis.min(i64::MAX as u128) as i64
}
