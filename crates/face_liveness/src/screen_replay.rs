use face_engine::DetectedFace;
use opencv::{
    core::{Mat, MatTraitConstManual, Point, Vector},
    imgproc,
};
use video_provider::VideoFrame;

use crate::{
    FaceImageRect, LivenessDecision, LivenessEvidence, LivenessProviderError, LivenessResult,
    ScreenReplayPreprocessConfig, preprocessing::preprocess_screen_replay_frame,
};

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenReplayLivenessProviderConfig {
    pub binary_threshold: f64,
    pub binary_mask_upper_threshold: f64,
    pub min_screen_area_ratio: f32,
    pub max_screen_area_ratio: f32,
    pub min_rectangularity_score: f32,
    pub min_brightness_contrast_score: f32,
    pub min_face_inside_screen_ratio: f32,
    pub min_screen_aspect_ratio: f32,
    pub max_screen_aspect_ratio: f32,
}

impl Default for ScreenReplayLivenessProviderConfig {
    fn default() -> Self {
        Self {
            binary_threshold: 150.0,
            binary_mask_upper_threshold: 50.0,
            min_screen_area_ratio: 0.08,
            max_screen_area_ratio: 0.90,
            min_rectangularity_score: 0.45,
            min_brightness_contrast_score: 0.05,
            min_face_inside_screen_ratio: 0.95,
            min_screen_aspect_ratio: 0.35,
            max_screen_aspect_ratio: 3.20,
        }
    }
}

impl ScreenReplayLivenessProviderConfig {
    pub fn preprocess_config(&self) -> ScreenReplayPreprocessConfig {
        ScreenReplayPreprocessConfig {
            binary_threshold: self.binary_threshold,
            binary_mask_upper_threshold: self.binary_mask_upper_threshold,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ScreenReplayProviderObservation {
    pub rectangle: FaceImageRect,
    pub face_inside_rectangle: bool,
    pub face_inside_screen_ratio: f32,
    pub rectangle_area_ratio: f32,
    pub brightness_contrast_score: f32,
    pub edge_rectangularity_score: f32,
    pub screen_replay_score: f32,
    pub contour_area: f32,
    pub approximated_polygon_vertex_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ScreenReplayProviderSummary {
    pub candidate_rectangle_count: usize,
    pub best_observation: Option<ScreenReplayProviderObservation>,
}

pub struct ScreenReplayLivenessProvider {
    config: ScreenReplayLivenessProviderConfig,
}

impl ScreenReplayLivenessProvider {
    pub fn new(config: ScreenReplayLivenessProviderConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &ScreenReplayLivenessProviderConfig {
        &self.config
    }

    pub fn evaluate(
        &self,
        frame: &VideoFrame,
        detected_face: Option<&DetectedFace>,
    ) -> Result<(LivenessResult, ScreenReplayProviderSummary), LivenessProviderError> {
        frame
            .validate()
            .map_err(|_| LivenessProviderError::InvalidFrame)?;
        let preprocessed = preprocess_screen_replay_frame(frame, &self.config.preprocess_config())?;
        let contours = self.find_external_binary_contours(&preprocessed.binary_mask)?;
        let face_rect = detected_face.map(|face| FaceImageRect::from_face_box(&face.bounds));
        let mut observations = Vec::new();

        for contour in contours {
            if let Some(observation) = self.contour_to_observation(
                frame,
                &preprocessed.gray,
                &contour,
                face_rect.as_ref(),
            )? {
                observations.push(observation);
            }
        }

        observations.sort_by(|left, right| {
            right
                .screen_replay_score
                .total_cmp(&left.screen_replay_score)
        });
        let best_observation = observations.first().cloned();
        let summary = ScreenReplayProviderSummary {
            candidate_rectangle_count: observations.len(),
            best_observation: best_observation.clone(),
        };

        if let Some(observation) = best_observation {
            let liveness_decision = if observation.face_inside_rectangle {
                LivenessDecision::SpoofRejected
            } else {
                LivenessDecision::Inconclusive
            };
            let evidence = LivenessEvidence::ScreenLikeRectangleDetected {
                rectangle: observation.rectangle,
                face_inside_rectangle: observation.face_inside_rectangle,
                face_inside_screen_ratio: observation.face_inside_screen_ratio,
                rectangle_area_ratio: observation.rectangle_area_ratio,
                brightness_contrast_score: observation.brightness_contrast_score,
                edge_rectangularity_score: observation.edge_rectangularity_score,
            };
            return Ok((
                LivenessResult {
                    liveness_decision,
                    liveness_score: Some(observation.screen_replay_score),
                    evidence: vec![evidence],
                },
                summary,
            ));
        }

        Ok((
            LivenessResult {
                liveness_decision: LivenessDecision::Inconclusive,
                liveness_score: None,
                evidence: vec![LivenessEvidence::NoScreenLikeRectangleDetected],
            },
            summary,
        ))
    }

    fn find_external_binary_contours(
        &self,
        binary_mask: &Mat,
    ) -> Result<Vector<Vector<Point>>, LivenessProviderError> {
        let mut contours = Vector::<Vector<Point>>::new();
        imgproc::find_contours(
            binary_mask,
            &mut contours,
            imgproc::RETR_EXTERNAL,
            imgproc::CHAIN_APPROX_SIMPLE,
            Point::new(0, 0),
        )
        .map_err(|_| LivenessProviderError::InvalidFrame)?;
        Ok(contours)
    }

    fn contour_to_observation(
        &self,
        frame: &VideoFrame,
        gray: &Mat,
        contour: &Vector<Point>,
        face_rect: Option<&FaceImageRect>,
    ) -> Result<Option<ScreenReplayProviderObservation>, LivenessProviderError> {
        let frame_area = (frame.width as f32) * (frame.height as f32);
        if frame_area <= 0.0 {
            return Ok(None);
        }

        let contour_area = imgproc::contour_area(contour, false)
            .map_err(|_| LivenessProviderError::InvalidFrame)? as f32;
        if contour_area <= 0.0 {
            return Ok(None);
        }

        let perimeter =
            imgproc::arc_length(contour, true).map_err(|_| LivenessProviderError::InvalidFrame)?;
        let mut approximated_polygon = Vector::<Point>::new();
        imgproc::approx_poly_dp(contour, &mut approximated_polygon, 0.02 * perimeter, true)
            .map_err(|_| LivenessProviderError::InvalidFrame)?;
        let bounds = imgproc::bounding_rect(&approximated_polygon)
            .map_err(|_| LivenessProviderError::InvalidFrame)?;
        if bounds.width <= 0 || bounds.height <= 0 {
            return Ok(None);
        }

        let rectangle = FaceImageRect {
            x: bounds.x as f32,
            y: bounds.y as f32,
            width: bounds.width as f32,
            height: bounds.height as f32,
        };
        let rectangle_area = rectangle.area();
        let rectangle_area_ratio = rectangle_area / frame_area;
        if rectangle_area_ratio < self.config.min_screen_area_ratio
            || rectangle_area_ratio > self.config.max_screen_area_ratio
        {
            return Ok(None);
        }

        let aspect_ratio = rectangle.width / rectangle.height.max(1.0);
        if aspect_ratio < self.config.min_screen_aspect_ratio
            || aspect_ratio > self.config.max_screen_aspect_ratio
        {
            return Ok(None);
        }

        let rectangularity_score = contour_area / rectangle_area.max(1.0);
        if rectangularity_score < self.config.min_rectangularity_score {
            return Ok(None);
        }

        let brightness_contrast_score = brightness_contrast_score(frame, gray, &rectangle)?;
        if brightness_contrast_score < self.config.min_brightness_contrast_score {
            return Ok(None);
        }

        let (face_inside_rectangle, face_inside_screen_ratio) = face_rect
            .map(|face| {
                let face_area = face.area().max(1.0);
                let face_inside_screen_ratio = rectangle.intersection_area(face) / face_area;
                (
                    rectangle.fully_contains(face)
                        || face_inside_screen_ratio >= self.config.min_face_inside_screen_ratio,
                    face_inside_screen_ratio,
                )
            })
            .unwrap_or((false, 0.0));
        let screen_replay_score = weighted_score(
            rectangularity_score,
            brightness_contrast_score,
            face_inside_screen_ratio,
        );

        Ok(Some(ScreenReplayProviderObservation {
            rectangle,
            face_inside_rectangle,
            face_inside_screen_ratio,
            rectangle_area_ratio,
            brightness_contrast_score,
            edge_rectangularity_score: rectangularity_score,
            screen_replay_score,
            contour_area,
            approximated_polygon_vertex_count: approximated_polygon.len(),
        }))
    }
}

fn brightness_contrast_score(
    frame: &VideoFrame,
    gray: &Mat,
    rectangle: &FaceImageRect,
) -> Result<f32, LivenessProviderError> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let gray_bytes = gray
        .data_bytes()
        .map_err(|_| LivenessProviderError::InvalidFrame)?;
    let rect_left = rectangle.x.max(0.0).floor() as usize;
    let rect_top = rectangle.y.max(0.0).floor() as usize;
    let rect_right = (rectangle.x + rectangle.width)
        .ceil()
        .min(frame.width as f32) as usize;
    let rect_bottom = (rectangle.y + rectangle.height)
        .ceil()
        .min(frame.height as f32) as usize;

    let mut inside_sum = 0_u64;
    let mut inside_count = 0_u64;
    let mut outside_sum = 0_u64;
    let mut outside_count = 0_u64;

    for y in 0..height {
        for x in 0..width {
            let value = u64::from(gray_bytes[y * width + x]);
            if x >= rect_left && x < rect_right && y >= rect_top && y < rect_bottom {
                inside_sum = inside_sum.saturating_add(value);
                inside_count = inside_count.saturating_add(1);
            } else {
                outside_sum = outside_sum.saturating_add(value);
                outside_count = outside_count.saturating_add(1);
            }
        }
    }

    if inside_count == 0 || outside_count == 0 {
        return Ok(0.0);
    }
    let inside_mean = inside_sum as f32 / inside_count as f32;
    let outside_mean = outside_sum as f32 / outside_count as f32;
    Ok(((inside_mean - outside_mean).abs() / 255.0).clamp(0.0, 1.0))
}

fn weighted_score(rectangularity: f32, contrast: f32, face_inside_ratio: f32) -> f32 {
    (rectangularity.clamp(0.0, 1.0) * 0.40)
        + (contrast.clamp(0.0, 1.0) * 0.25)
        + (face_inside_ratio.clamp(0.0, 1.0) * 0.35)
}

#[cfg(test)]
mod tests {
    use face_engine::{DetectedFace, FaceBox};
    use video_provider::{PixelFormat, VideoFrame};

    use super::*;

    #[test]
    fn bright_rectangle_with_face_inside_is_rejected_as_screen_replay()
    -> Result<(), LivenessProviderError> {
        let frame = synthetic_gray_frame_with_rectangle(
            100,
            80,
            SyntheticRectangle {
                x: 20,
                y: 10,
                width: 60,
                height: 50,
            },
            240,
            70,
        );
        let face = DetectedFace {
            bounds: FaceBox {
                x: 35.0,
                y: 25.0,
                width: 20.0,
                height: 20.0,
            },
            landmarks: Vec::new(),
            confidence: 0.95,
        };
        let provider = ScreenReplayLivenessProvider::new(ScreenReplayLivenessProviderConfig {
            min_screen_area_ratio: 0.10,
            min_brightness_contrast_score: 0.01,
            ..ScreenReplayLivenessProviderConfig::default()
        });

        let (result, summary) = provider.evaluate(&frame, Some(&face))?;

        assert_eq!(result.liveness_decision, LivenessDecision::SpoofRejected);
        assert_eq!(summary.candidate_rectangle_count, 1);
        Ok(())
    }

    #[test]
    fn bright_rectangle_without_face_is_not_rejected() -> Result<(), LivenessProviderError> {
        let frame = synthetic_gray_frame_with_rectangle(
            100,
            80,
            SyntheticRectangle {
                x: 20,
                y: 10,
                width: 60,
                height: 50,
            },
            240,
            70,
        );
        let face = DetectedFace {
            bounds: FaceBox {
                x: 2.0,
                y: 2.0,
                width: 10.0,
                height: 10.0,
            },
            landmarks: Vec::new(),
            confidence: 0.95,
        };
        let provider = ScreenReplayLivenessProvider::new(ScreenReplayLivenessProviderConfig {
            min_screen_area_ratio: 0.10,
            min_brightness_contrast_score: 0.01,
            ..ScreenReplayLivenessProviderConfig::default()
        });

        let (result, _) = provider.evaluate(&frame, Some(&face))?;

        assert_eq!(result.liveness_decision, LivenessDecision::Inconclusive);
        Ok(())
    }

    #[test]
    fn high_face_overlap_rejects_even_when_face_box_is_not_fully_contained()
    -> Result<(), LivenessProviderError> {
        let frame = synthetic_gray_frame_with_rectangle(
            100,
            80,
            SyntheticRectangle {
                x: 20,
                y: 10,
                width: 60,
                height: 50,
            },
            240,
            70,
        );
        let face = DetectedFace {
            bounds: FaceBox {
                x: 19.0,
                y: 20.0,
                width: 40.0,
                height: 30.0,
            },
            landmarks: Vec::new(),
            confidence: 0.95,
        };
        let provider = ScreenReplayLivenessProvider::new(ScreenReplayLivenessProviderConfig {
            min_screen_area_ratio: 0.10,
            min_brightness_contrast_score: 0.01,
            min_face_inside_screen_ratio: 0.95,
            ..ScreenReplayLivenessProviderConfig::default()
        });

        let (result, summary) = provider.evaluate(&frame, Some(&face))?;

        assert_eq!(result.liveness_decision, LivenessDecision::SpoofRejected);
        assert_eq!(summary.candidate_rectangle_count, 1);
        let Some(observation) = summary.best_observation else {
            return Err(LivenessProviderError::InvalidFrame);
        };
        assert!(
            !observation
                .rectangle
                .fully_contains(&FaceImageRect::from_face_box(&face.bounds))
        );
        assert!(observation.face_inside_screen_ratio >= 0.95);
        Ok(())
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct SyntheticRectangle {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    }

    fn synthetic_gray_frame_with_rectangle(
        width: u32,
        height: u32,
        rectangle: SyntheticRectangle,
        rect_value: u8,
        background_value: u8,
    ) -> VideoFrame {
        let mut data = vec![background_value; (width * height) as usize];
        for y in rectangle.y..rectangle.y + rectangle.height {
            for x in rectangle.x..rectangle.x + rectangle.width {
                data[(y * width + x) as usize] = rect_value;
            }
        }
        VideoFrame {
            width,
            height,
            format: PixelFormat::Gray8,
            data,
        }
    }
}
