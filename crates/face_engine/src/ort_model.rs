use std::path::PathBuf;

use ndarray::{Array, Array4};
use opencv::{
    calib3d,
    core::{self, Mat, Point2f, Scalar, Size, Vector},
    imgproc,
    prelude::{MatTraitConst, MatTraitConstManual},
};
use ort::{
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};

use video_provider::{PixelFormat, VideoFrame};

use crate::{
    DetectedFace, FaceEmbedding, FaceEngineError, FaceMatch, FaceMatchDecision,
    FaceModelDescriptor, FaceRecognitionModelProvider, cosine_similarity,
};

// Threshold for GhostFaceNet/ArcFace
pub const GHOSTFACENET_COSINE_MATCH_THRESHOLD: f32 = 0.45;

#[derive(Clone, Debug)]
pub struct OrtGhostFaceNetConfig {
    pub model_path: PathBuf,
    pub model: FaceModelDescriptor,
    pub match_threshold: f32,
}

impl OrtGhostFaceNetConfig {
    pub fn new(model_path: PathBuf) -> Self {
        Self {
            model_path,
            model: FaceModelDescriptor {
                model_family: "ghostfacenet".to_owned(),
                model_version: "v1-stride2".to_owned(),
            },
            match_threshold: GHOSTFACENET_COSINE_MATCH_THRESHOLD,
        }
    }
}

pub struct OrtGhostFaceNetProvider {
    config: OrtGhostFaceNetConfig,
    session: Option<Session>,
}

impl OrtGhostFaceNetProvider {
    pub fn new(config: OrtGhostFaceNetConfig) -> Self {
        // Initialize the ONNX Runtime environment
        let _ = ort::init().with_name("face_engine_ort").commit();

        Self {
            config,
            session: None,
        }
    }

    pub(crate) fn align_crop(
        &self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<Mat, FaceEngineError> {
        let image = video_frame_to_rgb_mat(frame)?;

        if face.landmarks.len() < 5 {
            return Err(FaceEngineError::InferenceFailed);
        }

        let src_pts: Vector<Point2f> = Vector::from_iter(vec![
            Point2f::new(face.landmarks[0].x, face.landmarks[0].y),
            Point2f::new(face.landmarks[1].x, face.landmarks[1].y),
            Point2f::new(face.landmarks[2].x, face.landmarks[2].y),
            Point2f::new(face.landmarks[3].x, face.landmarks[3].y),
            Point2f::new(face.landmarks[4].x, face.landmarks[4].y),
        ]);

        let dst_pts: Vector<Point2f> = Vector::from_iter(vec![
            Point2f::new(38.2946, 51.6963),
            Point2f::new(73.5318, 51.5014),
            Point2f::new(56.0252, 71.7366),
            Point2f::new(41.5493, 92.3655),
            Point2f::new(70.7299, 92.2041),
        ]);

        let mut inliers = Mat::default();
        let m = calib3d::estimate_affine_partial_2d(
            &src_pts,
            &dst_pts,
            &mut inliers,
            calib3d::RANSAC,
            3.0,
            2000,
            0.99,
            10,
        )
        .map_err(|_| FaceEngineError::InferenceFailed)?;

        if m.empty() {
            return Err(FaceEngineError::InferenceFailed);
        }

        let mut aligned = Mat::default();
        imgproc::warp_affine(
            &image,
            &mut aligned,
            &m,
            Size::new(112, 112),
            imgproc::INTER_LINEAR,
            core::BORDER_CONSTANT,
            Scalar::all(0.0),
        )
        .map_err(|_| FaceEngineError::InferenceFailed)?;

        Ok(aligned)
    }

    fn preprocess_mat_to_tensor(&self, mat: &Mat) -> Result<Array4<f32>, FaceEngineError> {
        // Mat is 112x112x3 RGB
        let mut tensor = Array::zeros((1, 3, 112, 112));

        let data = mat
            .data_typed::<core::Vec3b>()
            .map_err(|_| FaceEngineError::InferenceFailed)?;

        for y in 0..112 {
            for x in 0..112 {
                let pixel = data[y * 112 + x];
                // Normalization: (x - 127.5) / 127.5
                tensor[[0, 0, y, x]] = (pixel[0] as f32 - 127.5) / 127.5; // R
                tensor[[0, 1, y, x]] = (pixel[1] as f32 - 127.5) / 127.5; // G
                tensor[[0, 2, y, x]] = (pixel[2] as f32 - 127.5) / 127.5; // B
            }
        }

        Ok(tensor)
    }
}

impl FaceRecognitionModelProvider for OrtGhostFaceNetProvider {
    fn load_recognition_model(&mut self) -> Result<(), FaceEngineError> {
        if !self.config.model_path.exists() {
            return Err(FaceEngineError::ModelPathMissing);
        }

        let session = Session::builder()
            .map_err(|_| FaceEngineError::ModelLoadFailed)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|_| FaceEngineError::ModelLoadFailed)?
            .with_intra_threads(4)
            .map_err(|_| FaceEngineError::ModelLoadFailed)?
            .commit_from_file(&self.config.model_path)
            .map_err(|_| FaceEngineError::ModelLoadFailed)?;

        self.session = Some(session);
        Ok(())
    }

    fn unload_recognition_model(&mut self) {
        self.session = None;
    }

    fn recognition_model(&self) -> &FaceModelDescriptor {
        &self.config.model
    }

    fn extract(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FaceEmbedding, FaceEngineError> {
        let aligned = self.align_crop(frame, face)?;
        let array = self.preprocess_mat_to_tensor(&aligned)?;

        let tensor = Tensor::from_array(array).map_err(|_| FaceEngineError::InferenceFailed)?;

        let session = self
            .session
            .as_mut()
            .ok_or(FaceEngineError::ModelNotLoaded)?;

        // In ORT v2, positional inputs can be passed via inputs! macro
        let outputs = session
            .run(ort::inputs![tensor])
            .map_err(|_| FaceEngineError::InferenceFailed)?;

        let (_shape, slice) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|_| FaceEngineError::InferenceFailed)?;

        let values: Vec<f32> = slice.to_vec();

        // Normalize the embedding (L2 normalization is critical for cosine similarity threshold matching)
        let norm: f32 = values.iter().map(|v| v * v).sum::<f32>().sqrt();
        let values = if norm > 0.0 {
            values.into_iter().map(|v| v / norm).collect()
        } else {
            values
        };

        FaceEmbedding::new(values)
    }

    fn compare(&self, enrolled: &FaceEmbedding, candidate: &FaceEmbedding) -> FaceMatch {
        let score = cosine_similarity(&enrolled.values, &candidate.values).unwrap_or(0.0);

        // Sometimes cosine similarity with L2-normalized vectors is calculated directly via dot product,
        // cosine_similarity function usually computes it.
        let decision = if score >= self.config.match_threshold {
            FaceMatchDecision::MatchAccepted
        } else {
            FaceMatchDecision::MatchRejectedBelowThreshold
        };

        FaceMatch { score, decision }
    }
}

// Utility function to convert VideoFrame directly to RGB OpenCV Mat for consistency
fn video_frame_to_rgb_mat(frame: &VideoFrame) -> Result<Mat, FaceEngineError> {
    let channels = match frame.format {
        PixelFormat::Bgr8 => 3,
        PixelFormat::Rgb8 => 3,
        PixelFormat::Gray8 => 1,
    };
    let mat = Mat::from_slice(&frame.data).map_err(|_| FaceEngineError::InvalidFrame)?;
    let mat = mat
        .reshape(channels, frame.height as i32)
        .map_err(|_| FaceEngineError::InvalidFrame)?;
    let mut mat = mat.try_clone().map_err(|_| FaceEngineError::InvalidFrame)?;

    if frame.format == PixelFormat::Bgr8 {
        let mut rgb = Mat::default();
        imgproc::cvt_color(
            &mat,
            &mut rgb,
            imgproc::COLOR_BGR2RGB,
            0,
            core::AlgorithmHint::ALGO_HINT_DEFAULT,
        )
        .map_err(|_| FaceEngineError::InvalidFrame)?;
        mat = rgb;
    } else if frame.format == PixelFormat::Gray8 {
        let mut rgb = Mat::default();
        imgproc::cvt_color(
            &mat,
            &mut rgb,
            imgproc::COLOR_GRAY2RGB,
            0,
            core::AlgorithmHint::ALGO_HINT_DEFAULT,
        )
        .map_err(|_| FaceEngineError::InvalidFrame)?;
        mat = rgb;
    }

    Ok(mat)
}
