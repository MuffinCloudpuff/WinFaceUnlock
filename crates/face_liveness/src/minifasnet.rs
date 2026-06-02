use std::path::{Path, PathBuf};

use face_engine::DetectedFace;
use opencv::{
    core::{self, AlgorithmHint, Mat, MatTraitConst, MatTraitConstManual, Rect, Scalar, Size},
    dnn, imgcodecs, imgproc,
    prelude::NetTrait,
};
use video_provider::{PixelFormat, VideoFrame};

use crate::{
    FaceImageRect, LivenessDecision, LivenessEvidence, LivenessProviderError, LivenessResult,
    preprocessing::video_frame_to_mat,
};

const MINIFASNET_CLASS_COUNT: usize = 3;
const MINIFASNET_INPUT_SCALE: f64 = 1.0;

#[derive(Clone, Debug, PartialEq)]
pub struct MiniFasNetLivenessProviderConfig {
    pub model_path: PathBuf,
    pub crop_scale: f32,
    pub input_width: i32,
    pub input_height: i32,
    pub min_live_score: f32,
    pub min_spoof_score: f32,
    pub reject_on_model_spoof: bool,
}

impl Default for MiniFasNetLivenessProviderConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from("models/minifasnet_v2.onnx"),
            crop_scale: 2.7,
            input_width: 80,
            input_height: 80,
            min_live_score: 0.80,
            min_spoof_score: 0.70,
            reject_on_model_spoof: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct MiniFasNetLivenessPrediction {
    pub crop_rectangle: FaceImageRect,
    pub live_score: f32,
    pub print_attack_score: f32,
    pub replay_attack_score: f32,
    pub spoof_score: f32,
}

pub struct MiniFasNetLivenessProvider {
    config: MiniFasNetLivenessProviderConfig,
    net: Option<dnn::Net>,
}

#[derive(Clone, Debug)]
pub struct MiniFasNetCropImages {
    pub crop_rectangle: FaceImageRect,
    pub source_crop_bgr: Mat,
    pub model_input_bgr: Mat,
}

impl MiniFasNetLivenessProvider {
    pub fn new(config: MiniFasNetLivenessProviderConfig) -> Self {
        Self { config, net: None }
    }

    pub fn config(&self) -> &MiniFasNetLivenessProviderConfig {
        &self.config
    }

    pub fn load_model(&mut self) -> Result<(), LivenessProviderError> {
        if !self.config.model_path.exists() {
            return Err(LivenessProviderError::ModelPathMissing);
        }

        let model_path = self.config.model_path.to_string_lossy();
        let mut net = dnn::read_net_from_onnx(&model_path)
            .map_err(|_| LivenessProviderError::ModelLoadFailed)?;
        net.set_preferable_backend(dnn::DNN_BACKEND_OPENCV)
            .map_err(|_| LivenessProviderError::ModelLoadFailed)?;
        net.set_preferable_target(dnn::DNN_TARGET_CPU)
            .map_err(|_| LivenessProviderError::ModelLoadFailed)?;
        self.net = Some(net);
        Ok(())
    }

    pub fn unload_model(&mut self) {
        self.net = None;
    }

    pub fn evaluate(
        &mut self,
        frame: &VideoFrame,
        detected_face: Option<&DetectedFace>,
    ) -> Result<LivenessResult, LivenessProviderError> {
        let Some(face) = detected_face else {
            return Ok(LivenessResult {
                liveness_decision: LivenessDecision::Inconclusive,
                liveness_score: None,
                evidence: vec![LivenessEvidence::NoFaceForMiniFasNet],
            });
        };

        let prediction = self.predict(frame, face)?;
        let liveness_decision = if self.config.reject_on_model_spoof
            && prediction.spoof_score >= self.config.min_spoof_score
        {
            LivenessDecision::SpoofRejected
        } else if prediction.live_score >= self.config.min_live_score {
            LivenessDecision::LiveAccepted
        } else {
            LivenessDecision::Inconclusive
        };

        Ok(LivenessResult {
            liveness_decision,
            liveness_score: Some(prediction.live_score),
            evidence: vec![LivenessEvidence::MiniFasNetPrediction {
                crop_rectangle: prediction.crop_rectangle,
                live_score: prediction.live_score,
                print_attack_score: prediction.print_attack_score,
                replay_attack_score: prediction.replay_attack_score,
                spoof_score: prediction.spoof_score,
            }],
        })
    }

    pub fn predict(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<MiniFasNetLivenessPrediction, LivenessProviderError> {
        let crop_images = self.build_crop_images(frame, face)?;
        let input_blob = self.input_blob_from_crop(&crop_images.model_input_bgr)?;
        let net = self
            .net
            .as_mut()
            .ok_or(LivenessProviderError::ModelNotLoaded)?;
        net.set_input_def(&input_blob)
            .map_err(|_| LivenessProviderError::InferenceFailed)?;

        let mut output = Mat::default();
        net.forward_layer_def(&mut output)
            .map_err(|_| LivenessProviderError::InferenceFailed)?;
        let output_values = output
            .data_typed::<f32>()
            .map_err(|_| LivenessProviderError::InferenceFailed)?;
        if output_values.len() < MINIFASNET_CLASS_COUNT {
            return Err(LivenessProviderError::InferenceFailed);
        }

        let probabilities = softmax_first_three(output_values);
        let spoof_score = (probabilities[0] + probabilities[2]).clamp(0.0, 1.0);
        Ok(MiniFasNetLivenessPrediction {
            crop_rectangle: crop_images.crop_rectangle,
            live_score: probabilities[1],
            print_attack_score: probabilities[0],
            replay_attack_score: probabilities[2],
            spoof_score,
        })
    }

    pub fn write_debug_crops(
        &self,
        frame: &VideoFrame,
        face: &DetectedFace,
        source_crop_path: &Path,
        model_input_path: &Path,
    ) -> Result<(), LivenessProviderError> {
        let crop_images = self.build_crop_images(frame, face)?;
        let params = core::Vector::<i32>::new();
        imgcodecs::imwrite(
            &source_crop_path.to_string_lossy(),
            &crop_images.source_crop_bgr,
            &params,
        )
        .map_err(|_| LivenessProviderError::InvalidFrame)?;
        imgcodecs::imwrite(
            &model_input_path.to_string_lossy(),
            &crop_images.model_input_bgr,
            &params,
        )
        .map_err(|_| LivenessProviderError::InvalidFrame)?;
        Ok(())
    }

    fn build_crop_images(
        &self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<MiniFasNetCropImages, LivenessProviderError> {
        frame
            .validate()
            .map_err(|_| LivenessProviderError::InvalidFrame)?;
        let image = bgr_mat(frame)?;
        let crop_rect = scaled_face_crop_rect(frame, face, self.config.crop_scale);
        let roi = Rect::new(
            crop_rect.x.round().max(0.0) as i32,
            crop_rect.y.round().max(0.0) as i32,
            crop_rect.width.round().max(1.0) as i32,
            crop_rect.height.round().max(1.0) as i32,
        );
        let crop = Mat::roi(&image, roi).map_err(|_| LivenessProviderError::InvalidFrame)?;
        let source_crop_bgr = crop
            .try_clone()
            .map_err(|_| LivenessProviderError::InvalidFrame)?;
        let mut resized = Mat::default();
        imgproc::resize(
            &crop,
            &mut resized,
            Size::new(self.config.input_width, self.config.input_height),
            0.0,
            0.0,
            imgproc::INTER_LINEAR,
        )
        .map_err(|_| LivenessProviderError::InvalidFrame)?;

        Ok(MiniFasNetCropImages {
            crop_rectangle: crop_rect,
            source_crop_bgr,
            model_input_bgr: resized,
        })
    }

    fn input_blob_from_crop(&self, crop: &Mat) -> Result<Mat, LivenessProviderError> {
        dnn::blob_from_image(
            crop,
            MINIFASNET_INPUT_SCALE,
            Size::new(self.config.input_width, self.config.input_height),
            Scalar::default(),
            false,
            false,
            core::CV_32F,
        )
        .map_err(|_| LivenessProviderError::InvalidFrame)
    }
}

fn bgr_mat(frame: &VideoFrame) -> Result<Mat, LivenessProviderError> {
    let image = video_frame_to_mat(frame)?;
    match frame.format {
        PixelFormat::Bgr8 => Ok(image),
        PixelFormat::Rgb8 => {
            let mut converted = Mat::default();
            imgproc::cvt_color(
                &image,
                &mut converted,
                imgproc::COLOR_RGB2BGR,
                0,
                AlgorithmHint::ALGO_HINT_DEFAULT,
            )
            .map_err(|_| LivenessProviderError::InvalidFrame)?;
            Ok(converted)
        }
        PixelFormat::Gray8 => {
            let mut converted = Mat::default();
            imgproc::cvt_color(
                &image,
                &mut converted,
                imgproc::COLOR_GRAY2BGR,
                0,
                AlgorithmHint::ALGO_HINT_DEFAULT,
            )
            .map_err(|_| LivenessProviderError::InvalidFrame)?;
            Ok(converted)
        }
    }
}

fn scaled_face_crop_rect(
    frame: &VideoFrame,
    face: &DetectedFace,
    crop_scale: f32,
) -> FaceImageRect {
    let face_rect = FaceImageRect::from_face_box(&face.bounds);
    let crop_width = (face_rect.width * crop_scale).max(1.0);
    let crop_height = (face_rect.height * crop_scale).max(1.0);
    let center_x = face_rect.center_x();
    let center_y = face_rect.center_y();
    let left = (center_x - crop_width / 2.0).clamp(0.0, frame.width.saturating_sub(1) as f32);
    let top = (center_y - crop_height / 2.0).clamp(0.0, frame.height.saturating_sub(1) as f32);
    let right = (center_x + crop_width / 2.0).clamp(left + 1.0, frame.width as f32);
    let bottom = (center_y + crop_height / 2.0).clamp(top + 1.0, frame.height as f32);

    FaceImageRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

fn softmax_first_three(values: &[f32]) -> [f32; MINIFASNET_CLASS_COUNT] {
    let logits = [values[0], values[1], values[2]];
    let max_logit = logits
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, |left, right| left.max(right));
    let exp_values = [
        (logits[0] - max_logit).exp(),
        (logits[1] - max_logit).exp(),
        (logits[2] - max_logit).exp(),
    ];
    let sum = exp_values.iter().sum::<f32>().max(f32::EPSILON);
    [
        exp_values[0] / sum,
        exp_values[1] / sum,
        exp_values[2] / sum,
    ]
}

#[cfg(test)]
mod tests {
    use face_engine::{FaceBox, FaceLandmark};

    use super::*;

    #[test]
    fn softmax_returns_three_probabilities_that_sum_to_one() {
        let probabilities = softmax_first_three(&[2.0, 1.0, 0.0]);

        let sum = probabilities.iter().sum::<f32>();
        assert!((sum - 1.0).abs() < 0.0001);
        assert!(probabilities[0] > probabilities[1]);
        assert!(probabilities[1] > probabilities[2]);
    }

    #[test]
    fn input_blob_preserves_raw_pixel_scale_for_local_onnx_model()
    -> Result<(), LivenessProviderError> {
        let provider = MiniFasNetLivenessProvider::new(MiniFasNetLivenessProviderConfig::default());
        let crop = Mat::new_rows_cols_with_default(2, 2, core::CV_8UC3, Scalar::all(255.0))
            .map_err(|_| LivenessProviderError::InvalidFrame)?;

        let blob = provider.input_blob_from_crop(&crop)?;
        let values = blob
            .data_typed::<f32>()
            .map_err(|_| LivenessProviderError::InvalidFrame)?;

        assert!(
            values
                .iter()
                .all(|value| (*value - 255.0).abs() < f32::EPSILON)
        );
        Ok(())
    }

    #[test]
    fn scaled_crop_stays_inside_frame() {
        let frame = VideoFrame {
            width: 100,
            height: 80,
            format: PixelFormat::Bgr8,
            data: vec![0; 100 * 80 * 3],
        };
        let face = DetectedFace {
            bounds: FaceBox {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 30.0,
            },
            landmarks: Vec::<FaceLandmark>::new(),
            confidence: 0.95,
        };

        let crop = scaled_face_crop_rect(&frame, &face, 2.7);

        assert_eq!(crop.x, 0.0);
        assert_eq!(crop.y, 0.0);
        assert!(crop.width <= frame.width as f32);
        assert!(crop.height <= frame.height as f32);
        assert!(crop.width > face.bounds.width);
        assert!(crop.height > face.bounds.height);
    }

    #[test]
    fn local_minifasnet_model_runs_when_present() -> Result<(), LivenessProviderError> {
        let config = MiniFasNetLivenessProviderConfig::default();
        if !config.model_path.exists() {
            return Ok(());
        }

        let frame = VideoFrame {
            width: 100,
            height: 100,
            format: PixelFormat::Bgr8,
            data: vec![128; 100 * 100 * 3],
        };
        let face = DetectedFace {
            bounds: FaceBox {
                x: 30.0,
                y: 30.0,
                width: 40.0,
                height: 40.0,
            },
            landmarks: Vec::<FaceLandmark>::new(),
            confidence: 0.95,
        };
        let mut provider = MiniFasNetLivenessProvider::new(config);
        provider.load_model()?;

        let prediction = provider.predict(&frame, &face)?;

        let probability_sum =
            prediction.live_score + prediction.print_attack_score + prediction.replay_attack_score;
        assert!((probability_sum - 1.0).abs() < 0.001);
        Ok(())
    }
}
