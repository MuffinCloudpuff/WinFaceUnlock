use crate::{PixelFormat, VideoFrame};
use opencv::prelude::VectorToVec;
use opencv::{
    core::{Mat, MatTraitConst, Vector},
    imgcodecs, imgproc,
};

pub struct FaceCropRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub fn crop_and_encode_face_to_jpeg(
    frame: &VideoFrame,
    face_box: &FaceCropRect,
) -> Result<Vec<u8>, String> {
    let channels = match frame.format {
        PixelFormat::Bgr8 | PixelFormat::Rgb8 => 3,
        PixelFormat::Gray8 => 1,
    };
    let mat = Mat::from_slice(&frame.data).map_err(|e| e.to_string())?;
    let mat = mat
        .reshape(channels, frame.height as i32)
        .map_err(|e| e.to_string())?;
    let mut image = mat.try_clone().map_err(|e| e.to_string())?;

    if frame.format == PixelFormat::Rgb8 {
        let mut bgr = Mat::default();
        imgproc::cvt_color(
            &image,
            &mut bgr,
            imgproc::COLOR_RGB2BGR,
            0,
            opencv::core::AlgorithmHint::ALGO_HINT_DEFAULT,
        )
        .map_err(|e| e.to_string())?;
        image = bgr;
    }

    let max_dim = face_box.width.max(face_box.height);
    let padding = max_dim * 0.4;
    let crop_size = (max_dim + 2.0 * padding) as i32;
    let cx = face_box.x + face_box.width / 2.0;
    let cy = face_box.y + face_box.height / 2.0;

    let mut x1 = (cx - crop_size as f32 / 2.0) as i32;
    let mut y1 = (cy - crop_size as f32 / 2.0) as i32;
    let mut x2 = x1 + crop_size;
    let mut y2 = y1 + crop_size;

    x1 = x1.max(0);
    y1 = y1.max(0);
    x2 = x2.min(image.cols());
    y2 = y2.min(image.rows());

    if x2 > x1 && y2 > y1 {
        let roi = opencv::core::Rect::new(x1, y1, x2 - x1, y2 - y1);
        let cropped = Mat::roi(&image, roi).map_err(|e| e.to_string())?;

        let mut resized = Mat::default();
        imgproc::resize(
            &cropped,
            &mut resized,
            opencv::core::Size::new(256, 256),
            0.0,
            0.0,
            imgproc::INTER_AREA,
        )
        .map_err(|e| e.to_string())?;
        image = resized;
    }

    let mut encoded = Vector::<u8>::new();
    let params = Vector::from_slice(&[imgcodecs::IMWRITE_JPEG_QUALITY, 85]);
    imgcodecs::imencode(".jpg", &image, &mut encoded, &params)
        .map_err(|e| e.to_string())?
        .then_some(())
        .ok_or_else(|| "Failed to encode image to jpeg".to_string())?;

    Ok(encoded.to_vec())
}
