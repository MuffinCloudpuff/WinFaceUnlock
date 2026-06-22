use common_protocol::{
    AccountType, AuthGrant, ProtectedCredential, ProtectedCredentialMaterial, ProtocolError,
};
use credential_store::{
    AesGcmCredentialBlobProtector, CredentialBlobAssociatedData, CredentialBlobProtector,
    CredentialStore, CredentialStoreError, MasterKey, WindowsSecureRandom,
};
use ipc::{
    CredentialMaterialProtector, CredentialMaterialSecret,
    DpapiLocalMachineCredentialMaterialProtector, ProtectedCredentialMaterialResolver,
    ProtectedCredentialResolver,
};

const DEVELOPMENT_PLACEHOLDER_USERNAME: &str = "dev-user";
const DEVELOPMENT_PLACEHOLDER_CREDENTIAL_REF: &str = "dev-credential-ref";

pub struct StoreProtectedCredentialResolver<S: CredentialStore> {
    credential_store: S,
    master_key: Option<MasterKey>,
    credential_material_protector: DpapiLocalMachineCredentialMaterialProtector,
}

impl<S: CredentialStore> StoreProtectedCredentialResolver<S> {
    #[cfg(test)]
    pub fn new(credential_store: S) -> Self {
        Self {
            credential_store,
            master_key: None,
            credential_material_protector: DpapiLocalMachineCredentialMaterialProtector,
        }
    }

    pub fn with_master_key(credential_store: S, master_key: MasterKey) -> Self {
        Self {
            credential_store,
            master_key: Some(master_key),
            credential_material_protector: DpapiLocalMachineCredentialMaterialProtector,
        }
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

impl<S: CredentialStore> ProtectedCredentialMaterialResolver
    for StoreProtectedCredentialResolver<S>
{
    fn resolve_protected_credential_material(
        &mut self,
        grant: &AuthGrant,
    ) -> Result<ProtectedCredentialMaterial, ProtocolError> {
        let user = self
            .credential_store
            .get_user(&grant.user_id)
            .map_err(credential_store_error_to_protocol_error)?;
        reject_development_placeholder_credential(&user.username, &user.credential_ref.0)?;
        let credential_blob = self
            .credential_store
            .load_encrypted_credential_blob(&user.credential_ref)
            .map_err(credential_store_error_to_protocol_error)?;
        let master_key = self
            .master_key
            .as_ref()
            .ok_or(ProtocolError::TransportUnavailable)?;
        let blob_protector = AesGcmCredentialBlobProtector::new(WindowsSecureRandom::new());
        let secret = blob_protector
            .decrypt_credential_secret(
                master_key,
                &CredentialBlobAssociatedData {
                    user_id: user.user_id.clone(),
                    credential_ref: user.credential_ref,
                },
                &credential_blob,
            )
            .map_err(credential_store_error_to_protocol_error)?;

        self.credential_material_protector
            .protect_credential_material(CredentialMaterialSecret {
                user_id: user.user_id,
                domain: credential_domain(&user.account_type, &user.username),
                username: credential_username(&user.account_type, &user.username),
                password: secret.expose_for_encryption().to_vec(),
            })
    }
}

fn reject_development_placeholder_credential(
    username: &str,
    credential_ref: &str,
) -> Result<(), ProtocolError> {
    if username == DEVELOPMENT_PLACEHOLDER_USERNAME
        && credential_ref == DEVELOPMENT_PLACEHOLDER_CREDENTIAL_REF
    {
        return Err(ProtocolError::Unauthorized);
    }
    Ok(())
}

fn credential_domain(account_type: &AccountType, username: &str) -> String {
    match account_type {
        AccountType::Local => std::env::var("COMPUTERNAME").unwrap_or_else(|_| ".".to_owned()),
        AccountType::MicrosoftAccount => String::new(),
        AccountType::Domain => username
            .split_once('\\')
            .map(|(domain, _)| domain.to_owned())
            .unwrap_or_default(),
    }
}

fn credential_username(account_type: &AccountType, username: &str) -> String {
    match account_type {
        AccountType::Domain => username
            .split_once('\\')
            .map(|(_, username)| username.to_owned())
            .unwrap_or_else(|| username.to_owned()),
        AccountType::Local | AccountType::MicrosoftAccount => username.to_owned(),
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
        CredentialStore, FileCredentialStore, KeyProtector, ProtectedMasterKeyFile, UserRecord,
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

    #[test]
    fn local_account_domain_uses_machine_name_when_available() {
        let domain = credential_domain(&AccountType::Local, "Leo16");

        assert!(!domain.is_empty());
    }

    #[test]
    fn domain_account_splits_domain_and_username() {
        assert_eq!(
            credential_domain(&AccountType::Domain, r"WORKSTATION\Leo16"),
            "WORKSTATION"
        );
        assert_eq!(
            credential_username(&AccountType::Domain, r"WORKSTATION\Leo16"),
            "Leo16"
        );
    }

    #[test]
    fn resolver_rejects_development_placeholder_credential_material()
    -> Result<(), CredentialStoreError> {
        let mut store = FileCredentialStore::new(
            ProtectedMasterKeyFile::new(unique_temp_path("dev-placeholder-master-key.bin")?),
            WindowsDpapiKeyProtector::new(),
        );
        store.initialize(&HardwareFingerprint::empty())?;
        store.upsert_user(UserRecord {
            user_id: UserId("dev-user".to_owned()),
            user_sid: "S-1-5-21-example".to_owned(),
            username: "dev-user".to_owned(),
            account_type: common_protocol::AccountType::Local,
            credential_ref: CredentialRef("dev-credential-ref".to_owned()),
        })?;
        let master_key = WindowsDpapiKeyProtector::new()
            .generate_master_key()
            .map_err(|_| CredentialStoreError::StoreUnavailable)?;
        let mut resolver = StoreProtectedCredentialResolver::with_master_key(store, master_key);

        let result = resolver.resolve_protected_credential_material(&test_grant("dev-user"));

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
