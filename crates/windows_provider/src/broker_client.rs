use common_protocol::{
    AuthFailureReason, AuthSource, PIPE_NAME, ProtectedCredentialMaterial, ProtocolError,
    ServiceEvent, ServiceRequest, SessionId,
};
use ipc::{
    CredentialMaterialProtector, DpapiLocalMachineCredentialMaterialProtector, IpcClient,
    NamedPipeClient,
};

use crate::provider_state::CredentialMaterial;

#[derive(Clone, Debug, PartialEq)]
pub enum ProviderWakeOutcome {
    CredentialMaterialReady {
        session_id: SessionId,
        credential_material: CredentialMaterial,
    },
    AuthFailed {
        session_id: SessionId,
        auth_failure_reason: AuthFailureReason,
    },
    RequestRejected {
        session_id: SessionId,
        protocol_error: ProtocolError,
    },
}

pub struct ProviderBrokerClient {
    pipe_name: String,
    wake_auth_source: AuthSource,
}

impl ProviderBrokerClient {
    pub fn service_default(wake_auth_source: AuthSource) -> Self {
        Self {
            pipe_name: PIPE_NAME.to_owned(),
            wake_auth_source,
        }
    }

    pub fn wake_and_fetch_credential_material(
        &self,
        session_id: SessionId,
    ) -> Result<ProviderWakeOutcome, ProtocolError> {
        let wake_event = self.request(ServiceRequest::WakeAuth {
            session_id: session_id.clone(),
            source: self.wake_auth_source,
        })?;
        let grant = match wake_event {
            ServiceEvent::AuthSucceeded { grant } => grant,
            ServiceEvent::AuthFailed { reason, .. } => {
                return Ok(ProviderWakeOutcome::AuthFailed {
                    session_id,
                    auth_failure_reason: reason,
                });
            }
            ServiceEvent::RequestRejected { reason } => {
                return Ok(ProviderWakeOutcome::RequestRejected {
                    session_id,
                    protocol_error: reason,
                });
            }
            _ => {
                return Ok(ProviderWakeOutcome::RequestRejected {
                    session_id,
                    protocol_error: ProtocolError::InvalidMessage,
                });
            }
        };

        let credential_event = self.request(ServiceRequest::FetchCredentialMaterial {
            session_id: grant.session_id.clone(),
            grant_id: grant.grant_id,
            nonce: grant.nonce,
        })?;

        match credential_event {
            ServiceEvent::CredentialMaterialReady {
                protected_credential_material,
                ..
            } => Ok(ProviderWakeOutcome::CredentialMaterialReady {
                session_id: grant.session_id,
                credential_material: unprotect_credential_material(&protected_credential_material)?,
            }),
            ServiceEvent::RequestRejected { reason } => Ok(ProviderWakeOutcome::RequestRejected {
                session_id: grant.session_id,
                protocol_error: reason,
            }),
            _ => Ok(ProviderWakeOutcome::RequestRejected {
                session_id: grant.session_id,
                protocol_error: ProtocolError::InvalidMessage,
            }),
        }
    }

    fn request(&self, request: ServiceRequest) -> Result<ServiceEvent, ProtocolError> {
        let mut client = NamedPipeClient::new(self.pipe_name.clone());
        client.connect()?;
        let event = client.request(request)?;
        client.disconnect();
        Ok(event)
    }
}

fn unprotect_credential_material(
    protected_credential_material: &ProtectedCredentialMaterial,
) -> Result<CredentialMaterial, ProtocolError> {
    let protector = DpapiLocalMachineCredentialMaterialProtector;
    let secret = protector.unprotect_credential_material(protected_credential_material)?;
    let password = String::from_utf8(secret.password).map_err(|_| ProtocolError::InvalidMessage)?;
    Ok(CredentialMaterial {
        domain: secret.domain,
        username: secret.username,
        password,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_outcome_names_credential_reference_layer_explicitly() {
        let outcome = ProviderWakeOutcome::RequestRejected {
            session_id: SessionId("session-1".to_owned()),
            protocol_error: ProtocolError::Unauthorized,
        };

        assert!(matches!(
            outcome,
            ProviderWakeOutcome::RequestRejected { .. }
        ));
    }
}
