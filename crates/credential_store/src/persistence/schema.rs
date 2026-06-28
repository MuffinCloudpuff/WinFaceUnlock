use crate::{DatabaseMigration, DatabaseSchemaVersion};

pub struct SqlCipherSchema;

impl SqlCipherSchema {
    pub const CURRENT_VERSION: DatabaseSchemaVersion = DatabaseSchemaVersion(1);

    pub const MIGRATIONS: &'static [DatabaseMigration] = &[DatabaseMigration::new(
        DatabaseSchemaVersion(1),
        &[
            CREATE_SCHEMA_METADATA,
            CREATE_CREDENTIALS,
            CREATE_POLICIES,
            CREATE_USERS,
            CREATE_FACE_TEMPLATES,
            CREATE_USER_FACE_TEMPLATES,
            CREATE_AUDIT_LOG,
        ],
    )];

    pub fn all_statements() -> impl Iterator<Item = &'static str> {
        Self::MIGRATIONS
            .iter()
            .flat_map(|migration| migration.statements.iter().copied())
    }
}

const CREATE_SCHEMA_METADATA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_metadata (
    schema_key TEXT PRIMARY KEY NOT NULL,
    schema_value TEXT NOT NULL
);
"#;

const CREATE_USERS: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    user_id TEXT PRIMARY KEY NOT NULL,
    user_sid TEXT NOT NULL,
    username TEXT NOT NULL,
    account_type TEXT NOT NULL,
    credential_ref TEXT NOT NULL,
    policy_id TEXT NOT NULL,
    created_at_unix_ms INTEGER NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL,
    FOREIGN KEY (credential_ref) REFERENCES credentials(credential_ref),
    FOREIGN KEY (policy_id) REFERENCES policies(policy_id)
);
"#;

const CREATE_CREDENTIALS: &str = r#"
CREATE TABLE IF NOT EXISTS credentials (
    credential_ref TEXT PRIMARY KEY NOT NULL,
    encrypted_blob_bytes BLOB NOT NULL,
    key_version INTEGER NOT NULL,
    created_at_unix_ms INTEGER NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL
);
"#;

const CREATE_FACE_TEMPLATES: &str = r#"
CREATE TABLE IF NOT EXISTS face_templates (
    face_template_ref TEXT PRIMARY KEY NOT NULL,
    enrolled_user_id TEXT NOT NULL,
    model_family TEXT NOT NULL,
    model_version TEXT NOT NULL,
    encrypted_template_bytes BLOB NOT NULL,
    created_at_unix_ms INTEGER NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL,
    FOREIGN KEY (enrolled_user_id) REFERENCES users(user_id)
);
"#;

const CREATE_USER_FACE_TEMPLATES: &str = r#"
CREATE TABLE IF NOT EXISTS user_face_templates (
    user_id TEXT NOT NULL,
    face_template_ref TEXT NOT NULL,
    linked_at_unix_ms INTEGER NOT NULL,
    PRIMARY KEY (user_id, face_template_ref),
    FOREIGN KEY (user_id) REFERENCES users(user_id),
    FOREIGN KEY (face_template_ref) REFERENCES face_templates(face_template_ref)
);
"#;

const CREATE_POLICIES: &str = r#"
CREATE TABLE IF NOT EXISTS policies (
    policy_id TEXT PRIMARY KEY NOT NULL,
    liveness_requirement TEXT NOT NULL,
    face_match_threshold REAL NOT NULL,
    failure_limit_before_cooldown INTEGER NOT NULL,
    cooldown_duration_seconds INTEGER NOT NULL,
    created_at_unix_ms INTEGER NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL
);
"#;

const CREATE_AUDIT_LOG: &str = r#"
CREATE TABLE IF NOT EXISTS audit_log (
    audit_event_id TEXT PRIMARY KEY NOT NULL,
    event_kind TEXT NOT NULL,
    event_outcome TEXT NOT NULL,
    occurred_at_unix_ms INTEGER NOT NULL,
    user_id TEXT,
    detail_code TEXT NOT NULL
);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_ordered_and_non_empty() {
        let mut previous_version = DatabaseSchemaVersion(0);

        for migration in SqlCipherSchema::MIGRATIONS {
            assert!(migration.target_version > previous_version);
            assert!(!migration.statements.is_empty());
            previous_version = migration.target_version;
        }

        assert_eq!(previous_version, SqlCipherSchema::CURRENT_VERSION);
    }

    #[test]
    fn schema_does_not_define_plaintext_password_columns() {
        let schema = SqlCipherSchema::all_statements()
            .collect::<Vec<_>>()
            .join("\n");
        let lowered_schema = schema.to_ascii_lowercase();

        assert!(!lowered_schema.contains("password"));
        assert!(!lowered_schema.contains("user_pwd"));
        assert!(!lowered_schema.contains("plain"));
    }

    #[test]
    fn schema_uses_explicit_status_and_policy_column_names() {
        let schema = SqlCipherSchema::all_statements()
            .collect::<Vec<_>>()
            .join("\n");

        assert!(schema.contains("liveness_requirement"));
        assert!(schema.contains("event_outcome"));
        assert!(schema.contains("failure_limit_before_cooldown"));
    }

    #[test]
    fn schema_models_user_face_templates_as_link_table() {
        let schema = SqlCipherSchema::all_statements()
            .collect::<Vec<_>>()
            .join("\n");

        assert!(schema.contains("CREATE TABLE IF NOT EXISTS user_face_templates"));
        assert!(schema.contains("PRIMARY KEY (user_id, face_template_ref)"));
    }
}
