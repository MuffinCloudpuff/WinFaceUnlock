use std::time::{SystemTime, UNIX_EPOCH};

use common_protocol::{
    AuthFailureReason, AuthGrant, AuthSource, ProtectedCredential, ProtocolError, ServiceEvent,
    ServiceRequest, SessionId,
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
    R: ProtectedCredentialResolver,
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
            ServiceRequest::Cancel { session_id } => {
                self.grant_registry.remove_grants_for_session(&session_id);
                Ok(ServiceEvent::AuthCancelled { session_id })
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
}

#[cfg(test)]
mod tests {
    use common_protocol::{AuthScore, CredentialRef, DEFAULT_GRANT_TTL, GrantId, Nonce, UserId};

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
    fn handler_returns_auth_failed_event_for_failed_auth_attempt() -> Result<(), ProtocolError> {
        let mut handler = ServiceRequestHandler::new(
            FailingGrantIssuer,
            FixedCredentialResolver,
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
}
