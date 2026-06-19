use face_engine::{DetectedFace, FaceSampleRejectReason, FaceTemplateQualityScores};
use face_pose::{FacePoseEstimate, pose_fit_score};
use video_provider::{PixelFormat, VideoFrame};

use crate::GuidedEnrollmentStep;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceQualityPolicy {
    pub min_quality_score: f32,
    pub min_face_size_score: f32,
    pub min_illumination_score: f32,
    pub min_blur_score: f32,
    pub min_pose_fit_score: f32,
}

impl Default for FaceQualityPolicy {
    fn default() -> Self {
        Self {
            min_quality_score: 0.55,
            min_face_size_score: 0.30,
            min_illumination_score: 0.35,
            min_blur_score: 0.20,
            min_pose_fit_score: 0.25,
        }
    }
}

pub fn score_face_sample(
    frame: &VideoFrame,
    face: &DetectedFace,
    step: GuidedEnrollmentStep,
    pose_estimate: Option<FacePoseEstimate>,
    embedding_consistency_score: Option<f32>,
) -> FaceTemplateQualityScores {
    let face_size_score = score_face_size(frame, face);
    let blur_score = score_face_region_blur(frame, face);
    let illumination_score = score_face_region_illumination(frame, face);
    let alignment_score = score_alignment(face);
    let pose_fit_score = pose_fit_score(step.pose_target(), pose_estimate);
    let embedding_component = embedding_consistency_score.unwrap_or(1.0).clamp(0.0, 1.0);

    let quality_score = weighted_average(&[
        (face_size_score, 0.18),
        (blur_score, 0.15),
        (illumination_score, 0.17),
        (alignment_score, 0.15),
        (pose_fit_score, 0.20),
        (embedding_component, 0.15),
    ]);

    FaceTemplateQualityScores {
        quality_score,
        blur_score,
        illumination_score,
        face_size_score,
        alignment_score,
        pose_fit_score,
        embedding_consistency_score,
    }
}

pub fn reject_reason_for_quality(
    quality: &FaceTemplateQualityScores,
    policy: &FaceQualityPolicy,
) -> Option<FaceSampleRejectReason> {
    if quality.face_size_score < policy.min_face_size_score {
        return Some(FaceSampleRejectReason::FaceTooSmall);
    }
    if quality.blur_score < policy.min_blur_score {
        return Some(FaceSampleRejectReason::BlurTooHigh);
    }
    if quality.illumination_score < policy.min_illumination_score {
        return Some(FaceSampleRejectReason::UnderExposed);
    }
    if quality.pose_fit_score < policy.min_pose_fit_score {
        return Some(FaceSampleRejectReason::PoseOutOfExpectedRange);
    }
    if quality.alignment_score <= 0.0 {
        return Some(FaceSampleRejectReason::AlignmentFailed);
    }
    if quality.quality_score < policy.min_quality_score {
        return Some(FaceSampleRejectReason::PoseOutOfExpectedRange);
    }
    None
}

fn score_face_size(frame: &VideoFrame, face: &DetectedFace) -> f32 {
    if frame.width == 0 || frame.height == 0 {
        return 0.0;
    }

    let frame_area = frame.width as f32 * frame.height as f32;
    let face_area_ratio = (face.bounds.width * face.bounds.height / frame_area).clamp(0.0, 1.0);
    if face_area_ratio < 0.02 {
        return face_area_ratio / 0.02 * 0.3;
    }
    if face_area_ratio < 0.08 {
        return 0.3 + (face_area_ratio - 0.02) / 0.06 * 0.5;
    }
    if face_area_ratio <= 0.45 {
        return 1.0;
    }
    (1.0 - (face_area_ratio - 0.45) / 0.35).clamp(0.0, 1.0)
}

fn score_blur(frame: &VideoFrame) -> f32 {
    if frame.width < 3 || frame.height < 3 || frame.data.is_empty() {
        return 0.0;
    }

    let gray = grayscale_values(frame);
    score_blur_values(&gray, frame.width as usize, frame.height as usize)
}

fn score_face_region_blur(frame: &VideoFrame, face: &DetectedFace) -> f32 {
    let Some(region) = face_region(frame, face, 0.20) else {
        return score_blur(frame);
    };
    let gray = grayscale_values(frame);
    score_blur_region(&gray, frame.width as usize, region).unwrap_or_else(|| score_blur(frame))
}

fn score_blur_values(gray: &[f32], width: usize, height: usize) -> f32 {
    if width < 3 || height < 3 || gray.len() < width.saturating_mul(height) {
        return 0.0;
    }

    let mut laplacian_values = Vec::with_capacity((width - 2) * (height - 2));

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let center = gray[y * width + x] * 4.0;
            let neighbors = gray[(y - 1) * width + x]
                + gray[(y + 1) * width + x]
                + gray[y * width + x - 1]
                + gray[y * width + x + 1];
            laplacian_values.push(center - neighbors);
        }
    }

    let variance = variance(&laplacian_values);
    (variance / 350.0).clamp(0.0, 1.0)
}

fn score_blur_region(gray: &[f32], frame_width: usize, region: FaceRegion) -> Option<f32> {
    if region.width() < 3 || region.height() < 3 || gray.len() < frame_width * region.y_end {
        return None;
    }

    let mut laplacian_values = Vec::with_capacity((region.width() - 2) * (region.height() - 2));
    for y in (region.y_start + 1)..(region.y_end - 1) {
        for x in (region.x_start + 1)..(region.x_end - 1) {
            let center = gray[y * frame_width + x] * 4.0;
            let neighbors = gray[(y - 1) * frame_width + x]
                + gray[(y + 1) * frame_width + x]
                + gray[y * frame_width + x - 1]
                + gray[y * frame_width + x + 1];
            laplacian_values.push(center - neighbors);
        }
    }

    let variance = variance(&laplacian_values);
    Some((variance / 350.0).clamp(0.0, 1.0))
}

fn score_illumination(frame: &VideoFrame) -> f32 {
    if frame.data.is_empty() {
        return 0.0;
    }

    let gray = grayscale_values(frame);
    let mean = gray.iter().sum::<f32>() / gray.len() as f32;
    let mean_score = 1.0 - ((mean - 128.0).abs() / 128.0).clamp(0.0, 1.0);
    let saturated_count = gray
        .iter()
        .filter(|value| **value < 8.0 || **value > 247.0)
        .count();
    let saturated_ratio = saturated_count as f32 / gray.len() as f32;
    let saturation_score = 1.0 - (saturated_ratio / 0.25).clamp(0.0, 1.0);
    (mean_score * 0.65 + saturation_score * 0.35).clamp(0.0, 1.0)
}

fn score_face_region_illumination(frame: &VideoFrame, face: &DetectedFace) -> f32 {
    let Some(region) = face_region(frame, face, 0.20) else {
        return score_illumination(frame);
    };
    let gray = grayscale_values(frame);
    let mut region_values = Vec::with_capacity(region.width() * region.height());
    for y in region.y_start..region.y_end {
        let row = y * frame.width as usize;
        for x in region.x_start..region.x_end {
            region_values.push(gray[row + x]);
        }
    }
    score_illumination_values(&region_values).unwrap_or_else(|| score_illumination(frame))
}

fn score_illumination_values(gray: &[f32]) -> Option<f32> {
    if gray.is_empty() {
        return None;
    }

    let mean = gray.iter().sum::<f32>() / gray.len() as f32;
    let mean_score = 1.0 - ((mean - 128.0).abs() / 128.0).clamp(0.0, 1.0);
    let saturated_count = gray
        .iter()
        .filter(|value| **value < 8.0 || **value > 247.0)
        .count();
    let saturated_ratio = saturated_count as f32 / gray.len() as f32;
    let saturation_score = 1.0 - (saturated_ratio / 0.25).clamp(0.0, 1.0);
    Some((mean_score * 0.65 + saturation_score * 0.35).clamp(0.0, 1.0))
}

fn score_alignment(face: &DetectedFace) -> f32 {
    if face.landmarks.len() < 5 {
        return 0.0;
    }
    if face.confidence < 0.5 {
        return 0.2;
    }
    face.confidence.clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FaceRegion {
    x_start: usize,
    y_start: usize,
    x_end: usize,
    y_end: usize,
}

impl FaceRegion {
    fn width(self) -> usize {
        self.x_end.saturating_sub(self.x_start)
    }

    fn height(self) -> usize {
        self.y_end.saturating_sub(self.y_start)
    }
}

fn face_region(frame: &VideoFrame, face: &DetectedFace, padding_ratio: f32) -> Option<FaceRegion> {
    if frame.width == 0 || frame.height == 0 {
        return None;
    }

    let padding_x = face.bounds.width.max(0.0) * padding_ratio;
    let padding_y = face.bounds.height.max(0.0) * padding_ratio;
    let x_start = (face.bounds.x - padding_x).floor().max(0.0) as usize;
    let y_start = (face.bounds.y - padding_y).floor().max(0.0) as usize;
    let x_end = (face.bounds.x + face.bounds.width + padding_x)
        .ceil()
        .min(frame.width as f32) as usize;
    let y_end = (face.bounds.y + face.bounds.height + padding_y)
        .ceil()
        .min(frame.height as f32) as usize;

    if x_end <= x_start || y_end <= y_start {
        return None;
    }

    Some(FaceRegion {
        x_start,
        y_start,
        x_end,
        y_end,
    })
}

fn grayscale_values(frame: &VideoFrame) -> Vec<f32> {
    match frame.format {
        PixelFormat::Gray8 => frame.data.iter().map(|value| f32::from(*value)).collect(),
        PixelFormat::Bgr8 => frame
            .data
            .chunks_exact(3)
            .map(|pixel| {
                let b = f32::from(pixel[0]);
                let g = f32::from(pixel[1]);
                let r = f32::from(pixel[2]);
                0.114 * b + 0.587 * g + 0.299 * r
            })
            .collect(),
        PixelFormat::Rgb8 => frame
            .data
            .chunks_exact(3)
            .map(|pixel| {
                let r = f32::from(pixel[0]);
                let g = f32::from(pixel[1]);
                let b = f32::from(pixel[2]);
                0.299 * r + 0.587 * g + 0.114 * b
            })
            .collect(),
    }
}

fn variance(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    values
        .iter()
        .map(|value| {
            let delta = value - mean;
            delta * delta
        })
        .sum::<f32>()
        / values.len() as f32
}

fn weighted_average(values: &[(f32, f32)]) -> f32 {
    let total_weight = values.iter().map(|(_, weight)| *weight).sum::<f32>();
    if total_weight <= f32::EPSILON {
        return 0.0;
    }

    values
        .iter()
        .map(|(value, weight)| value.clamp(0.0, 1.0) * weight)
        .sum::<f32>()
        / total_weight
}

#[cfg(test)]
mod tests {
    use face_engine::{DetectedFace, FaceBox};
    use video_provider::{PixelFormat, VideoFrame};

    use super::*;

    #[test]
    fn blank_frame_has_low_blur_score() {
        let frame = VideoFrame {
            width: 8,
            height: 8,
            format: PixelFormat::Gray8,
            data: vec![128; 64],
        };

        assert_eq!(score_blur(&frame), 0.0);
    }

    #[test]
    fn tiny_face_is_rejected_as_too_small() {
        let frame = VideoFrame {
            width: 100,
            height: 100,
            format: PixelFormat::Gray8,
            data: vec![128; 10_000],
        };
        let face = DetectedFace {
            bounds: FaceBox {
                x: 1.0,
                y: 1.0,
                width: 5.0,
                height: 5.0,
            },
            landmarks: Vec::new(),
            confidence: 0.99,
        };

        assert!(score_face_size(&frame, &face) < 0.3);
    }
}
