use opencv::{
    core::{self, AlgorithmHint, Mat, MatTraitConst, MatTraitConstManual, Scalar},
    imgproc,
};
use video_provider::{PixelFormat, VideoFrame};

use crate::LivenessProviderError;

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenReplayPreprocessConfig {
    pub binary_threshold: f64,
    pub binary_mask_upper_threshold: f64,
}

impl Default for ScreenReplayPreprocessConfig {
    fn default() -> Self {
        Self {
            binary_threshold: 150.0,
            binary_mask_upper_threshold: 50.0,
        }
    }
}

pub(crate) struct ScreenReplayPreprocessedFrame {
    pub gray: Mat,
    pub binary_mask: Mat,
}

pub(crate) fn preprocess_screen_replay_frame(
    frame: &VideoFrame,
    config: &ScreenReplayPreprocessConfig,
) -> Result<ScreenReplayPreprocessedFrame, LivenessProviderError> {
    frame
        .validate()
        .map_err(|_| LivenessProviderError::InvalidFrame)?;
    let image = video_frame_to_mat(frame)?;
    let gray = grayscale_mat(&image, frame.format.clone())?;
    let binary_mask = build_binary_mask(&gray, config)?;
    Ok(ScreenReplayPreprocessedFrame { gray, binary_mask })
}

pub fn build_screen_replay_binary_mask_frame(
    frame: &VideoFrame,
    config: &ScreenReplayPreprocessConfig,
) -> Result<VideoFrame, LivenessProviderError> {
    let preprocessed = preprocess_screen_replay_frame(frame, config)?;
    mat_to_gray_video_frame(&preprocessed.binary_mask)
}

pub(crate) fn video_frame_to_mat(frame: &VideoFrame) -> Result<Mat, LivenessProviderError> {
    let channels = match frame.format {
        PixelFormat::Bgr8 | PixelFormat::Rgb8 => 3,
        PixelFormat::Gray8 => 1,
    };
    let mat = Mat::from_slice(&frame.data).map_err(|_| LivenessProviderError::InvalidFrame)?;
    let mat = mat
        .reshape(channels, frame.height as i32)
        .map_err(|_| LivenessProviderError::InvalidFrame)?;
    mat.try_clone()
        .map_err(|_| LivenessProviderError::InvalidFrame)
}

fn grayscale_mat(image: &Mat, pixel_format: PixelFormat) -> Result<Mat, LivenessProviderError> {
    if pixel_format == PixelFormat::Gray8 {
        return image
            .try_clone()
            .map_err(|_| LivenessProviderError::InvalidFrame);
    }

    let mut gray = Mat::default();
    let color_conversion = match pixel_format {
        PixelFormat::Bgr8 => imgproc::COLOR_BGR2GRAY,
        PixelFormat::Rgb8 => imgproc::COLOR_RGB2GRAY,
        PixelFormat::Gray8 => imgproc::COLOR_BGR2GRAY,
    };
    imgproc::cvt_color(
        image,
        &mut gray,
        color_conversion,
        0,
        AlgorithmHint::ALGO_HINT_DEFAULT,
    )
    .map_err(|_| LivenessProviderError::InvalidFrame)?;
    Ok(gray)
}

fn build_binary_mask(
    gray: &Mat,
    config: &ScreenReplayPreprocessConfig,
) -> Result<Mat, LivenessProviderError> {
    let mut inverted_binary = Mat::default();
    imgproc::threshold(
        gray,
        &mut inverted_binary,
        config.binary_threshold,
        255.0,
        imgproc::THRESH_BINARY_INV,
    )
    .map_err(|_| LivenessProviderError::InvalidFrame)?;

    let mut binary_mask = Mat::default();
    core::in_range(
        &inverted_binary,
        &Scalar::new(0.0, 0.0, 0.0, 0.0),
        &Scalar::new(config.binary_mask_upper_threshold, 0.0, 0.0, 0.0),
        &mut binary_mask,
    )
    .map_err(|_| LivenessProviderError::InvalidFrame)?;
    Ok(binary_mask)
}

fn mat_to_gray_video_frame(mat: &Mat) -> Result<VideoFrame, LivenessProviderError> {
    if mat.empty() || mat.channels() != 1 {
        return Err(LivenessProviderError::InvalidFrame);
    }
    let data = mat
        .data_bytes()
        .map_err(|_| LivenessProviderError::InvalidFrame)?
        .to_vec();
    let frame = VideoFrame {
        width: mat.cols() as u32,
        height: mat.rows() as u32,
        format: PixelFormat::Gray8,
        data,
    };
    frame
        .validate()
        .map_err(|_| LivenessProviderError::InvalidFrame)?;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_replay_mask_keeps_bright_region_white_after_inverted_threshold_pipeline()
    -> Result<(), LivenessProviderError> {
        let frame = VideoFrame {
            width: 3,
            height: 1,
            format: PixelFormat::Gray8,
            data: vec![20, 160, 240],
        };

        let mask = build_screen_replay_binary_mask_frame(
            &frame,
            &ScreenReplayPreprocessConfig::default(),
        )?;

        assert_eq!(mask.data, vec![0, 255, 255]);
        Ok(())
    }
}
