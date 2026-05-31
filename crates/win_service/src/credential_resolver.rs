use common_protocol::{AuthGrant, ProtectedCredential, ProtocolError};
use credential_store::{CredentialStore, CredentialStoreError};
use ipc::ProtectedCredentialResolver;

pub struct StoreProtectedCredentialResolver<S: CredentialStore> {
    credential_store: S,
}

impl<S: CredentialStore> StoreProtectedCredentialResolver<S> {
    pub fn new(credential_store: S) -> Self {
        Self { credential_store }
    }
}

impl<S: CredentialStore> ProtectedCredentialResolver for StoreProtectedCredentialResolver<S> {
    fn resolve_protected_credential(
        &mut self,
        grant: &AuthGrant,
    ) -> Result<ProtectedCredential, ProtocolError> {
        self.credential_store
            .get_protected_credential(&grant.user_id)
            .map_err(credential_store_error_to_protocol_error)
    }
}

fn credential_store_error_to_protocol_error(error: CredentialStoreError) -> ProtocolError {
    match error {
        CredentialStoreError::UserNotFound | CredentialStoreError::CredentialNotFound => {
            ProtocolError::Unauthorized
        }
        CredentialStoreError::StoreUnavailable
        | CredentialStoreError::RepositoryUnavailable
        | CredentialStoreError::IoFailed => ProtocolError::TransportUnavailable,
        _ => ProtocolError::InvalidMessage,
    }
}

#[cfg(test)]
mod tests {
    use common_protocol::{
        AuthGrant, AuthScore, AuthSource, CredentialRef, GrantId, Nonce, SessionId, UserId,
    };
    use credential_store::{
        CredentialStore, FileCredentialStore, ProtectedMasterKeyFile, UserRecord,
        WindowsDpapiKeyProtector,
    };
    use hardware_binding::HardwareFingerprint;

    use super::*;

    #[test]
    fn store_resolver_returns_credential_ref_from_store() -> Result<(), CredentialStoreError> {
        let mut store = FileCredentialStore::new(
            ProtectedMasterKeyFile::new(unique_temp_path("resolver-master-key.bin")?),
            WindowsDpapiKeyProtector::new(),
        );
        store.initialize(&HardwareFingerprint::empty())?;
        store.upsert_user(UserRecord {
            user_id: UserId("user-1".to_owned()),
            user_sid: "S-1-5-21-example".to_owned(),
            username: "Liu".to_owned(),
            account_type: common_protocol::AccountType::Local,
            credential_ref: CredentialRef("cred-from-store".to_owned()),
        })?;
        let mut resolver = StoreProtectedCredentialResolver::new(store);

        let protected_credential = resolver
            .resolve_protected_credential(&test_grant("user-1"))
            .map_err(|_| CredentialStoreError::StoreUnavailable)?;

        assert_eq!(
            protected_credential.credential_ref,
            CredentialRef("cred-from-store".to_owned())
        );
        Ok(())
    }

    #[test]
    fn store_resolver_rejects_missing_user() -> Result<(), CredentialStoreError> {
        let mut store = FileCredentialStore::new(
            ProtectedMasterKeyFile::new(unique_temp_path("missing-user-master-key.bin")?),
            WindowsDpapiKeyProtector::new(),
        );
        store.initialize(&HardwareFingerprint::empty())?;
        let mut resolver = StoreProtectedCredentialResolver::new(store);

        let result = resolver.resolve_protected_credential(&test_grant("missing-user"));

        assert_eq!(result, Err(ProtocolError::Unauthorized));
        Ok(())
    }

    fn test_grant(user_id: &str) -> AuthGrant {
        AuthGrant {
            grant_id: GrantId("grant-1".to_owned()),
            nonce: Nonce("nonce-1".to_owned()),
            session_id: SessionId("session-1".to_owned()),
            user_id: UserId(user_id.to_owned()),
            source: AuthSource::ManualTest,
            score: AuthScore {
                match_score: 1.0,
                liveness_score: None,
            },
            issued_at_unix_ms: 1_000,
            expires_at_unix_ms: 6_000,
        }
    }

    fn unique_temp_path(name: &str) -> Result<std::path::PathBuf, CredentialStoreError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| CredentialStoreError::IoFailed)?
            .as_nanos();
        Ok(std::env::temp_dir().join(format!(
            "winfaceunlock-service-{}-{}-{name}",
            std::process::id(),
            nanos
        )))
    }
}
