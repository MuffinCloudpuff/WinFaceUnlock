//! Private pipe protocol. Frames travel as bounded binary payloads, never files.
use crate::{OpenCvCameraBackend, OpenCvCameraProviderConfig, PixelFormat, VideoError, VideoFrame};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{self, Read, Write};

const MAX_HEADER: usize = 64 * 1024;
const MAX_FRAME: usize = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub(crate) enum Request {
    Open {
        camera_id: String,
        config: OpenCvCameraProviderConfig,
        backend: Option<OpenCvCameraBackend>,
    },
    Read,
    Close,
}

#[derive(Serialize, Deserialize)]
pub(crate) enum Reply {
    Opened,
    Closed,
    Frame {
        width: u32,
        height: u32,
        format: PixelFormat,
        length: usize,
    },
    Failed(VideoError),
}

#[derive(Debug, PartialEq)]
pub(crate) enum WorkerResponse {
    Opened,
    Frame(VideoFrame),
    Closed,
}

pub(crate) fn write_header(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_HEADER {
        return Err(io::ErrorKind::InvalidData.into());
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

pub(crate) fn read_header<T: DeserializeOwned>(reader: &mut impl Read) -> io::Result<T> {
    let mut size = [0; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    if size > MAX_HEADER {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub(crate) fn write_frame(writer: &mut impl Write, frame: VideoFrame) -> io::Result<()> {
    if frame.validate().is_err() || frame.data.len() > MAX_FRAME {
        return Err(io::ErrorKind::InvalidData.into());
    }
    write_header(
        writer,
        &Reply::Frame {
            width: frame.width,
            height: frame.height,
            format: frame.format,
            length: frame.data.len(),
        },
    )?;
    writer.write_all(&frame.data)?;
    writer.flush()
}

// Protocol failures invalidate the transport; camera failures may be retried.
pub(crate) fn read_reply(reader: &mut impl Read) -> io::Result<Result<WorkerResponse, VideoError>> {
    match read_header(reader)? {
        Reply::Opened => Ok(Ok(WorkerResponse::Opened)),
        Reply::Closed => Ok(Ok(WorkerResponse::Closed)),
        Reply::Failed(error) => Ok(Err(error)),
        Reply::Frame {
            width,
            height,
            format,
            length,
        } => {
            let expected = (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(format.channel_count()));
            if expected != Some(length) || length == 0 || length > MAX_FRAME {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let mut data = vec![0; length];
            reader.read_exact(&mut data)?;
            let frame = VideoFrame {
                width,
                height,
                format,
                data,
            };
            frame.validate().map_err(|_| io::ErrorKind::InvalidData)?;
            Ok(Ok(WorkerResponse::Frame(frame)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_frames_round_trip() -> io::Result<()> {
        let frame = VideoFrame {
            width: 2,
            height: 1,
            format: PixelFormat::Bgr8,
            data: vec![1, 2, 3, 4, 5, 6],
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, frame.clone())?;
        assert_eq!(
            read_reply(&mut bytes.as_slice())?,
            Ok(WorkerResponse::Frame(frame))
        );
        Ok(())
    }
    #[test]
    fn closed_is_distinct_from_opened_and_frame() -> io::Result<()> {
        let mut bytes = Vec::new();
        write_header(&mut bytes, &Reply::Closed)?;
        assert_eq!(
            read_reply(&mut bytes.as_slice())?,
            Ok(WorkerResponse::Closed)
        );
        Ok(())
    }
    #[test]
    fn reject_oversized_and_inconsistent_frames_before_allocation() -> io::Result<()> {
        for (width, length) in [(u32::MAX, usize::MAX), (1, 4), (0, 0)] {
            let mut bytes = Vec::new();
            write_header(
                &mut bytes,
                &Reply::Frame {
                    width,
                    height: 1,
                    format: PixelFormat::Gray8,
                    length,
                },
            )?;
            assert_eq!(
                read_reply(&mut bytes.as_slice())
                    .err()
                    .map(|error| error.kind()),
                Some(io::ErrorKind::InvalidData)
            );
        }
        assert!(read_header::<Reply>(&mut u32::MAX.to_le_bytes().as_slice()).is_err());
        Ok(())
    }
    #[test]
    fn camera_failure_remains_distinct_from_corrupt_transport() -> io::Result<()> {
        let mut bytes = Vec::new();
        write_header(&mut bytes, &Reply::Failed(VideoError::ReadFailed))?;
        assert_eq!(
            read_reply(&mut bytes.as_slice())?,
            Err(VideoError::ReadFailed)
        );
        assert!(read_reply(&mut &b"broken"[..]).is_err());
        Ok(())
    }
}
