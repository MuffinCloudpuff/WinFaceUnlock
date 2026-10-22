mod backend_policy;
mod camera_identity;
mod camera_worker;
mod camera_worker_protocol;
mod capture_open;
#[cfg(windows)]
mod com_bindings;
#[cfg(windows)]
mod device_path_bindings;
#[cfg(windows)]
mod dshow_bindings;
mod frame;
pub mod frame_filter;
mod isolated_camera;
#[cfg(windows)]
mod mf_bindings;
mod opencv_camera;
mod provider;
#[cfg(windows)]
mod worker_process_bindings;

pub mod image_utils;

pub use backend_policy::windows_camera_backend_order;
#[cfg(windows)]
pub use camera_identity::list_camera_sources;
pub use camera_identity::resolve_camera_id;
pub use camera_worker::run_camera_worker;
pub use frame::{PixelFormat, VideoFrame};
pub use isolated_camera::OpenCvCameraProvider;
pub use opencv_camera::{OpenCvCameraBackend, OpenCvCameraProviderConfig};
pub use provider::{CameraId, CameraInfo, VideoError, VideoFrameProvider};
