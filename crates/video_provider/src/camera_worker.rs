use crate::camera_worker_protocol::{Reply, Request, read_header, write_frame, write_header};
use crate::{CameraId, VideoFrameProvider};
use std::io;

/// Entrypoint for the dedicated camera worker executable. No persistent output.
#[doc(hidden)]
pub fn run_camera_worker() -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut provider = None;
    loop {
        let request: Request = match read_header(&mut input) {
            Ok(request) => request,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        };
        match request {
            Request::Open {
                camera_id,
                config,
                backend,
            } => {
                let mut camera = crate::opencv_camera::InProcessCameraProvider::new(config);
                let result = match backend {
                    Some(backend) => camera.open_backend(&CameraId(camera_id), backend),
                    None => camera.open(&CameraId(camera_id)),
                };
                match result {
                    Ok(()) => {
                        provider = Some(camera);
                        write_header(&mut output, &Reply::Opened)?;
                    }
                    Err(error) => {
                        write_header(&mut output, &Reply::Failed(error))?;
                        return Ok(());
                    }
                }
            }
            Request::Read => {
                let result = provider
                    .as_mut()
                    .ok_or(crate::VideoError::CameraNotOpen)
                    .and_then(|camera| camera.read_frame());
                match result {
                    Ok(frame) => write_frame(&mut output, frame)?,
                    Err(error) => write_header(&mut output, &Reply::Failed(error))?,
                }
            }
            Request::Close => {
                // Release capture/driver resources before acknowledging shutdown.
                // Abrupt process termination is reserved for the parent's timeout.
                drop(provider.take());
                write_header(&mut output, &Reply::Closed)?;
                return Ok(());
            }
        }
    }
}
