mod credential_blob;
mod error;
mod file_store;
mod key_file;
mod key_protector;
mod master_key;
mod persistence;
mod repository_store;
mod secure_random;
mod user_record;

pub use credential_blob::{
    AesGcmCredentialBlobProtector, CredentialBlob, CredentialBlobAlgorithm,
    CredentialBlobAssociatedData, CredentialBlobProtector, CredentialSecret,
};
pub use error::CredentialStoreError;
pub use file_store::{CredentialStore, FileCredentialStore};
pub use key_file::{MasterKeyHandle, ProtectedMasterKeyFile};
pub use key_protector::KeyProtector;
pub use master_key::{MASTER_KEY_LEN, MasterKey};
pub use persistence::{
    AuditLogRecord, CredentialBlobRecord, DatabaseMigration, DatabaseSchemaVersion,
    FaceTemplateRecord, FaceTemplateRef, LivenessRequirement, PolicyId, PolicyRecord,
    RepositoryTransaction, SqlCipherRepository, SqlCipherSchema, StoreRepository, StoredUserRecord,
    UnixTimeMillis, UserFaceTemplateLinkRecord,
};
pub use repository_store::RepositoryCredentialStore;
pub use secure_random::SecureRandom;
pub use user_record::UserRecord;

#[cfg(windows)]
pub use key_protector::WindowsDpapiKeyProtector;

#[cfg(windows)]
pub use secure_random::WindowsSecureRandom;
