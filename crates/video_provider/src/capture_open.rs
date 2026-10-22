//! Bounded warm-up and backend fallback, independent of platform capture APIs.
use crate::{VideoError, VideoFrame};

const WARMUP_READ_ATTEMPTS: usize = 8;

pub(crate) fn open_readable_capture<B, C>(
    backends: impl IntoIterator<Item = B>,
    mut open: impl FnMut(B) -> Result<C, VideoError>,
    mut read: impl FnMut(&mut C) -> Result<VideoFrame, VideoError>,
    mut pause: impl FnMut(),
) -> Result<(C, VideoFrame), VideoError> {
    let mut last_error = VideoError::OpenFailed;
    for backend in backends {
        let Ok(mut capture) = open(backend) else {
            continue;
        };
        for attempt in 0..WARMUP_READ_ATTEMPTS {
            match read(&mut capture) {
                Ok(frame) => return Ok((capture, frame)),
                Err(error) => last_error = error,
            }
            if attempt + 1 < WARMUP_READ_ATTEMPTS {
                pause();
            }
        }
        // Drop the failed backend before opening the next one (exclusive cameras).
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PixelFormat;
    use std::{cell::Cell, rc::Rc};

    fn frame() -> VideoFrame {
        VideoFrame {
            width: 1,
            height: 1,
            format: PixelFormat::Gray8,
            data: vec![42],
        }
    }

    struct Capture {
        backend: usize,
        active: Rc<Cell<bool>>,
    }

    impl Drop for Capture {
        fn drop(&mut self) {
            self.active.set(false);
        }
    }

    #[test]
    fn unreadable_backend_is_released_before_fallback() -> Result<(), VideoError> {
        let active = Rc::new(Cell::new(false));
        let mut reads = 0;
        let (capture, first_frame) = open_readable_capture(
            [0, 1],
            |backend| {
                assert!(!active.replace(true));
                Ok(Capture {
                    backend,
                    active: Rc::clone(&active),
                })
            },
            |capture| {
                reads += 1;
                if capture.backend == 0 {
                    Err(VideoError::EmptyFrame)
                } else {
                    Ok(frame())
                }
            },
            || {},
        )?;
        assert_eq!(capture.backend, 1);
        assert_eq!(first_frame.data, vec![42]);
        assert_eq!(reads, WARMUP_READ_ATTEMPTS + 1);
        drop(capture);
        assert!(!active.get());
        Ok(())
    }

    #[test]
    fn transient_warmup_failure_does_not_switch_backend() -> Result<(), VideoError> {
        let mut opens = 0;
        let mut reads = 0;
        let (backend, _) = open_readable_capture(
            [0, 1],
            |backend| {
                opens += 1;
                Ok(backend)
            },
            |_| {
                reads += 1;
                if reads < 3 {
                    Err(VideoError::ReadFailed)
                } else {
                    Ok(frame())
                }
            },
            || {},
        )?;
        assert_eq!((backend, opens, reads), (0, 1, 3));
        Ok(())
    }

    #[test]
    fn exhausted_backends_return_error_after_bounded_reads() {
        let mut reads = 0;
        let result = open_readable_capture(
            [0, 1],
            Ok,
            |_| {
                reads += 1;
                Err(VideoError::ReadFailed)
            },
            || {},
        );
        assert!(matches!(result, Err(VideoError::ReadFailed)));
        assert_eq!(reads, 2 * WARMUP_READ_ATTEMPTS);
    }
}
