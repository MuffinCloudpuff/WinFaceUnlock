use std::time::{SystemTime, UNIX_EPOCH};

use crate::GrantRegistry;
use common_protocol::{
    AuthFailureReason, AuthGrant, AuthSource, AuthTriggerSource, ProtectedCredential,
    ProtectedCredentialMaterial, ProtocolError, ServiceEvent, ServiceRequest, SessionId,
};

pub trait AuthGrantIssuer {
    fn issue_auth_grant(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        trigger_source: AuthTriggerSource,
        issued_at_unix_ms: i64,
    ) -> AuthGrantIssueResult;

    fn issue_auth_grant_with_observer(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        trigger_source: AuthTriggerSource,
        issued_at_unix_ms: i64,
        on_grant_issued: &mut dyn FnMut(&AuthGrant),
    ) -> AuthGrantIssueResult {
        let result = self.issue_auth_grant(session_id, source, trigger_source, issued_at_unix_ms);
        if let AuthGrantIssueResult::Issued(grant) = &result {
            on_grant_issued(grant);
        }
        result
    }

    fn fetch_auth_result(
        &mut self,
        _session_id: &SessionId,
        _issued_at_unix_ms: i64,
    ) -> Option<Result<AuthGrant, AuthFailureReason>> {
        None
    }

    fn cancel_auth(&mut self, _session_id: &SessionId) {}

    fn reload_auth_config(&mut self) -> Result<(), ProtocolError> {
        Ok(())
    }

    fn apply_local_camera_auth_config(
        &mut self,
        _template_path: &str,
        _camera_id: &str,
        _install_dir: &str,
    ) -> Result<(), ProtocolError> {
        Err(ProtocolError::Unauthorized)
    }

    fn update_settings(
        &mut self,
        _patch: &control_protocol::ControlSettingsPatch,
    ) -> Result<(), ProtocolError> {
        Err(ProtocolError::Unauthorized)
    }

    fn check_windows_credential(
        &mut self,
        _user_id: &common_protocol::UserId,
        _credential_ref: &common_protocol::CredentialRef,
    ) -> Result<bool, ProtocolError> {
        Ok(false)
    }

    fn enroll_windows_credential(
        &mut self,
        _user_id: &common_protocol::UserId,
        _user_sid: &str,
        _username: &str,
        _account_type: &common_protocol::AccountType,
        _credential_ref: &common_protocol::CredentialRef,
        _password_secret: &str,
    ) -> Result<(), ProtocolError> {
        Err(ProtocolError::Unauthorized)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AuthGrantIssueResult {
    Issued(AuthGrant),
    Started,
    Failed(AuthFailureReason),
}

pub trait ProtectedCredentialResolver {
    fn resolve_protected_credential(
        &mut self,
        grant: &AuthGrant,
    ) -> Result<ProtectedCredential, ProtocolError>;
}

pub trait ProtectedCredentialMaterialResolver {
    fn resolve_protected_credential_material(
        &mut self,
        grant: &AuthGrant,
    ) -> Result<ProtectedCredentialMaterial, ProtocolError>;
}

pub trait UnixTimeMillisClock {
    fn now_unix_ms(&self) -> i64;
}

#[derive(Default)]
pub struct SystemUnixTimeMillisClock;

impl UnixTimeMillisClock for SystemUnixTimeMillisClock {
    fn now_unix_ms(&self) -> i64 {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        millis.min(i64::MAX as u128) as i64
    }
}

pub struct ServiceRequestHandler<I, R, C> {
    grant_issuer: I,
    credential_resolver: R,
    clock: C,
    grant_registry: GrantRegistry,
}

impl<I, R, C> ServiceRequestHandler<I, R, C>
where
    I: AuthGrantIssuer,
    R: ProtectedCredentialResolver + ProtectedCredentialMaterialResolver,
    C: UnixTimeMillisClock,
{
    pub fn new(grant_issuer: I, credential_resolver: R, clock: C) -> Self {
        Self {
            grant_issuer,
            credential_resolver,
            clock,
            grant_registry: GrantRegistry::default(),
        }
    }

    pub fn handle_request(
        &mut self,
        request: ServiceRequest,
    ) -> Result<ServiceEvent, ProtocolError> {
        match request {
            ServiceRequest::WakeAuth {
                session_id,
                source,
                trigger_source,
            } => self.handle_wake_auth(session_id, source, trigger_source),
            ServiceRequest::FetchAuthResult { session_id } => {
                self.handle_fetch_auth_result(session_id)
            }
            ServiceRequest::FetchCredential {
                session_id,
                grant_id,
                nonce,
            } => {
                let grant = self.grant_registry.redeem_grant_for_session(
                    &grant_id,
                    &nonce,
                    &session_id,
                    self.clock.now_unix_ms(),
                )?;
                let protected_credential = self
                    .credential_resolver
                    .resolve_protected_credential(&grant)?;

                Ok(ServiceEvent::CredentialReady {
                    grant_id,
                    protected_credential,
                })
            }
            ServiceRequest::FetchCredentialMaterial {
                session_id,
                grant_id,
                nonce,
            } => {
                let grant = self.grant_registry.redeem_grant_for_session(
                    &grant_id,
                    &nonce,
                    &session_id,
                    self.clock.now_unix_ms(),
                )?;
                let protected_credential_material = self
                    .credential_resolver
                    .resolve_protected_credential_material(&grant)?;

                Ok(ServiceEvent::CredentialMaterialReady {
                    grant_id,
                    protected_credential_material,
                })
            }
            ServiceRequest::Cancel { session_id } => {
                self.grant_issuer.cancel_auth(&session_id);
                self.grant_registry.remove_grants_for_session(&session_id);
                Ok(ServiceEvent::AuthCancelled { session_id })
            }
            ServiceRequest::ReloadAuthConfig => {
                self.grant_issuer.reload_auth_config()?;
                Ok(ServiceEvent::AuthConfigReloaded)
            }
            ServiceRequest::ApplyLocalCameraAuthConfig {
                template_path,
                camera_id,
                install_dir,
            } => {
                self.grant_issuer.apply_local_camera_auth_config(
                    &template_path,
                    &camera_id,
                    &install_dir,
                )?;
                Ok(ServiceEvent::AuthConfigReloaded)
            }
            ServiceRequest::UpdateSettings { patch } => {
                self.grant_issuer.update_settings(&patch)?;
                Ok(ServiceEvent::AuthConfigReloaded)
            }
            ServiceRequest::HealthCheck => Ok(ServiceEvent::HealthOk),
            ServiceRequest::CheckWindowsCredential {
                user_id,
                credential_ref,
            } => {
                let configured = self
                    .grant_issuer
                    .check_windows_credential(&user_id, &credential_ref)?;
                Ok(ServiceEvent::WindowsCredentialStatus { configured })
            }
            ServiceRequest::EnrollWindowsCredential {
                user_id,
                user_sid,
                username,
                account_type,
                credential_ref,
                password_secret,
            } => {
                self.grant_issuer.enroll_windows_credential(
                    &user_id,
                    &user_sid,
                    &username,
                    &account_type,
                    &credential_ref,
                    &password_secret,
                )?;
                Ok(ServiceEvent::WindowsCredentialEnrolled)
            }
        }
    }

    fn handle_wake_auth(
        &mut self,
        session_id: SessionId,
        source: AuthSource,
        trigger_source: AuthTriggerSource,
    ) -> Result<ServiceEvent, ProtocolError> {
        let issued_at_unix_ms = self.clock.now_unix_ms();
        let grant_result = self.grant_issuer.issue_auth_grant(
            &session_id,
            source,
            trigger_source,
            issued_at_unix_ms,
        );

        match grant_result {
            AuthGrantIssueResult::Issued(grant) => {
                self.grant_registry.insert_issued_grant(grant.clone())?;
                Ok(ServiceEvent::AuthSucceeded { grant })
            }
            AuthGrantIssueResult::Started => Ok(ServiceEvent::AuthStarted { session_id }),
            AuthGrantIssueResult::Failed(reason) => {
                Ok(ServiceEvent::AuthFailed { session_id, reason })
            }
        }
    }

    fn handle_fetch_auth_result(
        &mut self,
        session_id: SessionId,
    ) -> Result<ServiceEvent, ProtocolError> {
        let issued_at_unix_ms = self.clock.now_unix_ms();
        let Some(grant_result) = self
            .grant_issuer
            .fetch_auth_result(&session_id, issued_at_unix_ms)
        else {
            return Ok(ServiceEvent::AuthStarted { session_id });
        };

        match grant_result {
            Ok(grant) => {
                self.grant_registry.insert_issued_grant(grant.clone())?;
                Ok(ServiceEvent::AuthSucceeded { grant })
            }
            Err(reason) => Ok(ServiceEvent::AuthFailed { session_id, reason }),
        }
    }
}

#[cfg(test)]
mod tests {
    use common_protocol::{
        AuthScore, CredentialMaterialProtection, CredentialRef, DEFAULT_GRANT_TTL, GrantId, Nonce,
        UserId,
    };

    use super::*;

    #[derive(Clone, Copy)]
    struct FixedClock {
        now_unix_ms: i64,
    }

    impl UnixTimeMillisClock for FixedClock {
        fn now_unix_ms(&self) -> i64 {
            self.now_unix_ms
        }
    }

    struct SuccessfulGrantIssuer;

    impl AuthGrantIssuer for SuccessfulGrantIssuer {
        fn issue_auth_grant(
            &mut self,
            session_id: &SessionId,
            source: AuthSource,
            _trigger_source: AuthTriggerSource,
            issued_at_unix_ms: i64,
        ) -> AuthGrantIssueResult {
            AuthGrantIssueResult::Issued(AuthGrant {
                grant_id: GrantId("grant-1".to_owned()),
                nonce: Nonce("nonce-1".to_owned()),
                session_id: session_id.clone(),
                user_id: UserId("user-1".to_owned()),
                source,
                score: AuthScore {
                    match_score: 0.82,
                    liveness_score: None,
                },
                issued_at_unix_ms,
                expires_at_unix_ms: issued_at_unix_ms + DEFAULT_GRANT_TTL.as_millis() as i64,
            })
        }
    }

    struct FailingGrantIssuer;

    impl AuthGrantIssuer for FailingGrantIssuer {
        fn issue_auth_grant(
            &mut self,
            _session_id: &SessionId,
            _source: AuthSource,
            _trigger_source: AuthTriggerSource,
            _issued_at_unix_ms: i64,
        ) -> AuthGrantIssueResult {
            AuthGrantIssueResult::Failed(AuthFailureReason::NoFaceDetected)
        }
    }

    struct StartedThenSuccessfulGrantIssuer;
    struct ReloadRecordingGrantIssuer {
        reload_count: usize,
    }

    impl AuthGrantIssuer for StartedThenSuccessfulGrantIssuer {
        fn issue_auth_grant(
            &mut self,
            _session_id: &SessionId,
            _source: AuthSource,
            _trigger_source: AuthTriggerSource,
            _issued_at_unix_ms: i64,
        ) -> AuthGrantIssueResult {
            AuthGrantIssueResult::Started
        }

        fn fetch_auth_result(
            &mut self,
            session_id: &SessionId,
            issued_at_unix_ms: i64,
        ) -> Option<Result<AuthGrant, AuthFailureReason>> {
            Some(Ok(AuthGrant {
                grant_id: GrantId("grant-1".to_owned()),
                nonce: Nonce("nonce-1".to_owned()),
                session_id: session_id.clone(),
                user_id: UserId("user-1".to_owned()),
                source: AuthSource::LocalCamera,
                score: AuthScore {
                    match_score: 0.82,
                    liveness_score: None,
                },
                issued_at_unix_ms,
                expires_at_unix_ms: issued_at_unix_ms + DEFAULT_GRANT_TTL.as_millis() as i64,
            }))
        }
    }

    impl AuthGrantIssuer for ReloadRecordingGrantIssuer {
        fn issue_auth_grant(
            &mut self,
            _session_id: &SessionId,
            _source: AuthSource,
            _trigger_source: AuthTriggerSource,
            _issued_at_unix_ms: i64,
        ) -> AuthGrantIssueResult {
            AuthGrantIssueResult::Failed(AuthFailureReason::InternalError)
        }

        fn reload_auth_config(&mut self) -> Result<(), ProtocolError> {
            self.reload_count += 1;
            Ok(())
        }
    }

    struct FixedCredentialResolver;
    impl ProtectedCredentialResolver for FixedCredentialResolver {
        fn resolve_protected_credential(
            &mut self,
            grant: &AuthGrant,
        ) -> Result<ProtectedCredential, ProtocolError> {
            Ok(ProtectedCredential {
                user_id: grant.user_id.clone(),
                credential_ref: CredentialRef("cred-1".to_owned()),
            })
        }
    }

    impl ProtectedCredentialMaterialResolver for FixedCredentialResolver {
        fn resolve_protected_credential_material(
            &mut self,
            grant: &AuthGrant,
        ) -> Result<ProtectedCredentialMaterial, ProtocolError> {
            Ok(ProtectedCredentialMaterial {
                user_id: grant.user_id.clone(),
                domain: ".".to_owned(),
                username: "test-user".to_owned(),
                protected_password: vec![1, 2, 3],
                protection: CredentialMaterialProtection::DpapiLocalMachineV1,
            })
        }
    }

    #[test]
    fn handler_issues_grant_then_redeems_protected_credential_once() -> Result<(), ProtocolError> {
        let mut handler = ServiceRequestHandler::new(
            SuccessfulGrantIssuer,
            FixedCredentialResolver,
            FixedClock { now_unix_ms: 1_000 },
        );

        let issued = handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::LocalCamera,
            trigger_source: AuthTriggerSource::CredentialScreenEntered,
        })?;
        let ready = handler.handle_request(ServiceRequest::FetchCredential {
            session_id: SessionId("session-1".to_owned()),
            grant_id: GrantId("grant-1".to_owned()),
            nonce: Nonce("nonce-1".to_owned()),
        })?;
        let replay = handler.handle_request(ServiceRequest::FetchCredential {
            session_id: SessionId("session-1".to_owned()),
            grant_id: GrantId("grant-1".to_owned()),
            nonce: Nonce("nonce-1".to_owned()),
        });

        assert!(matches!(issued, ServiceEvent::AuthSucceeded { .. }));
        assert_eq!(
            ready,
            ServiceEvent::CredentialReady {
                grant_id: GrantId("grant-1".to_owned()),
                protected_credential: ProtectedCredential {
                    user_id: UserId("user-1".to_owned()),
                    credential_ref: CredentialRef("cred-1".to_owned()),
                },
            }
        );
        assert_eq!(replay, Err(ProtocolError::UsedGrant));
        Ok(())
    }

    #[test]
    fn handler_rejects_wrong_session_when_fetching_credential() -> Result<(), ProtocolError> {
        let mut handler = ServiceRequestHandler::new(
            SuccessfulGrantIssuer,
            FixedCredentialResolver,
            FixedClock { now_unix_ms: 1_000 },
        );

        handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::LocalCamera,
            trigger_source: AuthTriggerSource::CredentialScreenEntered,
        })?;
        let result = handler.handle_request(ServiceRequest::FetchCredential {
            session_id: SessionId("other-session".to_owned()),
            grant_id: GrantId("grant-1".to_owned()),
            nonce: Nonce("nonce-1".to_owned()),
        });

        assert_eq!(result, Err(ProtocolError::SessionMismatch));
        Ok(())
    }

    #[test]
    fn handler_redeems_grant_for_protected_credential_material_once() -> Result<(), ProtocolError> {
        let mut handler = ServiceRequestHandler::new(
            SuccessfulGrantIssuer,
            FixedCredentialResolver,
            FixedClock { now_unix_ms: 1_000 },
        );

        handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::LocalCamera,
            trigger_source: AuthTriggerSource::CredentialScreenEntered,
        })?;
        let ready = handler.handle_request(ServiceRequest::FetchCredentialMaterial {
            session_id: SessionId("session-1".to_owned()),
            grant_id: GrantId("grant-1".to_owned()),
            nonce: Nonce("nonce-1".to_owned()),
        })?;
        let replay = handler.handle_request(ServiceRequest::FetchCredentialMaterial {
            session_id: SessionId("session-1".to_owned()),
            grant_id: GrantId("grant-1".to_owned()),
            nonce: Nonce("nonce-1".to_owned()),
        });

        assert!(matches!(
            ready,
            ServiceEvent::CredentialMaterialReady { .. }
        ));
        assert_eq!(replay, Err(ProtocolError::UsedGrant));
        Ok(())
    }

    #[test]
    fn handler_returns_auth_failed_event_for_failed_auth_attempt() -> Result<(), ProtocolError> {
        let mut handler = ServiceRequestHandler::new(
            FailingGrantIssuer,
            FixedCredentialResolver,
            FixedClock { now_unix_ms: 1_000 },
        );

        let event = handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::LocalCamera,
            trigger_source: AuthTriggerSource::CredentialScreenEntered,
        })?;

        assert_eq!(
            event,
            ServiceEvent::AuthFailed {
                session_id: SessionId("session-1".to_owned()),
                reason: AuthFailureReason::NoFaceDetected,
            }
        );
        Ok(())
    }

    #[test]
    fn handler_returns_started_then_fetches_background_auth_result() -> Result<(), ProtocolError> {
        let mut handler = ServiceRequestHandler::new(
            StartedThenSuccessfulGrantIssuer,
            FixedCredentialResolver,
            FixedClock { now_unix_ms: 1_000 },
        );
        let session_id = SessionId("session-1".to_owned());

        let started = handler.handle_request(ServiceRequest::WakeAuth {
            session_id: session_id.clone(),
            source: AuthSource::LocalCamera,
            trigger_source: AuthTriggerSource::CredentialScreenEntered,
        })?;
        let succeeded = handler.handle_request(ServiceRequest::FetchAuthResult {
            session_id: session_id.clone(),
        })?;
        let ready = handler.handle_request(ServiceRequest::FetchCredential {
            session_id,
            grant_id: GrantId("grant-1".to_owned()),
            nonce: Nonce("nonce-1".to_owned()),
        })?;

        assert!(matches!(started, ServiceEvent::AuthStarted { .. }));
        assert!(matches!(succeeded, ServiceEvent::AuthSucceeded { .. }));
        assert!(matches!(ready, ServiceEvent::CredentialReady { .. }));
        Ok(())
    }

    #[test]
    fn handler_cancels_session_grants() -> Result<(), ProtocolError> {
        let mut handler = ServiceRequestHandler::new(
            SuccessfulGrantIssuer,
            FixedCredentialResolver,
            FixedClock { now_unix_ms: 1_000 },
        );

        handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::LocalCamera,
            trigger_source: AuthTriggerSource::CredentialScreenEntered,
        })?;
        let cancel_event = handler.handle_request(ServiceRequest::Cancel {
            session_id: SessionId("session-1".to_owned()),
        })?;
        let fetch_result = handler.handle_request(ServiceRequest::FetchCredential {
            session_id: SessionId("session-1".to_owned()),
            grant_id: GrantId("grant-1".to_owned()),
            nonce: Nonce("nonce-1".to_owned()),
        });

        assert_eq!(
            cancel_event,
            ServiceEvent::AuthCancelled {
                session_id: SessionId("session-1".to_owned()),
            }
        );
        assert_eq!(fetch_result, Err(ProtocolError::InvalidMessage));
        Ok(())
    }

    #[test]
    fn handler_reloads_auth_config() -> Result<(), ProtocolError> {
        let mut handler = ServiceRequestHandler::new(
            ReloadRecordingGrantIssuer { reload_count: 0 },
            FixedCredentialResolver,
            FixedClock { now_unix_ms: 1_000 },
        );

        let event = handler.handle_request(ServiceRequest::ReloadAuthConfig)?;

        assert_eq!(event, ServiceEvent::AuthConfigReloaded);
        assert_eq!(handler.grant_issuer.reload_count, 1);
        Ok(())
    }
}
