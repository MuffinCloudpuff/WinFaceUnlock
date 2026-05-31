use std::path::Path;

use common_protocol::{AccountType, CredentialRef, UserId};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    AuditLogRecord, CredentialBlobRecord, CredentialStoreError, FaceTemplateRecord,
    FaceTemplateRef, LivenessRequirement, MasterKey, PolicyId, PolicyRecord, RepositoryTransaction,
    SqlCipherSchema, StoreRepository, UnixTimeMillis, UserFaceTemplateLinkRecord,
    persistence::records::{AuditEventKind, AuditEventOutcome, StoredUserRecord},
};

pub struct SqlCipherRepository {
    connection: Connection,
}

impl SqlCipherRepository {
    pub fn open(
        database_path: &Path,
        master_key: &MasterKey,
    ) -> Result<Self, CredentialStoreError> {
        let connection = Connection::open(database_path)?;
        let repository = Self { connection };
        repository.configure_encrypted_connection(master_key)?;
        repository.apply_migrations()?;
        Ok(repository)
    }

    fn configure_encrypted_connection(
        &self,
        master_key: &MasterKey,
    ) -> Result<(), CredentialStoreError> {
        let raw_key_hex = encode_sqlcipher_raw_key(master_key.expose_for_cryptographic_use());
        self.connection
            .pragma_update(None, "key", format!("x'{raw_key_hex}'"))?;
        self.connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(())
    }

    fn apply_migrations(&self) -> Result<(), CredentialStoreError> {
        let transaction = self.connection.unchecked_transaction()?;
        for statement in SqlCipherSchema::all_statements() {
            transaction
                .execute_batch(statement)
                .map_err(|_| CredentialStoreError::SchemaMigrationFailed)?;
        }
        transaction
            .execute(
                "INSERT OR REPLACE INTO schema_metadata (schema_key, schema_value) VALUES ('current_version', ?1)",
                params![SqlCipherSchema::CURRENT_VERSION.0.to_string()],
            )
            .map_err(|_| CredentialStoreError::SchemaMigrationFailed)?;
        transaction
            .commit()
            .map_err(|_| CredentialStoreError::SchemaMigrationFailed)?;
        Ok(())
    }
}

pub struct SqlCipherRepositoryTransaction<'repo> {
    transaction: rusqlite::Transaction<'repo>,
}

impl RepositoryTransaction for SqlCipherRepositoryTransaction<'_> {
    fn commit_transaction(self) -> Result<(), CredentialStoreError> {
        self.transaction.commit()?;
        Ok(())
    }

    fn rollback_transaction(self) -> Result<(), CredentialStoreError> {
        self.transaction.rollback()?;
        Ok(())
    }
}

impl StoreRepository for SqlCipherRepository {
    type Transaction<'repo>
        = SqlCipherRepositoryTransaction<'repo>
    where
        Self: 'repo;

    fn begin_write_transaction(&mut self) -> Result<Self::Transaction<'_>, CredentialStoreError> {
        Ok(SqlCipherRepositoryTransaction {
            transaction: self.connection.transaction()?,
        })
    }

    fn save_user_record(
        &mut self,
        user_record: StoredUserRecord,
    ) -> Result<(), CredentialStoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO users (
                user_id, user_sid, username, account_type, credential_ref, policy_id,
                created_at_unix_ms, updated_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                user_record.user_id.0,
                user_record.user_sid,
                user_record.username,
                account_type_to_storage(user_record.account_type),
                user_record.credential_ref.0,
                user_record.policy_id.0,
                user_record.created_at.0,
                user_record.updated_at.0,
            ],
        )?;
        Ok(())
    }

    fn load_user_record(&self, user_id: &UserId) -> Result<StoredUserRecord, CredentialStoreError> {
        self.connection
            .query_row(
                "SELECT user_id, user_sid, username, account_type, credential_ref, policy_id,
                    created_at_unix_ms, updated_at_unix_ms
                 FROM users WHERE user_id = ?1",
                params![user_id.0],
                |row| {
                    Ok(StoredUserRecord {
                        user_id: UserId(row.get(0)?),
                        user_sid: row.get(1)?,
                        username: row.get(2)?,
                        account_type: account_type_from_storage(row.get::<_, String>(3)?)?,
                        credential_ref: CredentialRef(row.get(4)?),
                        policy_id: PolicyId(row.get(5)?),
                        created_at: UnixTimeMillis(row.get(6)?),
                        updated_at: UnixTimeMillis(row.get(7)?),
                    })
                },
            )
            .optional()?
            .ok_or(CredentialStoreError::UserNotFound)
    }

    fn save_credential_blob_record(
        &mut self,
        credential_blob_record: CredentialBlobRecord,
    ) -> Result<(), CredentialStoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO credentials (
                credential_ref, encrypted_blob_bytes, key_version, created_at_unix_ms, updated_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                credential_blob_record.credential_ref.0,
                credential_blob_record.encrypted_blob_bytes,
                credential_blob_record.key_version,
                credential_blob_record.created_at.0,
                credential_blob_record.updated_at.0,
            ],
        )?;
        Ok(())
    }

    fn load_credential_blob_record(
        &self,
        credential_ref: &CredentialRef,
    ) -> Result<CredentialBlobRecord, CredentialStoreError> {
        self.connection
            .query_row(
                "SELECT credential_ref, encrypted_blob_bytes, key_version, created_at_unix_ms, updated_at_unix_ms
                 FROM credentials WHERE credential_ref = ?1",
                params![credential_ref.0],
                |row| {
                    Ok(CredentialBlobRecord {
                        credential_ref: CredentialRef(row.get(0)?),
                        encrypted_blob_bytes: row.get(1)?,
                        key_version: row.get(2)?,
                        created_at: UnixTimeMillis(row.get(3)?),
                        updated_at: UnixTimeMillis(row.get(4)?),
                    })
                },
            )
            .optional()?
            .ok_or(CredentialStoreError::CredentialNotFound)
    }

    fn save_face_template_record(
        &mut self,
        face_template_record: FaceTemplateRecord,
    ) -> Result<(), CredentialStoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO face_templates (
                face_template_ref, enrolled_user_id, model_family, model_version,
                encrypted_template_bytes, created_at_unix_ms, updated_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                face_template_record.face_template_ref.0,
                face_template_record.enrolled_user_id.0,
                face_template_record.model_family,
                face_template_record.model_version,
                face_template_record.encrypted_template_bytes,
                face_template_record.created_at.0,
                face_template_record.updated_at.0,
            ],
        )?;
        Ok(())
    }

    fn load_face_template_record(
        &self,
        face_template_ref: &FaceTemplateRef,
    ) -> Result<FaceTemplateRecord, CredentialStoreError> {
        self.connection
            .query_row(
                "SELECT face_template_ref, enrolled_user_id, model_family, model_version,
                    encrypted_template_bytes, created_at_unix_ms, updated_at_unix_ms
                 FROM face_templates WHERE face_template_ref = ?1",
                params![face_template_ref.0],
                |row| {
                    Ok(FaceTemplateRecord {
                        face_template_ref: FaceTemplateRef(row.get(0)?),
                        enrolled_user_id: UserId(row.get(1)?),
                        model_family: row.get(2)?,
                        model_version: row.get(3)?,
                        encrypted_template_bytes: row.get(4)?,
                        created_at: UnixTimeMillis(row.get(5)?),
                        updated_at: UnixTimeMillis(row.get(6)?),
                    })
                },
            )
            .optional()?
            .ok_or(CredentialStoreError::CredentialNotFound)
    }

    fn link_face_template_to_user(
        &mut self,
        link_record: UserFaceTemplateLinkRecord,
    ) -> Result<(), CredentialStoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO user_face_templates (
                user_id, face_template_ref, linked_at_unix_ms
            ) VALUES (?1, ?2, ?3)",
            params![
                link_record.user_id.0,
                link_record.face_template_ref.0,
                link_record.linked_at.0,
            ],
        )?;
        Ok(())
    }

    fn list_face_template_refs_for_user(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<FaceTemplateRef>, CredentialStoreError> {
        let mut statement = self.connection.prepare(
            "SELECT face_template_ref FROM user_face_templates
             WHERE user_id = ?1 ORDER BY linked_at_unix_ms ASC",
        )?;
        let rows = statement.query_map(params![user_id.0], |row| {
            Ok(FaceTemplateRef(row.get::<_, String>(0)?))
        })?;

        let mut refs = Vec::new();
        for row in rows {
            refs.push(row?);
        }
        Ok(refs)
    }

    fn save_policy_record(
        &mut self,
        policy_record: PolicyRecord,
    ) -> Result<(), CredentialStoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO policies (
                policy_id, liveness_requirement, face_match_threshold,
                failure_limit_before_cooldown, cooldown_duration_seconds,
                created_at_unix_ms, updated_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                policy_record.policy_id.0,
                liveness_requirement_to_storage(policy_record.liveness_requirement),
                policy_record.face_match_threshold,
                policy_record.failure_limit_before_cooldown,
                policy_record.cooldown_duration_seconds,
                0_i64,
                0_i64,
            ],
        )?;
        Ok(())
    }

    fn load_policy_record(
        &self,
        policy_id: &PolicyId,
    ) -> Result<PolicyRecord, CredentialStoreError> {
        self.connection
            .query_row(
                "SELECT policy_id, liveness_requirement, face_match_threshold,
                    failure_limit_before_cooldown, cooldown_duration_seconds
                 FROM policies WHERE policy_id = ?1",
                params![policy_id.0],
                |row| {
                    Ok(PolicyRecord {
                        policy_id: PolicyId(row.get(0)?),
                        liveness_requirement: liveness_requirement_from_storage(
                            row.get::<_, String>(1)?,
                        )?,
                        face_match_threshold: row.get(2)?,
                        failure_limit_before_cooldown: row.get(3)?,
                        cooldown_duration_seconds: row.get(4)?,
                    })
                },
            )
            .optional()?
            .ok_or(CredentialStoreError::CredentialNotFound)
    }

    fn append_audit_log_record(
        &mut self,
        audit_log_record: AuditLogRecord,
    ) -> Result<(), CredentialStoreError> {
        self.connection.execute(
            "INSERT INTO audit_log (
                audit_event_id, event_kind, event_outcome, occurred_at_unix_ms, user_id, detail_code
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                audit_log_record.audit_event_id,
                audit_event_kind_to_storage(audit_log_record.event_kind),
                audit_event_outcome_to_storage(audit_log_record.event_outcome),
                audit_log_record.occurred_at.0,
                audit_log_record.user_id.map(|user_id| user_id.0),
                audit_log_record.detail_code,
            ],
        )?;
        Ok(())
    }
}

fn encode_sqlcipher_raw_key(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn account_type_to_storage(account_type: AccountType) -> &'static str {
    match account_type {
        AccountType::Local => "local",
        AccountType::MicrosoftAccount => "microsoft_account",
        AccountType::Domain => "domain",
    }
}

fn account_type_from_storage(account_type: String) -> rusqlite::Result<AccountType> {
    match account_type.as_str() {
        "local" => Ok(AccountType::Local),
        "microsoft_account" => Ok(AccountType::MicrosoftAccount),
        "domain" => Ok(AccountType::Domain),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn liveness_requirement_to_storage(requirement: LivenessRequirement) -> &'static str {
    match requirement {
        LivenessRequirement::Required => "required",
        LivenessRequirement::NotRequired => "not_required",
    }
}

fn liveness_requirement_from_storage(requirement: String) -> rusqlite::Result<LivenessRequirement> {
    match requirement.as_str() {
        "required" => Ok(LivenessRequirement::Required),
        "not_required" => Ok(LivenessRequirement::NotRequired),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn audit_event_kind_to_storage(event_kind: AuditEventKind) -> &'static str {
    match event_kind {
        AuditEventKind::StoreInitialized => "store_initialized",
        AuditEventKind::CredentialBlobSaved => "credential_blob_saved",
        AuditEventKind::CredentialBlobLoaded => "credential_blob_loaded",
        AuditEventKind::FaceTemplateSaved => "face_template_saved",
        AuditEventKind::PolicyUpdated => "policy_updated",
        AuditEventKind::AuthenticationAttemptLogged => "authentication_attempt_logged",
    }
}

fn audit_event_outcome_to_storage(event_outcome: AuditEventOutcome) -> &'static str {
    match event_outcome {
        AuditEventOutcome::Completed => "completed",
        AuditEventOutcome::Rejected => "rejected",
        AuditEventOutcome::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use common_protocol::{AccountType, CredentialRef, UserId};

    use crate::{
        CredentialBlobRecord, CredentialStoreError, FaceTemplateRecord, FaceTemplateRef,
        LivenessRequirement, MASTER_KEY_LEN, MasterKey, PolicyId, PolicyRecord,
        SqlCipherRepository, StoreRepository, UnixTimeMillis, UserFaceTemplateLinkRecord,
        persistence::records::StoredUserRecord,
    };

    #[test]
    fn sqlcipher_repository_round_trips_core_records() -> Result<(), CredentialStoreError> {
        let database_path = unique_temp_path("store.db")?;
        let master_key = MasterKey::from_bytes([3_u8; MASTER_KEY_LEN]);
        let mut repository = SqlCipherRepository::open(&database_path, &master_key)?;

        repository.save_policy_record(PolicyRecord {
            policy_id: PolicyId("default".to_owned()),
            liveness_requirement: LivenessRequirement::Required,
            face_match_threshold: 0.363,
            failure_limit_before_cooldown: 3,
            cooldown_duration_seconds: 30,
        })?;
        repository.save_credential_blob_record(CredentialBlobRecord {
            credential_ref: CredentialRef("cred-1".to_owned()),
            encrypted_blob_bytes: vec![1, 2, 3],
            key_version: 1,
            created_at: UnixTimeMillis(10),
            updated_at: UnixTimeMillis(10),
        })?;
        repository.save_user_record(StoredUserRecord {
            user_id: UserId("user-1".to_owned()),
            user_sid: "S-1-5-21-example".to_owned(),
            username: "Liu".to_owned(),
            account_type: AccountType::Local,
            credential_ref: CredentialRef("cred-1".to_owned()),
            policy_id: PolicyId("default".to_owned()),
            created_at: UnixTimeMillis(10),
            updated_at: UnixTimeMillis(10),
        })?;
        repository.save_face_template_record(FaceTemplateRecord {
            face_template_ref: FaceTemplateRef("face-1".to_owned()),
            enrolled_user_id: UserId("user-1".to_owned()),
            model_family: "sface".to_owned(),
            model_version: "2021dec".to_owned(),
            encrypted_template_bytes: vec![9, 8, 7],
            created_at: UnixTimeMillis(10),
            updated_at: UnixTimeMillis(10),
        })?;
        repository.link_face_template_to_user(UserFaceTemplateLinkRecord {
            user_id: UserId("user-1".to_owned()),
            face_template_ref: FaceTemplateRef("face-1".to_owned()),
            linked_at: UnixTimeMillis(11),
        })?;

        let loaded_user = repository.load_user_record(&UserId("user-1".to_owned()))?;
        let linked_faces =
            repository.list_face_template_refs_for_user(&UserId("user-1".to_owned()))?;
        let loaded_credential =
            repository.load_credential_blob_record(&CredentialRef("cred-1".to_owned()))?;

        drop(repository);
        let _ = fs::remove_file(&database_path);

        assert_eq!(loaded_user.username, "Liu");
        assert_eq!(linked_faces, vec![FaceTemplateRef("face-1".to_owned())]);
        assert_eq!(loaded_credential.encrypted_blob_bytes, vec![1, 2, 3]);
        Ok(())
    }

    #[test]
    fn sqlcipher_database_cannot_be_read_with_plain_sqlite() -> Result<(), CredentialStoreError> {
        let database_path = unique_temp_path("encrypted-store.db")?;
        let master_key = MasterKey::from_bytes([4_u8; MASTER_KEY_LEN]);
        let repository = SqlCipherRepository::open(&database_path, &master_key)?;
        drop(repository);

        let plain_open_result = rusqlite::Connection::open(&database_path).and_then(|connection| {
            connection.query_row("SELECT count(*) FROM users", [], |_| Ok(()))
        });
        let _ = fs::remove_file(&database_path);

        assert!(plain_open_result.is_err());
        Ok(())
    }

    fn unique_temp_path(name: &str) -> Result<PathBuf, CredentialStoreError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CredentialStoreError::IoFailed)?
            .as_nanos();
        Ok(std::env::temp_dir().join(format!(
            "winfaceunlock-sqlcipher-{}-{}-{name}",
            std::process::id(),
            nanos
        )))
    }
}
