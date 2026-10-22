use std::collections::HashMap;

use common_protocol::{AuthGrant, GrantId, Nonce, ProtocolError, SessionId};

#[derive(Clone, Debug, Eq, PartialEq)]
enum GrantRedemptionState {
    Available,
    Redeemed,
    Revoked,
}

#[derive(Clone, Debug, PartialEq)]
struct TrackedGrant {
    grant: AuthGrant,
    redemption_state: GrantRedemptionState,
}

#[derive(Default)]
pub struct GrantRegistry {
    grants_by_id: HashMap<GrantId, TrackedGrant>,
}

impl GrantRegistry {
    pub fn insert_issued_grant(
        &mut self,
        grant: AuthGrant,
        current_time_unix_ms: i64,
    ) -> Result<(), ProtocolError> {
        self.remove_expired_grants(current_time_unix_ms);
        if !grant.has_valid_time_window() {
            return Err(ProtocolError::InvalidMessage);
        }

        if grant.is_expired_at(current_time_unix_ms) {
            return Err(ProtocolError::ExpiredGrant);
        }
        if grant.issued_at_unix_ms > current_time_unix_ms {
            return Err(ProtocolError::InvalidMessage);
        }
        if let Some(existing) = self.grants_by_id.get(&grant.grant_id) {
            if existing.grant != grant {
                return Err(ProtocolError::InvalidMessage);
            }
            return match existing.redemption_state {
                GrantRedemptionState::Available => Ok(()),
                GrantRedemptionState::Redeemed => Err(ProtocolError::UsedGrant),
                GrantRedemptionState::Revoked => Err(ProtocolError::Unauthorized),
            };
        }

        self.grants_by_id.insert(
            grant.grant_id.clone(),
            TrackedGrant {
                grant,
                redemption_state: GrantRedemptionState::Available,
            },
        );
        Ok(())
    }

    pub fn redeem_grant_for_session(
        &mut self,
        grant_id: &GrantId,
        nonce: &Nonce,
        session_id: &SessionId,
        current_time_unix_ms: i64,
    ) -> Result<AuthGrant, ProtocolError> {
        let tracked_grant = self
            .grants_by_id
            .get_mut(grant_id)
            .ok_or(ProtocolError::InvalidMessage)?;

        if tracked_grant.grant.nonce != *nonce {
            return Err(ProtocolError::Unauthorized);
        }

        if tracked_grant.grant.session_id != *session_id {
            return Err(ProtocolError::SessionMismatch);
        }

        if tracked_grant.grant.is_expired_at(current_time_unix_ms) {
            return Err(ProtocolError::ExpiredGrant);
        }

        if tracked_grant.redemption_state == GrantRedemptionState::Redeemed {
            return Err(ProtocolError::UsedGrant);
        }
        if tracked_grant.redemption_state == GrantRedemptionState::Revoked {
            return Err(ProtocolError::Unauthorized);
        }

        tracked_grant.redemption_state = GrantRedemptionState::Redeemed;
        Ok(tracked_grant.grant.clone())
    }

    pub fn remove_expired_grants(&mut self, current_time_unix_ms: i64) {
        self.grants_by_id
            .retain(|_, tracked_grant| !tracked_grant.grant.is_expired_at(current_time_unix_ms));
    }

    /// Keep tombstones until expiry so a cached issuer result cannot resurrect
    /// an authorization invalidated by configuration changes or cancellation.
    pub fn revoke_all_grants(&mut self) {
        for tracked in self.grants_by_id.values_mut() {
            tracked.redemption_state = GrantRedemptionState::Revoked;
        }
    }

    pub fn revoke_grants_for_session(&mut self, session_id: &SessionId) {
        for tracked in self.grants_by_id.values_mut() {
            if tracked.grant.session_id == *session_id {
                tracked.redemption_state = GrantRedemptionState::Revoked;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use common_protocol::{AuthScore, AuthSource, GrantId, Nonce, SessionId, UserId};

    use super::*;

    #[test]
    fn grant_registry_redeems_matching_grant_once() -> Result<(), ProtocolError> {
        let mut registry = GrantRegistry::default();
        let grant = test_grant("grant-1", "nonce-1", 1_000, 6_000);

        registry.insert_issued_grant(grant.clone(), 1_000)?;
        let redeemed = registry.redeem_grant_for_session(
            &GrantId("grant-1".to_owned()),
            &Nonce("nonce-1".to_owned()),
            &SessionId("session-1".to_owned()),
            2_000,
        )?;
        let second_redeem = registry.redeem_grant_for_session(
            &GrantId("grant-1".to_owned()),
            &Nonce("nonce-1".to_owned()),
            &SessionId("session-1".to_owned()),
            2_001,
        );

        assert_eq!(redeemed, grant);
        assert_eq!(second_redeem, Err(ProtocolError::UsedGrant));
        Ok(())
    }

    #[test]
    fn grant_registry_rejects_wrong_nonce() -> Result<(), ProtocolError> {
        let mut registry = GrantRegistry::default();

        registry.insert_issued_grant(test_grant("grant-1", "nonce-1", 1_000, 6_000), 1_000)?;
        let result = registry.redeem_grant_for_session(
            &GrantId("grant-1".to_owned()),
            &Nonce("wrong-nonce".to_owned()),
            &SessionId("session-1".to_owned()),
            2_000,
        );

        assert_eq!(result, Err(ProtocolError::Unauthorized));
        Ok(())
    }

    #[test]
    fn grant_registry_rejects_expired_grant() -> Result<(), ProtocolError> {
        let mut registry = GrantRegistry::default();

        registry.insert_issued_grant(test_grant("grant-1", "nonce-1", 1_000, 6_000), 1_000)?;
        let result = registry.redeem_grant_for_session(
            &GrantId("grant-1".to_owned()),
            &Nonce("nonce-1".to_owned()),
            &SessionId("session-1".to_owned()),
            6_000,
        );

        assert_eq!(result, Err(ProtocolError::ExpiredGrant));
        Ok(())
    }

    #[test]
    fn grant_registry_rejects_invalid_time_window() {
        let mut registry = GrantRegistry::default();
        let result =
            registry.insert_issued_grant(test_grant("grant-1", "nonce-1", 6_000, 6_000), 6_000);

        assert_eq!(result, Err(ProtocolError::InvalidMessage));
    }

    #[test]
    fn grant_registry_rejects_wrong_session() -> Result<(), ProtocolError> {
        let mut registry = GrantRegistry::default();

        registry.insert_issued_grant(test_grant("grant-1", "nonce-1", 1_000, 6_000), 1_000)?;
        let result = registry.redeem_grant_for_session(
            &GrantId("grant-1".to_owned()),
            &Nonce("nonce-1".to_owned()),
            &SessionId("other-session".to_owned()),
            2_000,
        );

        assert_eq!(result, Err(ProtocolError::SessionMismatch));
        Ok(())
    }

    #[test]
    fn grant_registry_revokes_session_grants() -> Result<(), ProtocolError> {
        let mut registry = GrantRegistry::default();

        registry.insert_issued_grant(test_grant("grant-1", "nonce-1", 1_000, 6_000), 1_000)?;
        registry.revoke_grants_for_session(&SessionId("session-1".to_owned()));
        let result = registry.redeem_grant_for_session(
            &GrantId("grant-1".to_owned()),
            &Nonce("nonce-1".to_owned()),
            &SessionId("session-1".to_owned()),
            2_000,
        );

        assert_eq!(result, Err(ProtocolError::Unauthorized));
        Ok(())
    }

    #[test]
    fn repeated_issuer_result_cannot_reset_redeemed_grant() -> Result<(), ProtocolError> {
        let mut registry = GrantRegistry::default();
        let grant = test_grant("grant-1", "nonce-1", 1_000, 6_000);
        registry.insert_issued_grant(grant.clone(), 1_000)?;
        registry.redeem_grant_for_session(
            &grant.grant_id,
            &grant.nonce,
            &grant.session_id,
            2_000,
        )?;
        assert_eq!(
            registry.insert_issued_grant(grant.clone(), 2_001),
            Err(ProtocolError::UsedGrant)
        );
        assert_eq!(
            registry.redeem_grant_for_session(
                &grant.grant_id,
                &grant.nonce,
                &grant.session_id,
                2_002
            ),
            Err(ProtocolError::UsedGrant)
        );
        Ok(())
    }

    #[test]
    fn revoked_tombstone_blocks_reinsertion_and_is_pruned_at_expiry() -> Result<(), ProtocolError> {
        let mut registry = GrantRegistry::default();
        let grant = test_grant("grant-1", "nonce-1", 1_000, 6_000);
        registry.insert_issued_grant(grant.clone(), 1_000)?;
        registry.revoke_all_grants();
        assert_eq!(
            registry.insert_issued_grant(grant.clone(), 2_000),
            Err(ProtocolError::Unauthorized)
        );
        assert_eq!(registry.grants_by_id.len(), 1);
        registry.remove_expired_grants(6_000);
        assert!(registry.grants_by_id.is_empty());
        assert_eq!(
            registry.insert_issued_grant(grant, 6_001),
            Err(ProtocolError::ExpiredGrant)
        );
        assert!(registry.grants_by_id.is_empty());
        Ok(())
    }

    #[test]
    fn same_id_cannot_replace_grant_body() -> Result<(), ProtocolError> {
        let mut registry = GrantRegistry::default();
        let original = test_grant("grant-1", "nonce-1", 1_000, 6_000);
        registry.insert_issued_grant(original.clone(), 1_000)?;
        let mut changed = original.clone();
        changed.nonce = Nonce("replacement".to_owned());
        assert_eq!(
            registry.insert_issued_grant(changed, 2_000),
            Err(ProtocolError::InvalidMessage)
        );
        assert_eq!(
            registry.redeem_grant_for_session(
                &original.grant_id,
                &original.nonce,
                &original.session_id,
                2_001
            )?,
            original
        );
        Ok(())
    }

    fn test_grant(
        grant_id: &str,
        nonce: &str,
        issued_at_unix_ms: i64,
        expires_at_unix_ms: i64,
    ) -> AuthGrant {
        AuthGrant {
            grant_id: GrantId(grant_id.to_owned()),
            nonce: Nonce(nonce.to_owned()),
            session_id: SessionId("session-1".to_owned()),
            user_id: UserId("user-1".to_owned()),
            source: AuthSource::LocalCamera,
            score: AuthScore {
                match_score: 0.82,
                liveness_score: None,
            },
            issued_at_unix_ms,
            expires_at_unix_ms,
        }
    }
}
