use video_provider::{VideoError, VideoFrame};

pub const DEFAULT_MAX_CONSECUTIVE_TRANSIENT_FRAME_FAILURES: u32 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransientFrameFailureKind {
    EmptyFrame,
    ReadFailed,
    InvalidFrame,
}

impl TransientFrameFailureKind {
    pub fn from_video_error(error: VideoError) -> Option<Self> {
        match error {
            VideoError::EmptyFrame => Some(Self::EmptyFrame),
            VideoError::ReadFailed => Some(Self::ReadFailed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransientFrameFailureDecision {
    RetryNextFrame {
        consecutive_failures: u32,
        max_consecutive_failures: u32,
    },
    Escalate {
        consecutive_failures: u32,
        max_consecutive_failures: u32,
    },
}

#[derive(Clone, Debug)]
pub struct TransientFrameFailureTolerance {
    max_consecutive_failures: u32,
    consecutive_failures: u32,
}

impl TransientFrameFailureTolerance {
    pub fn new(max_consecutive_failures: u32) -> Self {
        Self {
            max_consecutive_failures: max_consecutive_failures.max(1),
            consecutive_failures: 0,
        }
    }

    pub fn default_for_camera_stream() -> Self {
        Self::new(DEFAULT_MAX_CONSECUTIVE_TRANSIENT_FRAME_FAILURES)
    }

    pub fn record_valid_frame(&mut self) {
        self.consecutive_failures = 0;
    }

    pub fn record_transient_failure(
        &mut self,
        _kind: TransientFrameFailureKind,
    ) -> TransientFrameFailureDecision {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= self.max_consecutive_failures {
            TransientFrameFailureDecision::Escalate {
                consecutive_failures: self.consecutive_failures,
                max_consecutive_failures: self.max_consecutive_failures,
            }
        } else {
            TransientFrameFailureDecision::RetryNextFrame {
                consecutive_failures: self.consecutive_failures,
                max_consecutive_failures: self.max_consecutive_failures,
            }
        }
    }
}

pub fn validate_frame_for_camera_stream(
    frame: &VideoFrame,
) -> Result<(), TransientFrameFailureKind> {
    frame.validate().map_err(|error| match error {
        VideoError::EmptyFrame => TransientFrameFailureKind::EmptyFrame,
        VideoError::ReadFailed => TransientFrameFailureKind::ReadFailed,
        _ => TransientFrameFailureKind::InvalidFrame,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use video_provider::PixelFormat;

    #[test]
    fn transient_video_errors_are_classified() {
        assert_eq!(
            TransientFrameFailureKind::from_video_error(VideoError::EmptyFrame),
            Some(TransientFrameFailureKind::EmptyFrame)
        );
        assert_eq!(
            TransientFrameFailureKind::from_video_error(VideoError::ReadFailed),
            Some(TransientFrameFailureKind::ReadFailed)
        );
        assert_eq!(
            TransientFrameFailureKind::from_video_error(VideoError::OpenFailed),
            None
        );
    }

    #[test]
    fn tolerance_retries_until_consecutive_failure_limit() {
        let mut tolerance = TransientFrameFailureTolerance::new(3);

        assert_eq!(
            tolerance.record_transient_failure(TransientFrameFailureKind::EmptyFrame),
            TransientFrameFailureDecision::RetryNextFrame {
                consecutive_failures: 1,
                max_consecutive_failures: 3
            }
        );
        assert_eq!(
            tolerance.record_transient_failure(TransientFrameFailureKind::InvalidFrame),
            TransientFrameFailureDecision::RetryNextFrame {
                consecutive_failures: 2,
                max_consecutive_failures: 3
            }
        );
        assert_eq!(
            tolerance.record_transient_failure(TransientFrameFailureKind::ReadFailed),
            TransientFrameFailureDecision::Escalate {
                consecutive_failures: 3,
                max_consecutive_failures: 3
            }
        );
    }

    #[test]
    fn valid_frame_resets_consecutive_failures() {
        let mut tolerance = TransientFrameFailureTolerance::new(2);
        let _ = tolerance.record_transient_failure(TransientFrameFailureKind::EmptyFrame);
        tolerance.record_valid_frame();

        assert_eq!(
            tolerance.record_transient_failure(TransientFrameFailureKind::EmptyFrame),
            TransientFrameFailureDecision::RetryNextFrame {
                consecutive_failures: 1,
                max_consecutive_failures: 2
            }
        );
    }

    #[test]
    fn frame_validation_reports_invalid_stream_frame() {
        let frame = VideoFrame {
            width: 2,
            height: 2,
            format: PixelFormat::Bgr8,
            data: vec![0; 3],
        };

        assert_eq!(
            validate_frame_for_camera_stream(&frame),
            Err(TransientFrameFailureKind::InvalidFrame)
        );
    }
}
