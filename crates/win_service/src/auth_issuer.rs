use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
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
    black_frame::BlackFrameFilter,
    camera_backend_profiles::apply_profile_to_config,
    camera_frame_recovery::{
        TransientFrameFailureDecision, TransientFrameFailureKind, TransientFrameFailureTolerance,
        validate_frame_for_camera_stream,
    },
    camera_runtime::{CameraLeaseKind, acquire_camera_lease_until},
    service_config::{LocalCameraAuthConfig, ServiceAuthConfig, ServiceAuthMode},
    service_log::{write_service_event, write_service_event_detail},
    simulated_auth::SimulatedAuthGrantIssuer,
    snapshot_policy::{SnapshotOutcome, SnapshotPolicy},
};
use std::time::Duration;

// IPC retains grant identities across issuer replacement. Never reset this
// sequence while the service process and its authorization registry are alive.
static NEXT_CAMERA_GRANT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub struct DevelopmentAuthGrantIssuer {
    manual_test_user_id: UserId,
    manual_test_issuer: SimulatedAuthGrantIssuer,
    local_camera_issuer: Option<CameraAuthOrchestrator<LocalCameraAuthGrantIssuer>>,
    snapshot_policy: SnapshotPolicy,
    auth_mode_unavailable: bool,
}

impl DevelopmentAuthGrantIssuer {
    pub fn from_environment(manual_test_user_id: UserId) -> Result<Self, ProtocolError> {
        Self::from_config(manual_test_user_id, ServiceAuthConfig::from_environment()?)
    }

    pub fn from_config(
        manual_test_user_id: UserId,
        config: ServiceAuthConfig,
    ) -> Result<Self, ProtocolError> {
        let auth_mode_unavailable =
            matches!(config.auth_mode, ServiceAuthMode::LocalCameraUnavailable);
        let snapshot_policy = snapshot_policy_from_config(&config);
        let local_camera_issuer = local_camera_issuer_from_config(config, snapshot_policy.clone())?;

        Ok(Self {
            manual_test_issuer: SimulatedAuthGrantIssuer::for_user(manual_test_user_id.clone()),
            manual_test_user_id,
            local_camera_issuer,
            snapshot_policy,
            auth_mode_unavailable,
        })
    }

    fn reload_from_environment(&mut self) -> Result<(), ProtocolError> {
        self.reload_with_config_loader(ServiceAuthConfig::from_environment)
    }

    fn reload_with_config_loader(
        &mut self,
        load_config: impl FnOnce() -> Result<ServiceAuthConfig, ProtocolError>,
    ) -> Result<(), ProtocolError> {
        // Old jobs can outlive their orchestrator. Revoke before any fallible reload
        // so even a failed reload cannot leave old workers saving private images.
        self.snapshot_policy.revoke();
        let previous_camera_id = self.current_local_camera_id();
        // Detach every old result cache before loading persisted settings or
        // templates. A failed reload must not continue authorizing with old state.
        self.local_camera_issuer = None;
        self.auth_mode_unavailable = true;
        let config = load_config()?;
        if matches!(config.auth_mode, ServiceAuthMode::LocalCameraUnavailable) {
            write_service_event("AuthIssuer.ReloadUnavailable");
            return Err(ProtocolError::TransportUnavailable);
        }
        let snapshot_policy = snapshot_policy_from_config(&config);
        let local_camera_issuer = local_camera_issuer_from_config(config, snapshot_policy.clone())?;
        self.local_camera_issuer = local_camera_issuer;
        self.snapshot_policy = snapshot_policy;
        self.auth_mode_unavailable = false;
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

    fn invalidate_auth_results(&mut self) {
        if let Some(issuer) = self.local_camera_issuer.as_mut() {
            issuer.invalidate_results();
        }
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
        if self.auth_mode_unavailable {
            return AuthGrantIssueResult::Failed(AuthFailureReason::InternalError);
        }
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
        if self.auth_mode_unavailable {
            return Some(Err(AuthFailureReason::InternalError));
        }
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

    fn apply_local_camera_auth_config(
        &mut self,
        template_path: &str,
        camera_id: &str,
        install_dir: &str,
    ) -> Result<(), ProtocolError> {
        use control_status::WindowsFaceTemplateStatusStore;
        self.invalidate_auth_results();
        let template_store = WindowsFaceTemplateStatusStore::from_environment_or_default();
        let install_path = std::path::Path::new(install_dir);
        let template_path = std::path::Path::new(template_path);
        let camera_id = video_provider::resolve_camera_id(&CameraId(camera_id.to_owned()))
            .map_err(|_| ProtocolError::InvalidMessage)?;

        template_store
            .apply_local_camera_auth_config(template_path, &camera_id.0, install_path)
            .map_err(|e| {
                write_service_event_detail(
                    "AuthIssuer.ApplyLocalCameraAuthConfigFailed",
                    format!("{:?}", e),
                );
                ProtocolError::InvalidMessage
            })?;

        self.reload_from_environment()
    }

    fn update_settings(
        &mut self,
        patch: &control_protocol::ControlSettingsPatch,
    ) -> Result<(), ProtocolError> {
        use control_status::WindowsControlSettingsStore;
        let settings_store = WindowsControlSettingsStore::new();

        if !patch.has_updates() || !patch.has_valid_values() {
            return Err(ProtocolError::InvalidMessage);
        }
        self.invalidate_auth_results();
        if patch.intruder_snap_enabled == Some(false) {
            // Revoke before persistence: a failed disk/registry write must not
            // allow an in-flight task to continue capturing after an opt-out.
            self.snapshot_policy.revoke();
        }

        settings_store.update_settings(patch).map_err(|e| {
            write_service_event_detail("AuthIssuer.UpdateSettingsFailed", format!("{:?}", e));
            ProtocolError::InvalidMessage
        })?;

        if patch.intruder_snap_enabled == Some(false)
            && patch.presence_lock_enabled.is_none()
            && patch.logon_wake_mode.is_none()
            && patch.logon_face_match_threshold.is_none()
            && patch.intruder_similarity_threshold.is_none()
        {
            // Disabling capture needs neither a camera nor a valid face model.
            // The revocation barrier above is already the effective runtime change.
            return Ok(());
        }
        self.reload_from_environment()
    }

    fn check_windows_credential(
        &mut self,
        user_id: &common_protocol::UserId,
        credential_ref: &common_protocol::CredentialRef,
    ) -> Result<bool, ProtocolError> {
        use crate::credential_store_config::{
            ServiceCredentialStorePaths, is_credential_secret_configured,
        };

        let store_paths = ServiceCredentialStorePaths::from_environment_or_default()?;
        is_credential_secret_configured(&store_paths, user_id, credential_ref)
    }

    fn enroll_windows_credential(
        &mut self,
        user_id: &common_protocol::UserId,
        user_sid: &str,
        username: &str,
        account_type: &common_protocol::AccountType,
        credential_ref: &common_protocol::CredentialRef,
        password_secret: &str,
    ) -> Result<(), ProtocolError> {
        use crate::credential_store_config::{
            ServiceCredentialStorePaths, WindowsCredentialEnrollment, enroll_windows_credential,
        };

        let store_paths = ServiceCredentialStorePaths::from_environment_or_default()?;
        let enrollment = WindowsCredentialEnrollment {
            user_id: user_id.clone(),
            user_sid: user_sid.to_owned(),
            username: username.to_owned(),
            account_type: account_type.clone(),
            credential_ref: credential_ref.clone(),
            password: password_secret.to_owned(),
        };

        enroll_windows_credential(&store_paths, enrollment).map_err(|e| {
            write_service_event_detail(
                "AuthIssuer.EnrollWindowsCredentialFailed",
                format!("{:?}", e),
            );
            ProtocolError::InvalidMessage
        })
    }
}

fn local_camera_issuer_from_config(
    config: ServiceAuthConfig,
    snapshot_policy: SnapshotPolicy,
) -> Result<Option<CameraAuthOrchestrator<LocalCameraAuthGrantIssuer>>, ProtocolError> {
    match config.auth_mode {
        ServiceAuthMode::ManualTestOnly | ServiceAuthMode::LocalCameraUnavailable => Ok(None),
        ServiceAuthMode::LocalCamera(local_camera_config) => Ok(Some(CameraAuthOrchestrator::new(
            LocalCameraAuthGrantIssuer::from_config(*local_camera_config, snapshot_policy)?,
        ))),
    }
}

fn snapshot_policy_from_config(config: &ServiceAuthConfig) -> SnapshotPolicy {
    SnapshotPolicy::new(match &config.auth_mode {
        ServiceAuthMode::ManualTestOnly | ServiceAuthMode::LocalCameraUnavailable => false,
        ServiceAuthMode::LocalCamera(config) => config.intruder_snap_enabled,
    })
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
    black_frame_filter: BlackFrameFilter,
    max_spoof_frame_ratio: f32,
    snapshot_policy: SnapshotPolicy,
    next_grant_sequence: &'static AtomicU64,
}

struct LocalCameraAuthenticationOutcome {
    face_authentication: face_auth::AuthenticationOutcome,
    liveness_score: f32,
}

impl LocalCameraAuthGrantIssuer {
    fn from_config(
        config: LocalCameraAuthConfig,
        snapshot_policy: SnapshotPolicy,
    ) -> Result<Self, ProtocolError> {
        let templates = read_face_templates(&config.face_template_path)?;
        let mut camera_config = config.camera_config;
        apply_profile_to_config(&config.camera_id, &mut camera_config);

        let mut model_config =
            HybridFaceModelConfig::new(config.yunet_model_path, config.sface_model_path);
        model_config.recognizer.match_threshold = config.match_threshold;

        let model_provider = HybridFaceModelProvider::new(model_config);
        let matcher = FaceTemplateMatcher::new(config.match_threshold);
        let attempt_policy = AttemptPolicy::new(AttemptPolicyConfig {
            required_consecutive_match_count: config.required_consecutive_match_count,
            intruder_similarity_threshold: config.intruder_similarity_threshold,
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
            black_frame_filter: BlackFrameFilter::default(),
            max_spoof_frame_ratio: config.minifasnet_max_spoof_frame_ratio,
            snapshot_policy,
            next_grant_sequence: &NEXT_CAMERA_GRANT_SEQUENCE,
        })
    }

    fn camera_id(&self) -> &CameraId {
        &self.camera_id
    }

    fn issue_auth_grant_blocking(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        trigger_source: AuthTriggerSource,
        _request_started_at_unix_ms: i64,
    ) -> Result<AuthGrant, AuthFailureReason> {
        self.issue_auth_grant_blocking_with_observer(
            session_id,
            source,
            trigger_source,
            _request_started_at_unix_ms,
            &mut |_| {},
        )
    }

    fn issue_auth_grant_blocking_with_observer(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        trigger_source: AuthTriggerSource,
        _request_started_at_unix_ms: i64,
        on_grant_issued: &mut dyn FnMut(&AuthGrant),
    ) -> Result<AuthGrant, AuthFailureReason> {
        write_service_event_detail(
            "LocalCameraAuth.Started",
            format!("session_id={} camera_id={}", session_id.0, self.camera_id.0),
        );
        let grant_sequence = reserve_camera_grant_sequence(self.next_grant_sequence)?;
        let mut issued_grant = None;
        self.authenticate_from_camera(trigger_source, &mut |outcome| {
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
        trigger_source: AuthTriggerSource,
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

        let auth_result = self.authenticate_from_camera_with_loaded_runtime(
            trigger_source,
            on_authorization_accepted,
        );

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
        trigger_source: AuthTriggerSource,
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
        // Pick up refreshed backend preferences without requiring a service restart.
        let mut camera_config = self.camera_config.clone();
        apply_profile_to_config(&self.camera_id, &mut camera_config);
        let mut camera_provider = OpenCvCameraProvider::new(camera_config);
        let auth_result = (|| {
            let camera_open_started = std::time::Instant::now();
            write_service_event_detail(
                "LocalCameraAuth.OpenCameraStarted",
                format!("camera_id={}", self.camera_id.0),
            );
            camera_provider
                .open(&self.camera_id)
                .map_err(video_error_to_auth_failure)?;
            write_service_event_detail(
                "LocalCameraAuth.OpenCameraSucceeded",
                format!(
                    "backend={} open_ms={}",
                    camera_provider
                        .opened_backend()
                        .map_or("unknown", |backend| backend.as_str()),
                    camera_open_started.elapsed().as_millis(),
                ),
            );
            let mut last_rejection = AuthFailureReason::Timeout;
            let mut frame_failure_tolerance =
                TransientFrameFailureTolerance::default_for_camera_stream();
            let mut liveness_window = MiniFasNetWindowEvidence::new(self.max_spoof_frame_ratio);

            for frame_index in 0..self.max_auth_frames {
                let frame = match camera_provider.read_frame() {
                    Ok(frame) => match validate_frame_for_camera_stream(
                        &frame,
                        frame_failure_tolerance.is_warmed_up(),
                    ) {
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

                if self.black_frame_filter.is_black_frame(&frame) {
                    write_service_event_detail("LocalCameraAuth.FrameSkipped", "reason=BlackFrame");
                    continue;
                }

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
                                trigger_source,
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
                                last_rejection = reason.clone();
                                if matches!(reason, AuthFailureReason::IntruderDetected)
                                    && let SnapshotOutcome::Captured(Err(error)) = self
                                        .snapshot_policy
                                        .capture(|| save_intruder_snapshot(&frame, &detected_face))
                                {
                                    write_service_event_detail(
                                        "LocalCameraAuth.SaveIntruderSnapshotFailed",
                                        format!("error={error:?}"),
                                    );
                                }
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
        trigger_source: AuthTriggerSource,
        issued_at_unix_ms: i64,
    ) -> AuthGrantIssueResult {
        match self.issue_auth_grant_blocking(session_id, source, trigger_source, issued_at_unix_ms)
        {
            Ok(grant) => AuthGrantIssueResult::Issued(grant),
            Err(reason) => AuthGrantIssueResult::Failed(reason),
        }
    }

    fn issue_auth_grant_with_observer(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        trigger_source: AuthTriggerSource,
        issued_at_unix_ms: i64,
        on_grant_issued: &mut dyn FnMut(&AuthGrant),
    ) -> AuthGrantIssueResult {
        match self.issue_auth_grant_blocking_with_observer(
            session_id,
            source,
            trigger_source,
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

fn save_intruder_snapshot(
    frame: &video_provider::VideoFrame,
    detected_face: &face_engine::DetectedFace,
) -> std::io::Result<()> {
    let install_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Program Files\WinFaceUnlock"));

    let intruders_dir = install_dir.join("Intruders");
    if !intruders_dir.exists() {
        std::fs::create_dir_all(&intruders_dir)?;
    }

    if let Ok(entries) = std::fs::read_dir(&intruders_dir) {
        let mut snapshots: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "jpg"))
            .collect();

        if snapshots.len() >= 20 {
            snapshots.sort_by_key(|a| {
                a.path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_prefix("intruder_"))
                    .and_then(|s| s.parse::<u128>().ok())
                    .unwrap_or(0)
            });

            for old_snapshot in snapshots.iter().take(snapshots.len() - 19) {
                let _ = std::fs::remove_file(old_snapshot.path());
            }
        }
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let snapshot_path = intruders_dir.join(format!("intruder_{timestamp}.jpg"));

    let crop_rect = video_provider::image_utils::FaceCropRect {
        x: detected_face.bounds.x,
        y: detected_face.bounds.y,
        width: detected_face.bounds.width,
        height: detected_face.bounds.height,
    };

    if let Ok(jpeg_bytes) =
        video_provider::image_utils::crop_and_encode_face_to_jpeg(frame, &crop_rect)
    {
        std::fs::write(&snapshot_path, jpeg_bytes)?;
    }

    Ok(())
}

fn liveness_error_to_auth_failure(_error: LivenessProviderError) -> AuthFailureReason {
    AuthFailureReason::InternalError
}

fn read_face_templates(
    template_path: &std::path::Path,
) -> Result<face_auth::RecognitionTemplates, ProtocolError> {
    let bytes = fs::read(template_path).map_err(|_| ProtocolError::InvalidMessage)?;
    if let Ok(template_set) = FaceTemplateSet::from_json_bytes(&bytes) {
        let mut templates = face_auth::RecognitionTemplates::new(template_set.selected_templates());
        if !template_set.sample_metadata.is_empty() {
            let total_area: u64 = template_set
                .sample_metadata
                .iter()
                .map(|m| (m.face_box.width * m.face_box.height) as u64)
                .sum();
            let avg_area = (total_area / template_set.sample_metadata.len() as u64) as u32;
            templates = templates.with_average_face_area(avg_area);
        }
        return Ok(templates);
    }

    FaceTemplate::from_json_bytes(&bytes)
        .map(|template| face_auth::RecognitionTemplates::new(vec![template]))
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

fn reserve_camera_grant_sequence(sequence: &AtomicU64) -> Result<u64, AuthFailureReason> {
    sequence
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| AuthFailureReason::InternalError)
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

    fn issuer_with_cached_camera_grant() -> (DevelopmentAuthGrantIssuer, SessionId) {
        let session_id = SessionId("previous-session".to_owned());
        let user_id = UserId("configured-user".to_owned());
        let snapshot_policy = SnapshotPolicy::new(true);
        let local_camera = LocalCameraAuthGrantIssuer {
            camera_id: CameraId::from_index(0),
            camera_config: OpenCvCameraProviderConfig::default(),
            max_auth_frames: 1,
            templates: RecognitionTemplates::new(Vec::new()),
            authenticator: FaceAuthenticator::new(
                HybridFaceModelProvider::new(HybridFaceModelConfig::new(
                    "unused-yunet.onnx".into(),
                    "unused-recognizer.onnx".into(),
                )),
                FaceTemplateMatcher::new(0.65),
                AttemptPolicy::new(AttemptPolicyConfig::default()),
            ),
            liveness_provider: MiniFasNetLivenessProvider::new(Default::default()),
            black_frame_filter: BlackFrameFilter::default(),
            max_spoof_frame_ratio: 0.4,
            snapshot_policy: snapshot_policy.clone(),
            next_grant_sequence: &NEXT_CAMERA_GRANT_SEQUENCE,
        };
        let grant = AuthGrant {
            grant_id: GrantId("previous-grant".to_owned()),
            nonce: Nonce("previous-nonce".to_owned()),
            session_id: session_id.clone(),
            user_id: user_id.clone(),
            source: AuthSource::LocalCamera,
            score: AuthScore {
                match_score: 0.9,
                liveness_score: Some(0.95),
            },
            issued_at_unix_ms: 1_000,
            expires_at_unix_ms: 6_000,
        };
        (
            DevelopmentAuthGrantIssuer {
                manual_test_issuer: SimulatedAuthGrantIssuer::for_user(user_id.clone()),
                manual_test_user_id: user_id,
                local_camera_issuer: Some(CameraAuthOrchestrator::with_cached_grant_for_test(
                    local_camera,
                    grant,
                )),
                snapshot_policy,
                auth_mode_unavailable: false,
            },
            session_id,
        )
    }

    fn assert_failed_reload_blocks_auth(
        issuer: &mut DevelopmentAuthGrantIssuer,
        session_id: &SessionId,
    ) {
        assert!(issuer.local_camera_issuer.is_none());
        assert_eq!(
            issuer.fetch_auth_result(session_id, 1_001),
            Some(Err(AuthFailureReason::InternalError)),
        );
        for source in [
            AuthSource::LocalCamera,
            AuthSource::ManualTest,
            AuthSource::VehicleCamera,
        ] {
            assert_eq!(
                issuer.issue_auth_grant(
                    session_id,
                    source,
                    AuthTriggerSource::CredentialScreenEntered,
                    1_001
                ),
                AuthGrantIssueResult::Failed(AuthFailureReason::InternalError),
            );
        }
        assert_eq!(
            issuer.snapshot_policy.capture(|| ()),
            SnapshotOutcome::Disabled
        );
    }

    #[test]
    fn settings_read_failure_invalidates_old_grant_and_all_auth_sources() {
        let (mut issuer, session_id) = issuer_with_cached_camera_grant();
        assert!(matches!(
            issuer.fetch_auth_result(&session_id, 1_001),
            Some(Ok(_))
        ));
        assert_eq!(
            issuer.reload_with_config_loader(|| Err(ProtocolError::TransportUnavailable)),
            Err(ProtocolError::TransportUnavailable),
        );
        assert_failed_reload_blocks_auth(&mut issuer, &session_id);
    }

    #[test]
    fn explicitly_reloading_unavailable_config_reports_failure_and_revokes_auth() {
        let (mut issuer, session_id) = issuer_with_cached_camera_grant();
        assert_eq!(
            issuer.reload_with_config_loader(|| Ok(ServiceAuthConfig {
                auth_mode: ServiceAuthMode::LocalCameraUnavailable,
            })),
            Err(ProtocolError::TransportUnavailable),
        );
        assert_failed_reload_blocks_auth(&mut issuer, &session_id);
    }

    #[test]
    fn camera_grant_identities_remain_unique_after_issuer_recreation()
    -> Result<(), AuthFailureReason> {
        let (first_issuer, session_id) = issuer_with_cached_camera_grant();
        let first_sequence = first_issuer
            .local_camera_issuer
            .as_ref()
            .ok_or(AuthFailureReason::InternalError)?
            .try_with_issuer(|inner| reserve_camera_grant_sequence(inner.next_grant_sequence))
            .ok_or(AuthFailureReason::InternalError)??;
        drop(first_issuer);
        let (replacement_issuer, _) = issuer_with_cached_camera_grant();
        let replacement_sequence = replacement_issuer
            .local_camera_issuer
            .as_ref()
            .ok_or(AuthFailureReason::InternalError)?
            .try_with_issuer(|inner| reserve_camera_grant_sequence(inner.next_grant_sequence))
            .ok_or(AuthFailureReason::InternalError)??;
        let outcome = LocalCameraAuthenticationOutcome {
            face_authentication: face_auth::AuthenticationOutcome {
                matched_user_id: UserId("configured-user".to_owned()),
                match_score: 0.9,
                matched_template: face_engine::FaceTemplateMatch {
                    template_ref: face_engine::FaceTemplateRef("configured-template".to_owned()),
                    user_id: "configured-user".to_owned(),
                    pose_group: face_engine::FacePoseGroup::FrontalPrimary,
                    score: 0.9,
                    decision: face_engine::FaceMatchDecision::MatchAccepted,
                },
                matched_pose_group: face_engine::FacePoseGroup::FrontalPrimary,
            },
            liveness_score: 0.95,
        };
        let first_grant = build_auth_grant(
            &session_id,
            AuthSource::LocalCamera,
            first_sequence,
            &outcome,
        );
        let replacement_grant = build_auth_grant(
            &session_id,
            AuthSource::LocalCamera,
            replacement_sequence,
            &outcome,
        );
        assert_ne!(first_grant.grant_id, replacement_grant.grant_id);
        assert_ne!(first_grant.nonce, replacement_grant.nonce);
        Ok(())
    }

    #[test]
    fn exhausted_camera_grant_sequence_never_wraps_or_reuses_an_identity() {
        let sequence = AtomicU64::new(u64::MAX - 1);
        assert_eq!(reserve_camera_grant_sequence(&sequence), Ok(u64::MAX - 1));
        for _ in 0..2 {
            assert_eq!(
                reserve_camera_grant_sequence(&sequence),
                Err(AuthFailureReason::InternalError)
            );
            assert_eq!(sequence.load(Ordering::Relaxed), u64::MAX);
        }
    }

    #[test]
    fn malformed_replacement_template_invalidates_old_grant_and_can_be_reconfigured()
    -> std::io::Result<()> {
        let (mut issuer, session_id) = issuer_with_cached_camera_grant();
        assert!(matches!(
            issuer.fetch_auth_result(&session_id, 1_001),
            Some(Ok(_))
        ));
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let template_path = std::env::temp_dir().join(format!(
            "winfaceunlock-invalid-reload-{}-{suffix}.json",
            std::process::id()
        ));
        fs::write(&template_path, b"invalid replacement template")?;
        let result = issuer.reload_with_config_loader(|| {
            Ok(ServiceAuthConfig {
                auth_mode: ServiceAuthMode::LocalCamera(Box::new(LocalCameraAuthConfig {
                    face_template_path: template_path.clone(),
                    camera_id: CameraId::from_index(0),
                    camera_config: OpenCvCameraProviderConfig::default(),
                    yunet_model_path: "unused-yunet.onnx".into(),
                    sface_model_path: "unused-recognizer.onnx".into(),
                    minifasnet_config: Default::default(),
                    minifasnet_max_spoof_frame_ratio: 0.4,
                    max_auth_frames: 1,
                    required_consecutive_match_count: 1,
                    match_threshold: 0.65,
                    intruder_snap_enabled: true,
                    intruder_similarity_threshold: 0.3,
                })),
            })
        });
        fs::remove_file(template_path)?;
        assert_eq!(result, Err(ProtocolError::InvalidMessage));
        assert_failed_reload_blocks_auth(&mut issuer, &session_id);

        // A later successful management reload recovers the same issuer.
        assert_eq!(
            issuer.reload_with_config_loader(|| Ok(ServiceAuthConfig {
                auth_mode: ServiceAuthMode::ManualTestOnly,
            })),
            Ok(())
        );
        assert!(matches!(
            issuer.issue_auth_grant(
                &session_id,
                AuthSource::ManualTest,
                AuthTriggerSource::CredentialScreenEntered,
                1_002,
            ),
            AuthGrantIssueResult::Issued(_)
        ));
        assert_eq!(issuer.fetch_auth_result(&session_id, 1_002), None);
        Ok(())
    }

    #[test]
    fn unavailable_camera_configuration_rejects_all_authentication_sources()
    -> Result<(), ProtocolError> {
        let mut issuer = DevelopmentAuthGrantIssuer::from_config(
            UserId("configured-user".to_owned()),
            ServiceAuthConfig {
                auth_mode: ServiceAuthMode::LocalCameraUnavailable,
            },
        )?;
        for source in [
            AuthSource::ManualTest,
            AuthSource::LocalCamera,
            AuthSource::VehicleCamera,
        ] {
            assert_eq!(
                issuer.issue_auth_grant(
                    &SessionId("session".to_owned()),
                    source,
                    AuthTriggerSource::CredentialScreenEntered,
                    1_000,
                ),
                AuthGrantIssueResult::Failed(AuthFailureReason::InternalError),
            );
        }
        assert_eq!(
            issuer.snapshot_policy.capture(|| ()),
            SnapshotOutcome::Disabled
        );
        Ok(())
    }

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
