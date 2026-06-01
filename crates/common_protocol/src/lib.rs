use std::{hash::Hash, time::Duration};

use serde::{Deserialize, Serialize};

pub const SERVICE_NAME: &str = "WinFaceUnlockService";
pub const PROVIDER_NAME: &str = "WinFaceUnlockProvider";
pub const PIPE_NAME: &str = r"\\.\pipe\winfaceunlock.service";
pub const DEFAULT_GRANT_TTL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct UserId(pub String);

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct SessionId(pub String);

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct GrantId(pub String);

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Nonce(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AccountType {
    Local,
    MicrosoftAccount,
    Domain,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthSource {
    LocalCamera,
    VehicleCamera,
    ManualTest,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthFailureReason {
    NoFaceDetected,
    MultipleFacesDetected,
    MatchBelowThreshold,
    TemplateModelMismatch,
    LivenessFailed,
    CooldownActive,
    Timeout,
    Cancelled,
    InternalError,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AuthScore {
    pub match_score: f32,
    pub liveness_score: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct CredentialRef(pub String);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProtectedCredential {
    pub user_id: UserId,
    pub credential_ref: CredentialRef,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CredentialMaterialProtection {
    DpapiLocalMachineV1,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProtectedCredentialMaterial {
    pub user_id: UserId,
    pub domain: String,
    pub username: String,
    pub protected_password: Vec<u8>,
    pub protection: CredentialMaterialProtection,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AuthGrant {
    pub grant_id: GrantId,
    pub nonce: Nonce,
    pub session_id: SessionId,
    pub user_id: UserId,
    pub source: AuthSource,
    pub score: AuthScore,
    pub issued_at_unix_ms: i64,
    pub expires_at_unix_ms: i64,
}

impl AuthGrant {
    pub fn is_expired_at(&self, current_time_unix_ms: i64) -> bool {
        current_time_unix_ms >= self.expires_at_unix_ms
    }

    pub fn has_valid_time_window(&self) -> bool {
        self.expires_at_unix_ms > self.issued_at_unix_ms
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ServiceRequest {
    WakeAuth {
        session_id: SessionId,
        source: AuthSource,
    },
    FetchCredential {
        session_id: SessionId,
        grant_id: GrantId,
        nonce: Nonce,
    },
    FetchCredentialMaterial {
        session_id: SessionId,
        grant_id: GrantId,
        nonce: Nonce,
    },
    Cancel {
        session_id: SessionId,
    },
    HealthCheck,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ServiceEvent {
    AuthStarted {
        session_id: SessionId,
    },
    AuthSucceeded {
        grant: AuthGrant,
    },
    AuthFailed {
        session_id: SessionId,
        reason: AuthFailureReason,
    },
    CredentialReady {
        grant_id: GrantId,
        protected_credential: ProtectedCredential,
    },
    CredentialMaterialReady {
        grant_id: GrantId,
        protected_credential_material: ProtectedCredentialMaterial,
    },
    AuthCancelled {
        session_id: SessionId,
    },
    RequestRejected {
        reason: ProtocolError,
    },
    HealthOk,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ProtocolError {
    Unauthorized,
    InvalidMessage,
    ExpiredGrant,
    UsedGrant,
    SessionMismatch,
    TransportUnavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_expires_at_explicit_expiry_boundary() {
        let grant = AuthGrant {
            grant_id: GrantId("grant".to_owned()),
            nonce: Nonce("nonce".to_owned()),
            session_id: SessionId("session".to_owned()),
            user_id: UserId("user".to_owned()),
            source: AuthSource::LocalCamera,
            score: AuthScore {
                match_score: 0.82,
                liveness_score: None,
            },
            issued_at_unix_ms: 1_000,
            expires_at_unix_ms: 6_000,
        };

        assert!(grant.has_valid_time_window());
        assert!(!grant.is_expired_at(5_999));
        assert!(grant.is_expired_at(6_000));
    }
}
