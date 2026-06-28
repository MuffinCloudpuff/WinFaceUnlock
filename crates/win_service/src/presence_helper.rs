use std::path::PathBuf;

use crate::screen_snapshot::{ScreenSnapshotError, capture_primary_screen_to_bmp};

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PresenceHelperRequest {
    CaptureScreenSnapshot {
        event_id: String,
        output_path: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PresenceHelperResponse {
    ScreenSnapshotCaptured {
        event_id: String,
        image_path: PathBuf,
        width: i32,
        height: i32,
    },
    ScreenSnapshotUnavailable {
        event_id: String,
        reason: PresenceHelperFailureReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PresenceHelperFailureReason {
    PlatformUnavailable,
    InvalidScreenSize,
    CaptureFailed,
}

impl From<ScreenSnapshotError> for PresenceHelperFailureReason {
    fn from(value: ScreenSnapshotError) -> Self {
        match value {
            ScreenSnapshotError::PlatformUnavailable => Self::PlatformUnavailable,
            ScreenSnapshotError::InvalidScreenSize => Self::InvalidScreenSize,
            ScreenSnapshotError::CaptureFailed => Self::CaptureFailed,
        }
    }
}

pub trait PresenceHelperClient {
    fn handle_request(&self, request: PresenceHelperRequest) -> PresenceHelperResponse;
}

pub struct LocalProcessPresenceHelper;

impl PresenceHelperClient for LocalProcessPresenceHelper {
    fn handle_request(&self, request: PresenceHelperRequest) -> PresenceHelperResponse {
        match request {
            PresenceHelperRequest::CaptureScreenSnapshot {
                event_id,
                output_path,
            } => match capture_primary_screen_to_bmp(&output_path) {
                Ok(snapshot) => PresenceHelperResponse::ScreenSnapshotCaptured {
                    event_id,
                    image_path: snapshot.image_path,
                    width: snapshot.width,
                    height: snapshot.height,
                },
                Err(error) => PresenceHelperResponse::ScreenSnapshotUnavailable {
                    event_id,
                    reason: error.into(),
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_request_round_trips_as_json() -> Result<(), serde_json::Error> {
        let request = PresenceHelperRequest::CaptureScreenSnapshot {
            event_id: "unknown-face-1".to_owned(),
            output_path: PathBuf::from(r"C:\audit\screen.bmp"),
        };

        let encoded = serde_json::to_vec(&request)?;
        let decoded: PresenceHelperRequest = serde_json::from_slice(&encoded)?;

        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn helper_failure_reason_maps_screen_snapshot_error() {
        assert_eq!(
            PresenceHelperFailureReason::from(ScreenSnapshotError::InvalidScreenSize),
            PresenceHelperFailureReason::InvalidScreenSize
        );
    }
}
