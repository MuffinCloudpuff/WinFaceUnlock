use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use common_protocol::{
    AuthFailureReason, AuthGrant, AuthSource, ProtectedCredential, ProtectedCredentialMaterial,
    ProtocolError, ServiceEvent, ServiceRequest, SessionId,
};

use crate::GrantRegistry;

pub trait AuthGrantIssuer {
    fn issue_auth_grant(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        issued_at_unix_ms: i64,
    ) -> Result<AuthGrant, AuthFailureReason>;
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

pub trait FaceTemplateConfigApplier {
    fn apply_face_template(&mut self, template_path: &Path) -> Result<(), ProtocolError>;
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

pub struct ServiceRequestHandler<I, R, A, C> {
    grant_issuer: I,
    credential_resolver: R,
    face_template_applier: A,
    clock: C,
    grant_registry: GrantRegistry,
}

impl<I, R, A, C> ServiceRequestHandler<I, R, A, C>
where
    I: AuthGrantIssuer,
    R: ProtectedCredentialResolver + ProtectedCredentialMaterialResolver,
    A: FaceTemplateConfigApplier,
    C: UnixTimeMillisClock,
{
    pub fn new(
        grant_issuer: I,
        credential_resolver: R,
        face_template_applier: A,
        clock: C,
    ) -> Self {
        Self {
            grant_issuer,
            credential_resolver,
            face_template_applier,
            clock,
            grant_registry: GrantRegistry::default(),
        }
    }

    pub fn handle_request(
        &mut self,
        request: ServiceRequest,
    ) -> Result<ServiceEvent, ProtocolError> {
        match request {
            ServiceRequest::WakeAuth { session_id, source } => {
                self.handle_wake_auth(session_id, source)
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
                self.grant_registry.remove_grants_for_session(&session_id);
                Ok(ServiceEvent::AuthCancelled { session_id })
            }
            ServiceRequest::ApplyFaceTemplate { template_path } => {
                self.handle_apply_face_template(template_path)
            }
            ServiceRequest::HealthCheck => Ok(ServiceEvent::HealthOk),
        }
    }

    fn handle_wake_auth(
        &mut self,
        session_id: SessionId,
        source: AuthSource,
    ) -> Result<ServiceEvent, ProtocolError> {
        let issued_at_unix_ms = self.clock.now_unix_ms();
        let grant_result =
            self.grant_issuer
                .issue_auth_grant(&session_id, source, issued_at_unix_ms);

        match grant_result {
            Ok(grant) => {
                self.grant_registry.insert_issued_grant(grant.clone())?;
                Ok(ServiceEvent::AuthSucceeded { grant })
            }
            Err(reason) => Ok(ServiceEvent::AuthFailed { session_id, reason }),
        }
    }

    fn handle_apply_face_template(
        &mut self,
        template_path: PathBuf,
    ) -> Result<ServiceEvent, ProtocolError> {
        if template_path.as_os_str().is_empty() {
            return Err(ProtocolError::InvalidMessage);
        }
        self.face_template_applier
            .apply_face_template(&template_path)?;
        Ok(ServiceEvent::FaceTemplateApplied { template_path })
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
            issued_at_unix_ms: i64,
        ) -> Result<AuthGrant, AuthFailureReason> {
            Ok(AuthGrant {
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
            _issued_at_unix_ms: i64,
        ) -> Result<AuthGrant, AuthFailureReason> {
            Err(AuthFailureReason::NoFaceDetected)
        }
    }

    struct FixedCredentialResolver;
    struct RecordingFaceTemplateApplier {
        applied_template_paths: Vec<PathBuf>,
    }

    impl RecordingFaceTemplateApplier {
        fn new() -> Self {
            Self {
                applied_template_paths: Vec::new(),
            }
        }
    }

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

    impl FaceTemplateConfigApplier for RecordingFaceTemplateApplier {
        fn apply_face_template(&mut self, template_path: &Path) -> Result<(), ProtocolError> {
            self.applied_template_paths
                .push(template_path.to_path_buf());
            Ok(())
        }
    }

    #[test]
    fn handler_issues_grant_then_redeems_protected_credential_once() -> Result<(), ProtocolError> {
        let mut handler = ServiceRequestHandler::new(
            SuccessfulGrantIssuer,
            FixedCredentialResolver,
            RecordingFaceTemplateApplier::new(),
            FixedClock { now_unix_ms: 1_000 },
        );

        let issued = handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::LocalCamera,
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
            RecordingFaceTemplateApplier::new(),
            FixedClock { now_unix_ms: 1_000 },
        );

        handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::LocalCamera,
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
            RecordingFaceTemplateApplier::new(),
            FixedClock { now_unix_ms: 1_000 },
        );

        handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::LocalCamera,
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
            RecordingFaceTemplateApplier::new(),
            FixedClock { now_unix_ms: 1_000 },
        );

        let event = handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::LocalCamera,
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
    fn handler_cancels_session_grants() -> Result<(), ProtocolError> {
        let mut handler = ServiceRequestHandler::new(
            SuccessfulGrantIssuer,
            FixedCredentialResolver,
            RecordingFaceTemplateApplier::new(),
            FixedClock { now_unix_ms: 1_000 },
        );

        handler.handle_request(ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::LocalCamera,
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
    fn handler_applies_face_template_via_config_applier() -> Result<(), ProtocolError> {
        let mut handler = ServiceRequestHandler::new(
            SuccessfulGrantIssuer,
            FixedCredentialResolver,
            RecordingFaceTemplateApplier::new(),
            FixedClock { now_unix_ms: 1_000 },
        );
        let template_path = PathBuf::from(r"C:\ProgramData\WinFaceUnlock\selected_templates.json");

        let event = handler.handle_request(ServiceRequest::ApplyFaceTemplate {
            template_path: template_path.clone(),
        })?;

        assert_eq!(event, ServiceEvent::FaceTemplateApplied { template_path });
        Ok(())
    }
}
