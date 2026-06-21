use common_protocol::{
    AuthGrant, AuthScore, AuthSource, AuthTriggerSource, DEFAULT_GRANT_TTL, GrantId, Nonce,
    SessionId, UserId,
};
use ipc::{AuthGrantIssueResult, AuthGrantIssuer};

pub struct SimulatedAuthGrantIssuer {
    user_id: UserId,
    next_grant_sequence: u64,
}

impl SimulatedAuthGrantIssuer {
    pub fn for_user(user_id: UserId) -> Self {
        Self {
            user_id,
            next_grant_sequence: 1,
        }
    }
}

impl AuthGrantIssuer for SimulatedAuthGrantIssuer {
    fn issue_auth_grant(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        _trigger_source: AuthTriggerSource,
        issued_at_unix_ms: i64,
    ) -> AuthGrantIssueResult {
        let grant_sequence = self.next_grant_sequence;
        self.next_grant_sequence = self.next_grant_sequence.saturating_add(1);

        AuthGrantIssueResult::Issued(AuthGrant {
            grant_id: GrantId(format!("dev-grant-{grant_sequence}")),
            nonce: Nonce(format!("dev-nonce-{grant_sequence}")),
            session_id: session_id.clone(),
            user_id: self.user_id.clone(),
            source,
            score: AuthScore {
                match_score: 1.0,
                liveness_score: None,
            },
            issued_at_unix_ms,
            expires_at_unix_ms: issued_at_unix_ms + DEFAULT_GRANT_TTL.as_millis() as i64,
        })
    }
}

#[cfg(test)]
mod tests {
    use common_protocol::AuthSource;

    use super::*;

    #[test]
    fn simulated_issuer_generates_explicit_short_lived_grant() {
        let mut issuer = SimulatedAuthGrantIssuer::for_user(UserId("user-1".to_owned()));
        let issued_result = issuer.issue_auth_grant(
            &SessionId("session-1".to_owned()),
            AuthSource::ManualTest,
            AuthTriggerSource::InputTriggered,
            1_000,
        );
        assert!(matches!(issued_result, AuthGrantIssueResult::Issued(_)));
        let AuthGrantIssueResult::Issued(grant) = issued_result else {
            return;
        };

        assert_eq!(grant.grant_id, GrantId("dev-grant-1".to_owned()));
        assert_eq!(grant.issued_at_unix_ms, 1_000);
        assert_eq!(
            grant.expires_at_unix_ms,
            1_000 + DEFAULT_GRANT_TTL.as_millis() as i64
        );
    }
}
