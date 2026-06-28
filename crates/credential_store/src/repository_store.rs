use common_protocol::{CredentialRef, ProtectedCredential, UserId};
use hardware_binding::HardwareFingerprint;

use crate::{
    CredentialBlob, CredentialBlobRecord, CredentialStore, CredentialStoreError,
    FaceTemplateRecord, FaceTemplateRef, LivenessRequirement, PolicyId, PolicyRecord,
    StoreRepository, StoredUserRecord, UnixTimeMillis, UserFaceTemplateLinkRecord, UserRecord,
};

const DEFAULT_POLICY_ID: &str = "default";
const DEFAULT_FACE_MATCH_THRESHOLD: f32 = 0.75;

pub struct RepositoryCredentialStore<R: StoreRepository> {
    repository: R,
}

impl<R: StoreRepository> RepositoryCredentialStore<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub fn save_face_template_record(
        &mut self,
        face_template_record: FaceTemplateRecord,
    ) -> Result<(), CredentialStoreError> {
        self.repository
            .save_face_template_record(face_template_record)
    }

    pub fn load_face_template_record(
        &self,
        face_template_ref: &FaceTemplateRef,
    ) -> Result<FaceTemplateRecord, CredentialStoreError> {
        self.repository.load_face_template_record(face_template_ref)
    }

    pub fn link_face_template_to_user(
        &mut self,
        user_id: common_protocol::UserId,
        face_template_ref: FaceTemplateRef,
    ) -> Result<(), CredentialStoreError> {
        self.repository
            .link_face_template_to_user(UserFaceTemplateLinkRecord {
                user_id,
                face_template_ref,
                linked_at: UnixTimeMillis(0),
            })
    }

    pub fn list_face_template_refs_for_user(
        &self,
        user_id: &common_protocol::UserId,
    ) -> Result<Vec<FaceTemplateRef>, CredentialStoreError> {
        self.repository.list_face_template_refs_for_user(user_id)
    }
}

impl<R: StoreRepository> CredentialStore for RepositoryCredentialStore<R> {
    fn initialize(
        &mut self,
        _fingerprint: &HardwareFingerprint,
    ) -> Result<(), CredentialStoreError> {
        self.repository.save_policy_record(PolicyRecord {
            policy_id: PolicyId(DEFAULT_POLICY_ID.to_owned()),
            liveness_requirement: LivenessRequirement::NotRequired,
            face_match_threshold: DEFAULT_FACE_MATCH_THRESHOLD,
            failure_limit_before_cooldown: 3,
            cooldown_duration_seconds: 30,
        })
    }

    fn upsert_user(&mut self, record: UserRecord) -> Result<(), CredentialStoreError> {
        let now = UnixTimeMillis(0);
        self.repository.save_user_record(StoredUserRecord {
            user_id: record.user_id,
            user_sid: record.user_sid,
            username: record.username,
            account_type: record.account_type,
            credential_ref: record.credential_ref,
            policy_id: PolicyId(DEFAULT_POLICY_ID.to_owned()),
            created_at: now,
            updated_at: now,
        })
    }

    fn get_user(&self, user_id: &UserId) -> Result<UserRecord, CredentialStoreError> {
        let stored_user = self.repository.load_user_record(user_id)?;
        Ok(UserRecord {
            user_id: stored_user.user_id,
            user_sid: stored_user.user_sid,
            username: stored_user.username,
            account_type: stored_user.account_type,
            credential_ref: stored_user.credential_ref,
        })
    }

    fn get_protected_credential(
        &self,
        user_id: &UserId,
    ) -> Result<ProtectedCredential, CredentialStoreError> {
        let user = self.get_user(user_id)?;
        Ok(ProtectedCredential {
            user_id: user.user_id,
            credential_ref: user.credential_ref,
        })
    }

    fn save_encrypted_credential_blob(
        &mut self,
        credential_ref: CredentialRef,
        credential_blob: CredentialBlob,
    ) -> Result<(), CredentialStoreError> {
        let now = UnixTimeMillis(0);
        self.repository
            .save_credential_blob_record(CredentialBlobRecord {
                credential_ref,
                encrypted_blob_bytes: credential_blob.serialize()?,
                key_version: 1,
                created_at: now,
                updated_at: now,
            })
    }

    fn load_encrypted_credential_blob(
        &self,
        credential_ref: &CredentialRef,
    ) -> Result<CredentialBlob, CredentialStoreError> {
        let record = self
            .repository
            .load_credential_blob_record(credential_ref)?;
        CredentialBlob::deserialize(&record.encrypted_blob_bytes)
    }
}

#[cfg(test)]
mod tests {
    use common_protocol::{AccountType, CredentialRef, UserId};
    use hardware_binding::HardwareFingerprint;

    use crate::{
        CredentialBlob, CredentialBlobAlgorithm, CredentialStore, CredentialStoreError,
        FaceTemplateRecord, FaceTemplateRef, MASTER_KEY_LEN, MasterKey, RepositoryCredentialStore,
        SqlCipherRepository, UserRecord,
    };

    #[test]
    fn repository_store_returns_protected_credential_reference() -> Result<(), CredentialStoreError>
    {
        let database_path = unique_temp_path("repo-store.db")?;
        let master_key = MasterKey::from_bytes([11_u8; MASTER_KEY_LEN]);
        let repository = SqlCipherRepository::open(&database_path, &master_key)?;
        let mut store = RepositoryCredentialStore::new(repository);

        store.initialize(&HardwareFingerprint::empty())?;
        store.save_encrypted_credential_blob(
            CredentialRef("cred-1".to_owned()),
            test_credential_blob(),
        )?;
        store.upsert_user(test_user("user-1", "cred-1"))?;
        let protected_credential = store.get_protected_credential(&UserId("user-1".to_owned()))?;
        let _ = std::fs::remove_file(database_path);

        assert_eq!(
            protected_credential.credential_ref,
            CredentialRef("cred-1".to_owned())
        );
        Ok(())
    }

    #[test]
    fn repository_store_round_trips_serialized_credential_blob() -> Result<(), CredentialStoreError>
    {
        let database_path = unique_temp_path("repo-store-blob.db")?;
        let master_key = MasterKey::from_bytes([12_u8; MASTER_KEY_LEN]);
        let repository = SqlCipherRepository::open(&database_path, &master_key)?;
        let mut store = RepositoryCredentialStore::new(repository);
        let credential_ref = CredentialRef("cred-1".to_owned());
        let credential_blob = CredentialBlob::new(
            CredentialBlobAlgorithm::Aes256GcmV1,
            vec![1; 12],
            vec![2; 16],
        );

        store.save_encrypted_credential_blob(credential_ref.clone(), credential_blob.clone())?;
        let loaded_blob = store.load_encrypted_credential_blob(&credential_ref)?;
        let _ = std::fs::remove_file(database_path);

        assert_eq!(loaded_blob, credential_blob);
        Ok(())
    }

    #[test]
    fn repository_store_round_trips_face_template_records() -> Result<(), CredentialStoreError> {
        let database_path = unique_temp_path("repo-store-face.db")?;
        let master_key = MasterKey::from_bytes([13_u8; MASTER_KEY_LEN]);
        let repository = SqlCipherRepository::open(&database_path, &master_key)?;
        let mut store = RepositoryCredentialStore::new(repository);
        let face_template_ref = FaceTemplateRef("face-template-1".to_owned());
        let user_id = UserId("user-1".to_owned());

        store.initialize(&HardwareFingerprint::empty())?;
        store.save_encrypted_credential_blob(
            CredentialRef("cred-1".to_owned()),
            test_credential_blob(),
        )?;
        store.upsert_user(test_user("user-1", "cred-1"))?;
        store.save_face_template_record(FaceTemplateRecord {
            face_template_ref: face_template_ref.clone(),
            enrolled_user_id: user_id.clone(),
            model_family: "sface".to_owned(),
            model_version: "2021dec".to_owned(),
            encrypted_template_bytes: vec![1, 2, 3],
            created_at: crate::UnixTimeMillis(0),
            updated_at: crate::UnixTimeMillis(0),
        })?;
        store.link_face_template_to_user(user_id.clone(), face_template_ref.clone())?;

        let loaded = store.load_face_template_record(&face_template_ref)?;
        let linked = store.list_face_template_refs_for_user(&user_id)?;
        let _ = std::fs::remove_file(database_path);

        assert_eq!(loaded.face_template_ref, face_template_ref);
        assert_eq!(loaded.encrypted_template_bytes, vec![1, 2, 3]);
        assert_eq!(linked, vec![FaceTemplateRef("face-template-1".to_owned())]);
        Ok(())
    }

    fn test_user(user_id: &str, credential_ref: &str) -> UserRecord {
        UserRecord {
            user_id: UserId(user_id.to_owned()),
            user_sid: "S-1-5-21-example".to_owned(),
            username: "Liu".to_owned(),
            account_type: AccountType::Local,
            credential_ref: CredentialRef(credential_ref.to_owned()),
        }
    }

    fn test_credential_blob() -> CredentialBlob {
        CredentialBlob::new(
            CredentialBlobAlgorithm::Aes256GcmV1,
            vec![1; 12],
            vec![2; 16],
        )
    }

    fn unique_temp_path(name: &str) -> Result<std::path::PathBuf, CredentialStoreError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| CredentialStoreError::IoFailed)?
            .as_nanos();
        Ok(std::env::temp_dir().join(format!(
            "winfaceunlock-repository-store-{}-{}-{name}",
            std::process::id(),
            nanos
        )))
    }
}
