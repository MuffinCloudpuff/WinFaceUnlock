#![allow(unsafe_code)]

use std::ptr;

use common_protocol::{ProtocolError, ServiceEvent, ServiceRequest};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ,
        GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    },
    Security::{
        Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, FlushFileBuffers,
        OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
    },
    System::{
        IO::OVERLAPPED,
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
            WaitNamedPipeW,
        },
    },
};

use crate::{
    PipeSecurity,
    codec::{decode_event, decode_request, encode_event, encode_request},
};

const PIPE_BUFFER_SIZE: u32 = 64 * 1024;
const DEFAULT_PIPE_TIMEOUT_MS: u32 = 5_000;
const CLIENT_CONNECT_RETRY_COUNT: usize = 20;
const CLIENT_CONNECT_RETRY_WAIT_MS: u32 = 100;

pub struct PipeSecurityDescriptor {
    security_descriptor: PSECURITY_DESCRIPTOR,
    security_attributes: SECURITY_ATTRIBUTES,
}

impl PipeSecurityDescriptor {
    pub fn from_pipe_security(security: &PipeSecurity) -> Result<Self, ProtocolError> {
        let sddl = pipe_security_sddl(security)?;
        let mut security_descriptor = ptr::null_mut();
        let sddl_wide = to_wide_null(&sddl);

        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_wide.as_ptr(),
                SDDL_REVISION_1,
                &mut security_descriptor,
                ptr::null_mut(),
            )
        };
        if converted == 0 || security_descriptor.is_null() {
            return Err(ProtocolError::Unauthorized);
        }

        Ok(Self {
            security_descriptor,
            security_attributes: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: security_descriptor,
                bInheritHandle: 0,
            },
        })
    }

    fn as_ptr(&self) -> *const SECURITY_ATTRIBUTES {
        &self.security_attributes
    }
}

impl Drop for PipeSecurityDescriptor {
    fn drop(&mut self) {
        if !self.security_descriptor.is_null() {
            unsafe {
                let _ = LocalFree(self.security_descriptor.cast());
            }
        }
    }
}

pub struct NamedPipeServer {
    pipe_name: String,
    pipe_handle: Option<OwnedHandle>,
    client_connected: bool,
}

impl NamedPipeServer {
    pub fn new(pipe_name: impl Into<String>) -> Self {
        Self {
            pipe_name: pipe_name.into(),
            pipe_handle: None,
            client_connected: false,
        }
    }

    fn pipe_handle(&self) -> Result<HANDLE, ProtocolError> {
        self.pipe_handle
            .as_ref()
            .map(|handle| handle.raw)
            .ok_or(ProtocolError::TransportUnavailable)
    }

    fn connect_client_if_needed(&mut self) -> Result<(), ProtocolError> {
        if self.client_connected {
            return Ok(());
        }

        let pipe_handle = self.pipe_handle()?;
        let connected = unsafe { ConnectNamedPipe(pipe_handle, ptr::null_mut::<OVERLAPPED>()) };
        if connected == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_PIPE_CONNECTED {
                return Err(ProtocolError::TransportUnavailable);
            }
        }
        self.client_connected = true;
        Ok(())
    }

    pub fn disconnect_current_client(&mut self) {
        if let Some(handle) = self.pipe_handle.as_ref() {
            unsafe {
                let _ = DisconnectNamedPipe(handle.raw);
            }
        }
        self.client_connected = false;
    }
}

impl crate::IpcServer for NamedPipeServer {
    fn start(&mut self, security: PipeSecurity) -> Result<(), ProtocolError> {
        let security_descriptor = PipeSecurityDescriptor::from_pipe_security(&security)?;
        let pipe_name = to_wide_null(&self.pipe_name);

        let handle = unsafe {
            CreateNamedPipeW(
                pipe_name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                PIPE_BUFFER_SIZE,
                PIPE_BUFFER_SIZE,
                DEFAULT_PIPE_TIMEOUT_MS,
                security_descriptor.as_ptr(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(ProtocolError::TransportUnavailable);
        }

        self.pipe_handle = Some(OwnedHandle { raw: handle });
        Ok(())
    }

    fn receive(&mut self) -> Result<ServiceRequest, ProtocolError> {
        self.connect_client_if_needed()?;
        let result = read_frame(self.pipe_handle()?).and_then(|frame| decode_request(&frame));
        if result.is_err() {
            self.disconnect_current_client();
        }
        result
    }

    fn send(&mut self, event: ServiceEvent) -> Result<(), ProtocolError> {
        let frame = encode_event(&event)?;
        if let Err(error) = write_frame(self.pipe_handle()?, &frame) {
            self.disconnect_current_client();
            return Err(error);
        }
        unsafe {
            let _ = FlushFileBuffers(self.pipe_handle()?);
            let _ = DisconnectNamedPipe(self.pipe_handle()?);
        }
        self.client_connected = false;
        Ok(())
    }

    fn stop(&mut self) {
        self.pipe_handle = None;
        self.client_connected = false;
    }
}

pub struct NamedPipeClient {
    pipe_name: String,
    pipe_handle: Option<OwnedHandle>,
    last_connect_error: Option<u32>,
}

impl NamedPipeClient {
    pub fn new(pipe_name: impl Into<String>) -> Self {
        Self {
            pipe_name: pipe_name.into(),
            pipe_handle: None,
            last_connect_error: None,
        }
    }

    fn pipe_handle(&self) -> Result<HANDLE, ProtocolError> {
        self.pipe_handle
            .as_ref()
            .map(|handle| handle.raw)
            .ok_or(ProtocolError::TransportUnavailable)
    }

    pub fn last_connect_error(&self) -> Option<u32> {
        self.last_connect_error
    }
}

impl crate::IpcClient for NamedPipeClient {
    fn connect(&mut self) -> Result<(), ProtocolError> {
        self.last_connect_error = None;
        let pipe_name = to_wide_null(&self.pipe_name);
        for _ in 0..CLIENT_CONNECT_RETRY_COUNT {
            let handle = unsafe {
                CreateFileW(
                    pipe_name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                self.pipe_handle = Some(OwnedHandle { raw: handle });
                return Ok(());
            }

            let last_error = unsafe { GetLastError() };
            self.last_connect_error = Some(last_error);
            if last_error != ERROR_PIPE_BUSY && last_error != ERROR_FILE_NOT_FOUND {
                return Err(ProtocolError::TransportUnavailable);
            }
            unsafe {
                let _ = WaitNamedPipeW(pipe_name.as_ptr(), CLIENT_CONNECT_RETRY_WAIT_MS);
            }
        }

        Err(ProtocolError::TransportUnavailable)
    }

    fn request(&mut self, request: ServiceRequest) -> Result<ServiceEvent, ProtocolError> {
        let frame = encode_request(&request)?;
        write_frame(self.pipe_handle()?, &frame)?;
        let response_frame = read_frame(self.pipe_handle()?)?;
        decode_event(&response_frame)
    }

    fn disconnect(&mut self) {
        self.pipe_handle = None;
    }
}

struct OwnedHandle {
    raw: HANDLE,
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if self.raw != INVALID_HANDLE_VALUE {
            unsafe {
                let _ = CloseHandle(self.raw);
            }
        }
    }
}

fn read_frame(handle: HANDLE) -> Result<Vec<u8>, ProtocolError> {
    let mut length_bytes = [0_u8; 4];
    read_exact(handle, &mut length_bytes)?;
    let payload_len = u32::from_le_bytes(length_bytes) as usize;
    if payload_len > PIPE_BUFFER_SIZE as usize {
        return Err(ProtocolError::InvalidMessage);
    }

    let mut frame = Vec::with_capacity(4 + payload_len);
    frame.extend_from_slice(&length_bytes);
    let mut payload = vec![0_u8; payload_len];
    read_exact(handle, &mut payload)?;
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn read_exact(handle: HANDLE, destination: &mut [u8]) -> Result<(), ProtocolError> {
    let mut offset = 0;
    while offset < destination.len() {
        let mut bytes_read = 0_u32;
        let read_ok = unsafe {
            ReadFile(
                handle,
                destination[offset..].as_mut_ptr(),
                (destination.len() - offset) as u32,
                &mut bytes_read,
                ptr::null_mut::<OVERLAPPED>(),
            )
        };
        if read_ok == 0 || bytes_read == 0 {
            return Err(ProtocolError::TransportUnavailable);
        }
        offset += bytes_read as usize;
    }
    Ok(())
}

fn write_frame(handle: HANDLE, frame: &[u8]) -> Result<(), ProtocolError> {
    let mut offset = 0;
    while offset < frame.len() {
        let mut bytes_written = 0_u32;
        let write_ok = unsafe {
            WriteFile(
                handle,
                frame[offset..].as_ptr(),
                (frame.len() - offset) as u32,
                &mut bytes_written,
                ptr::null_mut::<OVERLAPPED>(),
            )
        };
        if write_ok == 0 || bytes_written == 0 {
            return Err(ProtocolError::TransportUnavailable);
        }
        offset += bytes_written as usize;
    }
    Ok(())
}

fn pipe_security_sddl(security: &PipeSecurity) -> Result<String, ProtocolError> {
    let mut sddl = String::from("D:P");
    if security.allow_local_system {
        sddl.push_str("(A;;GA;;;SY)");
    }
    if security.allow_administrators {
        sddl.push_str("(A;;GA;;;BA)");
    }
    if security.allow_interactive_users {
        sddl.push_str("(A;;GRGW;;;IU)");
    }
    if security.allow_authenticated_users {
        sddl.push_str("(A;;GRGW;;;AU)");
    }
    if security.allow_service_sid {
        sddl.push_str("(A;;GA;;;OW)");
    }
    if !security.allow_local_system && !security.allow_administrators {
        return Err(ProtocolError::Unauthorized);
    }
    Ok(sddl)
}

fn to_wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::{
        sync::mpsc,
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    use common_protocol::{ServiceEvent, ServiceRequest};

    use crate::{IpcClient, IpcServer};

    use super::*;

    #[test]
    fn pipe_security_sddl_allows_only_trusted_local_identities() -> Result<(), ProtocolError> {
        let sddl = pipe_security_sddl(&PipeSecurity::service_default())?;

        assert!(sddl.contains("(A;;GA;;;SY)"));
        assert!(sddl.contains("(A;;GA;;;BA)"));
        assert!(sddl.contains("(A;;GRGW;;;IU)"));
        assert!(sddl.contains("(A;;GRGW;;;AU)"));
        assert!(sddl.contains("(A;;GA;;;OW)"));
        assert!(!sddl.contains("WD"));
        Ok(())
    }

    #[test]
    fn pipe_security_descriptor_rejects_empty_acl() {
        let result = PipeSecurityDescriptor::from_pipe_security(&PipeSecurity {
            allow_local_system: false,
            allow_administrators: false,
            allow_interactive_users: false,
            allow_authenticated_users: false,
            allow_service_sid: false,
        });

        assert!(matches!(result, Err(ProtocolError::Unauthorized)));
    }

    #[test]
    fn named_pipe_round_trips_health_check() -> Result<(), ProtocolError> {
        let pipe_name = unique_pipe_name();
        let (ready_tx, ready_rx) = mpsc::channel();
        let server_pipe_name = pipe_name.clone();

        let server_thread = thread::spawn(move || -> Result<(), ProtocolError> {
            let mut server = NamedPipeServer::new(server_pipe_name);
            server.start(PipeSecurity::service_default())?;
            ready_tx
                .send(())
                .map_err(|_| ProtocolError::TransportUnavailable)?;
            let request = server.receive()?;
            if request != ServiceRequest::HealthCheck {
                return Err(ProtocolError::InvalidMessage);
            }
            server.send(ServiceEvent::HealthOk)?;
            server.stop();
            Ok(())
        });

        if ready_rx.recv().is_err() {
            return server_thread
                .join()
                .map_err(|_| ProtocolError::TransportUnavailable)?;
        }
        let mut client = NamedPipeClient::new(pipe_name);
        client.connect()?;
        let response = client.request(ServiceRequest::HealthCheck)?;
        client.disconnect();

        let server_result = server_thread
            .join()
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        assert_eq!(server_result, Ok(()));
        assert_eq!(response, ServiceEvent::HealthOk);
        Ok(())
    }

    #[test]
    fn named_pipe_server_recovers_after_client_disconnects_before_request()
    -> Result<(), ProtocolError> {
        let pipe_name = unique_pipe_name();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (recovered_tx, recovered_rx) = mpsc::channel();
        let server_pipe_name = pipe_name.clone();

        let server_thread = thread::spawn(move || -> Result<(), ProtocolError> {
            let mut server = NamedPipeServer::new(server_pipe_name);
            server.start(PipeSecurity::service_default())?;
            ready_tx
                .send(())
                .map_err(|_| ProtocolError::TransportUnavailable)?;

            assert_eq!(server.receive(), Err(ProtocolError::TransportUnavailable));
            recovered_tx
                .send(())
                .map_err(|_| ProtocolError::TransportUnavailable)?;

            let request = server.receive()?;
            assert_eq!(request, ServiceRequest::HealthCheck);
            server.send(ServiceEvent::HealthOk)?;
            server.stop();
            Ok(())
        });

        ready_rx
            .recv()
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        let mut broken_client = NamedPipeClient::new(pipe_name.clone());
        broken_client.connect()?;
        broken_client.disconnect();
        recovered_rx
            .recv()
            .map_err(|_| ProtocolError::TransportUnavailable)?;

        let mut client = NamedPipeClient::new(pipe_name);
        client.connect()?;
        let response = client.request(ServiceRequest::HealthCheck)?;
        client.disconnect();

        let server_result = server_thread
            .join()
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        assert_eq!(server_result, Ok(()));
        assert_eq!(response, ServiceEvent::HealthOk);
        Ok(())
    }

    fn unique_pipe_name() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        format!(
            r"\\.\pipe\winfaceunlock-test-{}-{nanos}",
            std::process::id()
        )
    }
}
