use common_protocol::{CredentialRef, UserId};

use crate::{
    CredentialBlobRecord, CredentialStoreError, FaceTemplateRecord, FaceTemplateRef, PolicyId,
    PolicyRecord,
    persistence::records::{AuditLogRecord, StoredUserRecord, UserFaceTemplateLinkRecord},
};

pub trait RepositoryTransaction {
    fn commit_transaction(self) -> Result<(), CredentialStoreError>;
    fn rollback_transaction(self) -> Result<(), CredentialStoreError>;
}

pub trait StoreRepository {
    type Transaction<'repo>: RepositoryTransaction
    where
        Self: 'repo;

    fn begin_write_transaction(&mut self) -> Result<Self::Transaction<'_>, CredentialStoreError>;

    fn save_user_record(
        &mut self,
        user_record: StoredUserRecord,
    ) -> Result<(), CredentialStoreError>;

    fn load_user_record(&self, user_id: &UserId) -> Result<StoredUserRecord, CredentialStoreError>;

    fn save_credential_blob_record(
        &mut self,
        credential_blob_record: CredentialBlobRecord,
    ) -> Result<(), CredentialStoreError>;

    fn load_credential_blob_record(
        &self,
        credential_ref: &CredentialRef,
    ) -> Result<CredentialBlobRecord, CredentialStoreError>;

    fn save_face_template_record(
        &mut self,
        face_template_record: FaceTemplateRecord,
    ) -> Result<(), CredentialStoreError>;

    fn load_face_template_record(
        &self,
        face_template_ref: &FaceTemplateRef,
    ) -> Result<FaceTemplateRecord, CredentialStoreError>;

    fn link_face_template_to_user(
        &mut self,
        link_record: UserFaceTemplateLinkRecord,
    ) -> Result<(), CredentialStoreError>;

    fn list_face_template_refs_for_user(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<FaceTemplateRef>, CredentialStoreError>;

    fn save_policy_record(
        &mut self,
        policy_record: PolicyRecord,
    ) -> Result<(), CredentialStoreError>;

    fn load_policy_record(
        &self,
        policy_id: &PolicyId,
    ) -> Result<PolicyRecord, CredentialStoreError>;

    fn append_audit_log_record(
        &mut self,
        audit_log_record: AuditLogRecord,
    ) -> Result<(), CredentialStoreError>;
}
