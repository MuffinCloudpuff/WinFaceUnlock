#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CredentialStoreError {
    StoreUnavailable,
    IoFailed,
    RandomGenerationFailed,
    InvalidProtectedKeyFile,
    InvalidCredentialBlob,
    KeyGenerationFailed,
    KeyProtectionFailed,
    KeyUnprotectFailed,
    CredentialBlobEncryptionFailed,
    CredentialBlobDecryptionFailed,
    SchemaMigrationFailed,
    RecordValidationFailed,
    RepositoryUnavailable,
    HardwareMismatch,
    UserNotFound,
    CredentialNotFound,
}

impl From<std::io::Error> for CredentialStoreError {
    fn from(_: std::io::Error) -> Self {
        Self::IoFailed
    }
}

impl From<rusqlite::Error> for CredentialStoreError {
    fn from(_: rusqlite::Error) -> Self {
        Self::RepositoryUnavailable
    }
}
