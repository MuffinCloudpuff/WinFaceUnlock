use std::path::Path;

use face_engine::DetectedFace;
use opencv::{
    core::{AlgorithmHint, Mat, MatTraitConst, Point, Rect, Scalar, Vector},
    imgcodecs, imgproc,
};
use video_provider::{PixelFormat, VideoFrame};

use crate::{FaceImageRect, ScreenReplayProviderSummary};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScreenReplayDebugFrameError {
    InvalidFrame,
    WriteFailed,
}

pub fn write_screen_replay_debug_frame(
    frame: &VideoFrame,
    faces: &[DetectedFace],
    summary: &ScreenReplayProviderSummary,
    image_path: &Path,
) -> Result<(), ScreenReplayDebugFrameError> {
    let mut image = video_frame_to_mat(frame)?;
    for face in faces {
        draw_rectangle(
            &mut image,
            &FaceImageRect::from_face_box(&face.bounds),
            Scalar::new(0.0, 255.0, 0.0, 0.0),
            2,
        )?;
    }
    if let Some(observation) = &summary.best_observation {
        let color = if observation.face_inside_rectangle {
            Scalar::new(0.0, 0.0, 255.0, 0.0)
        } else {
            Scalar::new(255.0, 128.0, 0.0, 0.0)
        };
        draw_rectangle(&mut image, &observation.rectangle, color, 3)?;
        let label = format!(
            "screen score={:.2} inside={:.2}",
            observation.screen_replay_score, observation.face_inside_screen_ratio
        );
        imgproc::put_text(
            &mut image,
            &label,
            Point::new(
                observation.rectangle.x.round().max(0.0) as i32,
                observation.rectangle.y.round().max(20.0) as i32 - 8,
            ),
            imgproc::FONT_HERSHEY_SIMPLEX,
            0.5,
            color,
            1,
            imgproc::LINE_8,
            false,
        )
        .map_err(|_| ScreenReplayDebugFrameError::WriteFailed)?;
    }

    let path = image_path.to_string_lossy();
    imgcodecs::imwrite(&path, &image, &Vector::new())
        .map_err(|_| ScreenReplayDebugFrameError::WriteFailed)?
        .then_some(())
        .ok_or(ScreenReplayDebugFrameError::WriteFailed)
}

fn draw_rectangle(
    image: &mut Mat,
    rectangle: &FaceImageRect,
    color: Scalar,
    thickness: i32,
) -> Result<(), ScreenReplayDebugFrameError> {
    let rect = Rect::new(
        rectangle.x.round().max(0.0) as i32,
        rectangle.y.round().max(0.0) as i32,
        rectangle.width.round().max(1.0) as i32,
        rectangle.height.round().max(1.0) as i32,
    );
    imgproc::rectangle(image, rect, color, thickness, imgproc::LINE_8, 0)
        .map_err(|_| ScreenReplayDebugFrameError::WriteFailed)
}

fn video_frame_to_mat(frame: &VideoFrame) -> Result<Mat, ScreenReplayDebugFrameError> {
    frame
        .validate()
        .map_err(|_| ScreenReplayDebugFrameError::InvalidFrame)?;
    let channels = match frame.format {
        PixelFormat::Bgr8 | PixelFormat::Rgb8 => 3,
        PixelFormat::Gray8 => 1,
    };
    let mat =
        Mat::from_slice(&frame.data).map_err(|_| ScreenReplayDebugFrameError::InvalidFrame)?;
    let mat = mat
        .reshape(channels, frame.height as i32)
        .map_err(|_| ScreenReplayDebugFrameError::InvalidFrame)?;
    let mut mat = mat
        .try_clone()
        .map_err(|_| ScreenReplayDebugFrameError::InvalidFrame)?;

    if frame.format == PixelFormat::Rgb8 {
        let mut bgr = Mat::default();
        imgproc::cvt_color(
            &mat,
            &mut bgr,
            imgproc::COLOR_RGB2BGR,
            0,
            AlgorithmHint::ALGO_HINT_DEFAULT,
        )
        .map_err(|_| ScreenReplayDebugFrameError::InvalidFrame)?;
        mat = bgr;
    }

    Ok(mat)
}
