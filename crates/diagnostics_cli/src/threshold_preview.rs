use std::{fmt, thread, time::Duration};

use face_liveness::{ScreenReplayPreprocessConfig, build_screen_replay_binary_mask_frame};
use minifb::{Key, Window, WindowOptions};
use opencv::{
    core::{AlgorithmHint, Mat, MatTraitConst, MatTraitConstManual},
    imgproc,
};
use video_provider::{
    CameraId, OpenCvCameraProvider, OpenCvCameraProviderConfig, PixelFormat, VideoError,
    VideoFrame, VideoFrameProvider,
};

#[derive(Clone, Debug)]
pub struct ThresholdPreviewConfig {
    pub camera_id: Option<CameraId>,
    pub max_camera_index: u32,
    pub requested_frame_width: Option<u32>,
    pub requested_frame_height: Option<u32>,
    pub method: ThresholdPreviewMethod,
    pub adaptive_block_size: i32,
    pub adaptive_c: f64,
    pub binary_threshold: f64,
    pub binary_mask_upper_threshold: f64,
    pub frame_delay_ms: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThresholdPreviewMethod {
    BinaryInvertedMask,
    AdaptiveGaussian,
    AdaptiveMean,
    Otsu,
}

#[derive(Debug)]
pub enum ThresholdPreviewError {
    Video(VideoError),
    OpenCvFailed,
    PreviewWindowFailed,
    InvalidArgument,
}

impl fmt::Display for ThresholdPreviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Video(error) => write!(formatter, "video error: {error:?}"),
            Self::OpenCvFailed => write!(formatter, "opencv preview failed"),
            Self::PreviewWindowFailed => write!(formatter, "preview window failed"),
            Self::InvalidArgument => write!(formatter, "invalid threshold preview argument"),
        }
    }
}

impl From<VideoError> for ThresholdPreviewError {
    fn from(value: VideoError) -> Self {
        Self::Video(value)
    }
}

pub fn run_threshold_preview(config: ThresholdPreviewConfig) -> Result<(), ThresholdPreviewError> {
    let block_size = normalized_adaptive_block_size(config.adaptive_block_size)?;
    let mut camera_provider = OpenCvCameraProvider::new(OpenCvCameraProviderConfig {
        max_camera_index: config.max_camera_index,
        requested_frame_width: config.requested_frame_width,
        requested_frame_height: config.requested_frame_height,
        preferred_backend: None,
    });
    let sources = camera_provider.list_sources()?;
    let camera_id = selected_camera_id(config.camera_id, &sources)?;
    camera_provider.open(&camera_id)?;

    println!("threshold_preview_started: true");
    println!("camera_id: {}", camera_id.0);
    println!("method: {:?}", config.method);
    println!("adaptive_block_size: {block_size}");
    println!("adaptive_c: {}", config.adaptive_c);
    println!("binary_threshold: {}", config.binary_threshold);
    println!(
        "binary_mask_upper_threshold: {}",
        config.binary_mask_upper_threshold
    );
    println!("press q or Esc in the preview window to exit");

    let first_frame = camera_provider.read_frame()?;
    let preview_width = first_frame.width as usize * 2;
    let preview_height = first_frame.height as usize;
    let mut window = Window::new(
        "WinFaceUnlock OpenCV auto-threshold preview",
        preview_width,
        preview_height,
        WindowOptions {
            resize: true,
            ..WindowOptions::default()
        },
    )
    .map_err(|_| ThresholdPreviewError::PreviewWindowFailed)?;
    render_frame(
        &first_frame,
        &config.method,
        block_size,
        config.adaptive_c,
        config.binary_threshold,
        config.binary_mask_upper_threshold,
        &mut window,
    )?;

    while window.is_open() && !window.is_key_down(Key::Escape) && !window.is_key_down(Key::Q) {
        let frame = camera_provider.read_frame()?;
        render_frame(
            &frame,
            &config.method,
            block_size,
            config.adaptive_c,
            config.binary_threshold,
            config.binary_mask_upper_threshold,
            &mut window,
        )?;
        if config.frame_delay_ms > 0 {
            thread::sleep(Duration::from_millis(u64::from(config.frame_delay_ms)));
        }
    }

    camera_provider.close();
    println!("threshold_preview_closed: true");
    Ok(())
}

fn render_frame(
    frame: &VideoFrame,
    method: &ThresholdPreviewMethod,
    adaptive_block_size: i32,
    adaptive_c: f64,
    binary_threshold: f64,
    binary_mask_upper_threshold: f64,
    window: &mut Window,
) -> Result<(), ThresholdPreviewError> {
    let original = video_frame_to_mat(frame)?;
    let gray = grayscale_mat(&original, frame.format.clone())?;
    let thresholded = threshold_mat(
        &gray,
        method,
        adaptive_block_size,
        adaptive_c,
        binary_threshold,
        binary_mask_upper_threshold,
    )?;
    let preview_buffer = side_by_side_preview_buffer(frame, &thresholded)?;

    window
        .update_with_buffer(
            &preview_buffer,
            frame.width as usize * 2,
            frame.height as usize,
        )
        .map_err(|_| ThresholdPreviewError::PreviewWindowFailed)
}

fn normalized_adaptive_block_size(block_size: i32) -> Result<i32, ThresholdPreviewError> {
    if block_size < 3 {
        return Err(ThresholdPreviewError::InvalidArgument);
    }
    if block_size % 2 == 0 {
        Ok(block_size + 1)
    } else {
        Ok(block_size)
    }
}

fn selected_camera_id(
    requested_camera_id: Option<CameraId>,
    sources: &[video_provider::CameraInfo],
) -> Result<CameraId, ThresholdPreviewError> {
    if let Some(camera_id) = requested_camera_id {
        return Ok(camera_id);
    }

    sources
        .first()
        .map(|source| source.id.clone())
        .ok_or(ThresholdPreviewError::Video(VideoError::CameraNotFound))
}

fn video_frame_to_mat(frame: &VideoFrame) -> Result<Mat, ThresholdPreviewError> {
    frame
        .validate()
        .map_err(|_| ThresholdPreviewError::Video(VideoError::UnsupportedFormat))?;
    let channels = match frame.format {
        PixelFormat::Bgr8 | PixelFormat::Rgb8 => 3,
        PixelFormat::Gray8 => 1,
    };
    let mat = Mat::from_slice(&frame.data).map_err(|_| ThresholdPreviewError::OpenCvFailed)?;
    let mat = mat
        .reshape(channels, frame.height as i32)
        .map_err(|_| ThresholdPreviewError::OpenCvFailed)?;
    mat.try_clone()
        .map_err(|_| ThresholdPreviewError::OpenCvFailed)
}

fn grayscale_mat(original: &Mat, pixel_format: PixelFormat) -> Result<Mat, ThresholdPreviewError> {
    if pixel_format == PixelFormat::Gray8 {
        return original
            .try_clone()
            .map_err(|_| ThresholdPreviewError::OpenCvFailed);
    }

    let mut gray = Mat::default();
    let color_conversion = match pixel_format {
        PixelFormat::Bgr8 => imgproc::COLOR_BGR2GRAY,
        PixelFormat::Rgb8 => imgproc::COLOR_RGB2GRAY,
        PixelFormat::Gray8 => imgproc::COLOR_BGR2GRAY,
    };
    imgproc::cvt_color(
        original,
        &mut gray,
        color_conversion,
        0,
        AlgorithmHint::ALGO_HINT_DEFAULT,
    )
    .map_err(|_| ThresholdPreviewError::OpenCvFailed)?;
    Ok(gray)
}

fn threshold_mat(
    gray: &Mat,
    method: &ThresholdPreviewMethod,
    adaptive_block_size: i32,
    adaptive_c: f64,
    binary_threshold: f64,
    binary_mask_upper_threshold: f64,
) -> Result<Mat, ThresholdPreviewError> {
    let mut thresholded = Mat::default();
    match method {
        ThresholdPreviewMethod::BinaryInvertedMask => {
            let gray_frame = gray_mat_to_video_frame(gray)?;
            let mask_frame = build_screen_replay_binary_mask_frame(
                &gray_frame,
                &ScreenReplayPreprocessConfig {
                    binary_threshold,
                    binary_mask_upper_threshold,
                },
            )
            .map_err(|_| ThresholdPreviewError::OpenCvFailed)?;
            return video_frame_to_mat(&mask_frame);
        }
        ThresholdPreviewMethod::AdaptiveGaussian => imgproc::adaptive_threshold(
            gray,
            &mut thresholded,
            255.0,
            imgproc::ADAPTIVE_THRESH_GAUSSIAN_C,
            imgproc::THRESH_BINARY,
            adaptive_block_size,
            adaptive_c,
        )
        .map_err(|_| ThresholdPreviewError::OpenCvFailed)?,
        ThresholdPreviewMethod::AdaptiveMean => imgproc::adaptive_threshold(
            gray,
            &mut thresholded,
            255.0,
            imgproc::ADAPTIVE_THRESH_MEAN_C,
            imgproc::THRESH_BINARY,
            adaptive_block_size,
            adaptive_c,
        )
        .map_err(|_| ThresholdPreviewError::OpenCvFailed)?,
        ThresholdPreviewMethod::Otsu => {
            imgproc::threshold(
                gray,
                &mut thresholded,
                0.0,
                255.0,
                imgproc::THRESH_BINARY | imgproc::THRESH_OTSU,
            )
            .map_err(|_| ThresholdPreviewError::OpenCvFailed)?;
        }
    }
    Ok(thresholded)
}

fn gray_mat_to_video_frame(gray: &Mat) -> Result<VideoFrame, ThresholdPreviewError> {
    let data = gray
        .data_bytes()
        .map_err(|_| ThresholdPreviewError::OpenCvFailed)?
        .to_vec();
    let frame = VideoFrame {
        width: gray.cols() as u32,
        height: gray.rows() as u32,
        format: PixelFormat::Gray8,
        data,
    };
    frame
        .validate()
        .map_err(|_| ThresholdPreviewError::OpenCvFailed)?;
    Ok(frame)
}

fn side_by_side_preview_buffer(
    frame: &VideoFrame,
    thresholded: &Mat,
) -> Result<Vec<u32>, ThresholdPreviewError> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let threshold_bytes = thresholded
        .data_bytes()
        .map_err(|_| ThresholdPreviewError::OpenCvFailed)?;
    let mut buffer = vec![0_u32; width * 2 * height];

    for y in 0..height {
        for x in 0..width {
            let original_color = original_pixel_to_rgb_u32(frame, x, y);
            let threshold_value = threshold_bytes[y * width + x];
            let threshold_color = gray_to_rgb_u32(threshold_value);
            let row_start = y * width * 2;
            buffer[row_start + x] = original_color;
            buffer[row_start + width + x] = threshold_color;
        }
    }

    Ok(buffer)
}

fn original_pixel_to_rgb_u32(frame: &VideoFrame, x: usize, y: usize) -> u32 {
    let width = frame.width as usize;
    match frame.format {
        PixelFormat::Bgr8 => {
            let index = (y * width + x) * 3;
            rgb_to_u32(
                frame.data[index + 2],
                frame.data[index + 1],
                frame.data[index],
            )
        }
        PixelFormat::Rgb8 => {
            let index = (y * width + x) * 3;
            rgb_to_u32(
                frame.data[index],
                frame.data[index + 1],
                frame.data[index + 2],
            )
        }
        PixelFormat::Gray8 => gray_to_rgb_u32(frame.data[y * width + x]),
    }
}

fn gray_to_rgb_u32(value: u8) -> u32 {
    rgb_to_u32(value, value, value)
}

fn rgb_to_u32(red: u8, green: u8, blue: u8) -> u32 {
    (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_block_size_keeps_odd_value() -> Result<(), ThresholdPreviewError> {
        assert_eq!(normalized_adaptive_block_size(21)?, 21);
        Ok(())
    }

    #[test]
    fn normalized_block_size_rounds_even_value_to_next_odd() -> Result<(), ThresholdPreviewError> {
        assert_eq!(normalized_adaptive_block_size(20)?, 21);
        Ok(())
    }

    #[test]
    fn normalized_block_size_rejects_too_small_value() {
        assert!(matches!(
            normalized_adaptive_block_size(1),
            Err(ThresholdPreviewError::InvalidArgument)
        ));
    }
}
