//! Camera operations run in a disposable process, with parent-enforced deadlines.
use crate::camera_worker_protocol::{Request, WorkerResponse, read_reply, write_header};
use crate::{
    CameraId, CameraInfo, OpenCvCameraBackend, OpenCvCameraProviderConfig, VideoError, VideoFrame,
    VideoFrameProvider,
};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const OPEN_TIMEOUT: Duration = Duration::from_secs(8);
const READ_TIMEOUT: Duration = Duration::from_secs(3);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

struct WorkerSession {
    child: Child,
    requests: Option<mpsc::Sender<Request>>,
    replies: mpsc::Receiver<std::io::Result<Result<WorkerResponse, VideoError>>>,
    #[cfg(windows)]
    job: Option<crate::worker_process_bindings::WorkerJob>,
}

impl WorkerSession {
    fn spawn(mut command: Command) -> Result<Self, VideoError> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command
            .env("OPENCV_LOG_LEVEL", "SILENT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| VideoError::ProviderUnavailable)?;
        #[cfg(windows)]
        let job = match crate::worker_process_bindings::WorkerJob::attach(&child) {
            Ok(job) => job,
            Err(_) => {
                let _ = child.kill();
                return Err(VideoError::ProviderUnavailable);
            }
        };
        let (requests, receiver) = mpsc::channel::<Request>();
        let (sender, replies) = mpsc::channel();
        let mut stdin = child.stdin.take().ok_or(VideoError::ProviderUnavailable)?;
        let mut stdout = child.stdout.take().ok_or(VideoError::ProviderUnavailable)?;
        if thread::Builder::new()
            .name("camera-worker-transport".to_owned())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let reply =
                        write_header(&mut stdin, &request).and_then(|_| read_reply(&mut stdout));
                    let broken = reply.is_err();
                    if sender.send(reply).is_err() || broken {
                        break;
                    }
                }
            })
            .is_err()
        {
            let _ = child.kill();
            return Err(VideoError::ProviderUnavailable);
        }
        Ok(Self {
            child,
            requests: Some(requests),
            replies,
            #[cfg(windows)]
            job: Some(job),
        })
    }

    fn exchange(
        &mut self,
        request: Request,
        timeout: Duration,
    ) -> Result<WorkerResponse, VideoError> {
        let result = self
            .requests
            .as_ref()
            .ok_or(VideoError::CameraNotOpen)?
            .send(request)
            .map_err(|_| VideoError::ReadFailed)
            .and_then(|_| {
                self.replies
                    .recv_timeout(timeout)
                    .map_err(|_| VideoError::ReadFailed)
            })
            .and_then(|reply| reply.map_err(|_| VideoError::ReadFailed));
        if result.is_err() {
            self.terminate();
        }
        result.and_then(|reply| reply)
    }

    fn shutdown(&mut self) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            self.requests.take();
            return;
        }
        if self.requests.is_some()
            && matches!(
                self.exchange(Request::Close, CLOSE_TIMEOUT),
                Ok(WorkerResponse::Closed)
            )
        {
            self.requests.take();
            let deadline = Instant::now() + Duration::from_millis(250);
            while Instant::now() < deadline {
                if matches!(self.child.try_wait(), Ok(Some(_))) {
                    return;
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
        self.terminate();
    }

    fn terminate(&mut self) {
        self.requests.take();
        #[cfg(windows)]
        {
            self.job.take();
        }
        let _ = self.child.kill();
        let deadline = Instant::now() + Duration::from_millis(250);
        // No unbounded wait on a driver stuck in the kernel.
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for WorkerSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub struct OpenCvCameraProvider {
    config: OpenCvCameraProviderConfig,
    worker: Option<WorkerSession>,
    opened_backend: Option<OpenCvCameraBackend>,
}

impl OpenCvCameraProvider {
    pub fn new(config: OpenCvCameraProviderConfig) -> Self {
        Self {
            config,
            worker: None,
            opened_backend: None,
        }
    }
    pub fn with_default_config() -> Self {
        Self::new(OpenCvCameraProviderConfig::default())
    }
    pub fn opened_backend(&self) -> Option<OpenCvCameraBackend> {
        self.opened_backend
    }
    pub fn open_backend(
        &mut self,
        camera_id: &CameraId,
        backend: OpenCvCameraBackend,
    ) -> Result<(), VideoError> {
        if self.worker.is_some() {
            return Err(VideoError::CameraAlreadyOpen);
        }
        let camera_id = bind_for_open(camera_id)?;
        self.open_worker(&camera_id, Some(backend))
    }
    fn open_worker(
        &mut self,
        camera_id: &CameraId,
        backend: Option<OpenCvCameraBackend>,
    ) -> Result<(), VideoError> {
        self.open_with_command(camera_id, backend, Command::new(worker_path()?))
    }
    fn open_with_command(
        &mut self,
        camera_id: &CameraId,
        backend: Option<OpenCvCameraBackend>,
        command: Command,
    ) -> Result<(), VideoError> {
        if self.worker.is_some() {
            return Err(VideoError::CameraAlreadyOpen);
        }
        let mut worker = WorkerSession::spawn(command)?;
        match worker.exchange(
            Request::Open {
                camera_id: camera_id.0.clone(),
                config: self.config.clone(),
                backend,
            },
            OPEN_TIMEOUT,
        )? {
            WorkerResponse::Opened => {
                self.worker = Some(worker);
                self.opened_backend = backend;
                Ok(())
            }
            _ => Err(VideoError::OpenFailed),
        }
    }
}
impl Default for OpenCvCameraProvider {
    fn default() -> Self {
        Self::with_default_config()
    }
}
impl VideoFrameProvider for OpenCvCameraProvider {
    fn list_sources(&self) -> Result<Vec<CameraInfo>, VideoError> {
        crate::opencv_camera::InProcessCameraProvider::new(self.config.clone()).list_sources()
    }
    fn open(&mut self, camera_id: &CameraId) -> Result<(), VideoError> {
        if self.worker.is_some() {
            return Err(VideoError::CameraAlreadyOpen);
        }
        // Resolve a legacy index only once: fallback must keep the same device
        // even if the first backend times out while USB ordering changes.
        let camera_id = bind_for_open(camera_id)?;
        #[cfg(windows)]
        let backends = crate::windows_camera_backend_order(self.config.preferred_backend);
        #[cfg(not(windows))]
        let backends = [OpenCvCameraBackend::Any];
        let mut last_error = VideoError::OpenFailed;
        for backend in backends {
            match self.open_worker(&camera_id, Some(backend)) {
                Ok(()) => return Ok(()),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }
    fn read_frame(&mut self) -> Result<VideoFrame, VideoError> {
        let result = self
            .worker
            .as_mut()
            .ok_or(VideoError::CameraNotOpen)?
            .exchange(Request::Read, READ_TIMEOUT)
            .and_then(|reply| match reply {
                WorkerResponse::Frame(frame) => Ok(frame),
                _ => Err(VideoError::ReadFailed),
            });
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.requests.is_none())
        {
            self.close();
        }
        result
    }
    fn close(&mut self) {
        self.worker.take();
        self.opened_backend = None;
    }
}

fn bind_for_open(camera_id: &CameraId) -> Result<CameraId, VideoError> {
    if camera_id.is_stable() {
        camera_id.normalized()
    } else {
        crate::resolve_camera_id(camera_id)
    }
}

fn worker_path() -> Result<PathBuf, VideoError> {
    let executable = std::env::current_exe().map_err(|_| VideoError::ProviderUnavailable)?;
    let directory = executable.parent().ok_or(VideoError::ProviderUnavailable)?;
    let name = if cfg!(windows) {
        "camera_worker.exe"
    } else {
        "camera_worker"
    };
    let direct = directory.join(name);
    if direct.is_file() {
        return Ok(direct);
    }
    // Cargo test executables live in target/<profile>/deps.
    if directory.file_name().is_some_and(|name| name == "deps") {
        let sibling = directory
            .parent()
            .ok_or(VideoError::ProviderUnavailable)?
            .join(name);
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    Err(VideoError::ProviderUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    fn fake_worker(script: &str) -> Command {
        let mut command =
            Command::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        command
    }
    #[cfg(windows)]
    #[test]
    fn normal_shutdown_waits_for_camera_release_and_clean_exit() -> Result<(), VideoError> {
        let command = fake_worker(
            "$i = [IO.BinaryReader]::new([Console]::OpenStandardInput()); $n = $i.ReadInt32(); $s = [Text.Encoding]::UTF8.GetString($i.ReadBytes($n)); if ($s -ne '\"Close\"') { exit 9 }; Start-Sleep -Milliseconds 50; $o = [Console]::OpenStandardOutput(); $b = [byte[]](8,0,0,0,34,67,108,111,115,101,100,34); $o.Write($b,0,$b.Length); $o.Flush(); exit 0",
        );
        let mut worker = WorkerSession::spawn(command)?;
        worker.shutdown();
        assert!(
            worker
                .child
                .try_wait()
                .map_err(|_| VideoError::ReadFailed)?
                .is_some_and(|status| status.success())
        );
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn hung_close_still_terminates_within_deadline() -> Result<(), VideoError> {
        let command = fake_worker(
            "$null = [Console]::OpenStandardInput().ReadByte(); Start-Sleep -Seconds 30",
        );
        let mut worker = WorkerSession::spawn(command)?;
        let started = Instant::now();
        worker.shutdown();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(
            worker
                .child
                .try_wait()
                .map_err(|_| VideoError::ReadFailed)?
                .is_some()
        );
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn hung_worker_is_terminated_and_supervisor_returns() -> Result<(), VideoError> {
        let mut command =
            Command::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
        // No descendants and no work before the first request: job assignment
        // always precedes the simulated driver hang.
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$null = [Console]::OpenStandardInput().ReadByte(); Start-Sleep -Seconds 30",
        ]);
        let mut worker = WorkerSession::spawn(command)?;
        let started = Instant::now();
        assert!(
            worker
                .exchange(Request::Read, Duration::from_millis(100))
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(
            worker
                .child
                .try_wait()
                .map_err(|_| VideoError::ReadFailed)?
                .is_some()
        );
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn corrupt_transport_closes_provider_and_allows_reopen() -> Result<(), VideoError> {
        let bad = fake_worker(
            "$null = [Console]::OpenStandardInput().ReadByte(); $o = [Console]::OpenStandardOutput(); $b = [byte[]](255,255,255,255); $o.Write($b,0,4); $o.Flush(); Start-Sleep -Seconds 30",
        );
        let mut provider = OpenCvCameraProvider {
            worker: Some(WorkerSession::spawn(bad)?),
            ..Default::default()
        };
        assert_eq!(provider.read_frame(), Err(VideoError::ReadFailed));
        assert!(provider.worker.is_none());
        let good = fake_worker(
            "$null = [Console]::OpenStandardInput().ReadByte(); $o = [Console]::OpenStandardOutput(); $b = [byte[]](8,0,0,0,34,79,112,101,110,101,100,34); $o.Write($b,0,$b.Length); $o.Flush(); Start-Sleep -Seconds 30",
        );
        provider.open_with_command(&CameraId::from_index(0), None, good)?;
        assert!(provider.worker.is_some());
        provider.close();
        Ok(())
    }
}
