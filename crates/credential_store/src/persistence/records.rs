use common_protocol::{AccountType, CredentialRef, UserId};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UnixTimeMillis(pub i64);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FaceTemplateRef(pub String);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PolicyId(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LivenessRequirement {
    Required,
    NotRequired,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolicyRecord {
    pub policy_id: PolicyId,
    pub liveness_requirement: LivenessRequirement,
    pub face_match_threshold: f32,
    pub failure_limit_before_cooldown: u32,
    pub cooldown_duration_seconds: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialBlobRecord {
    pub credential_ref: CredentialRef,
    pub encrypted_blob_bytes: Vec<u8>,
    pub key_version: u32,
    pub created_at: UnixTimeMillis,
    pub updated_at: UnixTimeMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaceTemplateRecord {
    pub face_template_ref: FaceTemplateRef,
    pub enrolled_user_id: UserId,
    pub model_family: String,
    pub model_version: String,
    pub encrypted_template_bytes: Vec<u8>,
    pub created_at: UnixTimeMillis,
    pub updated_at: UnixTimeMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditLogRecord {
    pub audit_event_id: String,
    pub event_kind: AuditEventKind,
    pub event_outcome: AuditEventOutcome,
    pub occurred_at: UnixTimeMillis,
    pub user_id: Option<UserId>,
    pub detail_code: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditEventKind {
    StoreInitialized,
    CredentialBlobSaved,
    CredentialBlobLoaded,
    FaceTemplateSaved,
    PolicyUpdated,
    AuthenticationAttemptLogged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditEventOutcome {
    Completed,
    Rejected,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredUserRecord {
    pub user_id: UserId,
    pub user_sid: String,
    pub username: String,
    pub account_type: AccountType,
    pub credential_ref: CredentialRef,
    pub policy_id: PolicyId,
    pub created_at: UnixTimeMillis,
    pub updated_at: UnixTimeMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserFaceTemplateLinkRecord {
    pub user_id: UserId,
    pub face_template_ref: FaceTemplateRef,
    pub linked_at: UnixTimeMillis,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liveness_policy_uses_explicit_requirement_enum() {
        let policy = PolicyRecord {
            policy_id: PolicyId("default".to_owned()),
            liveness_requirement: LivenessRequirement::Required,
            face_match_threshold: 0.363,
            failure_limit_before_cooldown: 3,
            cooldown_duration_seconds: 30,
        };

        assert_eq!(policy.liveness_requirement, LivenessRequirement::Required);
    }
}
