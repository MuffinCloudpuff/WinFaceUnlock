use common_protocol::{ProtocolError, ServiceEvent, ServiceRequest};
use serde::{Serialize, de::DeserializeOwned};

const MAX_FRAME_BYTES: usize = 64 * 1024;

pub fn encode_request(request: &ServiceRequest) -> Result<Vec<u8>, ProtocolError> {
    encode_json_frame(request)
}

pub fn decode_request(frame: &[u8]) -> Result<ServiceRequest, ProtocolError> {
    decode_json_frame(frame)
}

pub fn encode_event(event: &ServiceEvent) -> Result<Vec<u8>, ProtocolError> {
    encode_json_frame(event)
}

pub fn decode_event(frame: &[u8]) -> Result<ServiceEvent, ProtocolError> {
    decode_json_frame(frame)
}

fn encode_json_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    let payload = serde_json::to_vec(value).map_err(|_| ProtocolError::InvalidMessage)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::InvalidMessage);
    }

    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn decode_json_frame<T: DeserializeOwned>(frame: &[u8]) -> Result<T, ProtocolError> {
    if frame.len() < 4 {
        return Err(ProtocolError::InvalidMessage);
    }

    let mut length_bytes = [0_u8; 4];
    length_bytes.copy_from_slice(&frame[..4]);
    let payload_len = u32::from_le_bytes(length_bytes) as usize;
    if payload_len > MAX_FRAME_BYTES || frame.len() != 4 + payload_len {
        return Err(ProtocolError::InvalidMessage);
    }

    serde_json::from_slice(&frame[4..]).map_err(|_| ProtocolError::InvalidMessage)
}

#[cfg(test)]
mod tests {
    use common_protocol::{AuthSource, ServiceRequest, SessionId};

    use super::*;

    #[test]
    fn request_codec_round_trips_structured_message() -> Result<(), ProtocolError> {
        let request = ServiceRequest::WakeAuth {
            session_id: SessionId("session-1".to_owned()),
            source: AuthSource::LocalCamera,
        };

        let frame = encode_request(&request)?;
        let decoded = decode_request(&frame)?;

        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn request_codec_rejects_truncated_frame() {
        let result = decode_request(&[10, 0, 0, 0, b'{']);

        assert_eq!(result, Err(ProtocolError::InvalidMessage));
    }
}
