use std::{thread, time::Duration};

use common_protocol::{
    AuthFailureReason, AuthSource, AuthTriggerSource, PIPE_NAME, ProtectedCredentialMaterial,
    ProtocolError, ServiceEvent, ServiceRequest, SessionId,
};
use ipc::{
    CredentialMaterialProtector, DpapiLocalMachineCredentialMaterialProtector, IpcClient,
    NamedPipeClient,
};

use crate::{provider_log::write_provider_event_detail, provider_state::CredentialMaterial};

const AUTH_RESULT_POLL_ATTEMPTS: usize = 80;
const AUTH_RESULT_POLL_DELAY: Duration = Duration::from_millis(500);

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
        trigger_source: AuthTriggerSource,
    ) -> Result<ProviderWakeOutcome, ProtocolError> {
        write_provider_event_detail(
            "Provider.BrokerWakeAuthRequest",
            format!(
                "session_id={} auth_trigger_source={trigger_source:?}",
                session_id.0
            ),
        );
        let wake_event = self.request(ServiceRequest::WakeAuth {
            session_id: session_id.clone(),
            source: self.wake_auth_source,
            trigger_source,
        })?;
        let grant = match wake_event {
            ServiceEvent::AuthSucceeded { grant } => {
                write_provider_event_detail(
                    "Provider.BrokerWakeAuthSucceeded",
                    format!("session_id={}", grant.session_id.0),
                );
                grant
            }
            ServiceEvent::AuthStarted { session_id } => {
                write_provider_event_detail(
                    "Provider.BrokerWakeAuthStarted",
                    format!("session_id={}", session_id.0),
                );
                match self.wait_for_auth_result(session_id.clone())? {
                    Ok(grant) => grant,
                    Err(reason) => {
                        return Ok(ProviderWakeOutcome::AuthFailed {
                            session_id,
                            auth_failure_reason: reason,
                        });
                    }
                }
            }
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

    fn wait_for_auth_result(
        &self,
        session_id: SessionId,
    ) -> Result<Result<common_protocol::AuthGrant, AuthFailureReason>, ProtocolError> {
        for attempt in 1..=AUTH_RESULT_POLL_ATTEMPTS {
            thread::sleep(AUTH_RESULT_POLL_DELAY);
            match self.request(ServiceRequest::FetchAuthResult {
                session_id: session_id.clone(),
            })? {
                ServiceEvent::AuthSucceeded { grant } => {
                    write_provider_event_detail(
                        "Provider.BrokerAuthResultSucceeded",
                        format!(
                            "attempt={attempt}/{AUTH_RESULT_POLL_ATTEMPTS} session_id={}",
                            grant.session_id.0
                        ),
                    );
                    return Ok(Ok(grant));
                }
                ServiceEvent::AuthStarted { .. } => continue,
                ServiceEvent::AuthFailed { reason, .. } => {
                    write_provider_event_detail(
                        "Provider.BrokerAuthResultFailed",
                        format!(
                            "attempt={attempt}/{AUTH_RESULT_POLL_ATTEMPTS} session_id={} reason={reason:?}",
                            session_id.0
                        ),
                    );
                    return Ok(Err(reason));
                }
                ServiceEvent::RequestRejected { reason } => return Err(reason),
                _ => return Err(ProtocolError::InvalidMessage),
            }
        }

        write_provider_event_detail(
            "Provider.BrokerAuthResultTimeout",
            format!(
                "attempts={AUTH_RESULT_POLL_ATTEMPTS} session_id={}",
                session_id.0
            ),
        );
        Err(ProtocolError::TransportUnavailable)
    }

    fn request(&self, request: ServiceRequest) -> Result<ServiceEvent, ProtocolError> {
        let mut client = NamedPipeClient::new(self.pipe_name.clone());
        if let Err(error) = client.connect() {
            write_provider_event_detail(
                "Provider.PipeConnectFailed",
                format!(
                    "pipe={} error={error:?} win32_error={}",
                    self.pipe_name,
                    format_win32_error(client.last_connect_error())
                ),
            );
            return Err(error);
        }
        let event = client.request(request)?;
        client.disconnect();
        Ok(event)
    }
}

fn format_win32_error(error: Option<u32>) -> String {
    match error {
        Some(code) => format!("{code}({})", win32_error_name(code)),
        None => "none".to_owned(),
    }
}

fn win32_error_name(code: u32) -> &'static str {
    match code {
        2 => "ERROR_FILE_NOT_FOUND",
        5 => "ERROR_ACCESS_DENIED",
        231 => "ERROR_PIPE_BUSY",
        _ => "UNKNOWN",
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
