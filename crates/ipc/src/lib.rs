mod codec;
mod credential_material;
mod grant_registry;
#[cfg(windows)]
mod named_pipe;
mod service_handler;

use common_protocol::{ProtocolError, ServiceEvent, ServiceRequest};

pub use codec::{decode_event, decode_request, encode_event, encode_request};
pub use credential_material::{
    CredentialMaterialProtector, CredentialMaterialSecret,
    DpapiLocalMachineCredentialMaterialProtector,
};
pub use grant_registry::GrantRegistry;
#[cfg(windows)]
pub use named_pipe::{NamedPipeClient, NamedPipeServer, PipeSecurityDescriptor};
pub use service_handler::{
    AuthGrantIssueResult, AuthGrantIssuer, ProtectedCredentialMaterialResolver,
    ProtectedCredentialResolver, ServiceConfigApplier, ServiceRequestHandler,
    SystemUnixTimeMillisClock, UnixTimeMillisClock,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipeSecurity {
    pub allow_local_system: bool,
    pub allow_administrators: bool,
    pub allow_interactive_users: bool,
    pub allow_authenticated_users: bool,
    pub allow_service_sid: bool,
}

impl PipeSecurity {
    pub fn service_default() -> Self {
        Self {
            allow_local_system: true,
            allow_administrators: true,
            allow_interactive_users: true,
            allow_authenticated_users: true,
            allow_service_sid: true,
        }
    }
}

pub trait IpcServer {
    fn start(&mut self, security: PipeSecurity) -> Result<(), ProtocolError>;
    fn receive(&mut self) -> Result<ServiceRequest, ProtocolError>;
    fn send(&mut self, event: ServiceEvent) -> Result<(), ProtocolError>;
    fn stop(&mut self);
}

pub trait IpcClient {
    fn connect(&mut self) -> Result<(), ProtocolError>;
    fn request(&mut self, request: ServiceRequest) -> Result<ServiceEvent, ProtocolError>;
    fn disconnect(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pipe_security_is_restricted_to_local_trusted_identities() {
        let security = PipeSecurity::service_default();

        assert!(security.allow_local_system);
        assert!(security.allow_administrators);
        assert!(security.allow_interactive_users);
        assert!(security.allow_authenticated_users);
        assert!(security.allow_service_sid);
    }
}
