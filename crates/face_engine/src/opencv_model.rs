use std::path::PathBuf;

use opencv::{
    core::{self, AlgorithmHint, Mat, MatTraitConst, Size},
    imgcodecs, imgproc,
    objdetect::{FaceDetectorYN, FaceDetectorYNTrait, FaceRecognizerSF, FaceRecognizerSFTrait},
    prelude::{FaceRecognizerSFTraitConst, MatTraitConstManual},
};
use video_provider::{PixelFormat, VideoFrame};

use crate::{
    DetectedFace, FaceBox, FaceEmbedding, FaceEngineError, FaceLandmark, FaceMatch,
    FaceMatchDecision, FaceModelProvider, cosine_similarity,
};

pub const SFACE_COSINE_MATCH_THRESHOLD: f32 = 0.363;
const YUNET_INPUT_WIDTH: i32 = 320;
const YUNET_INPUT_HEIGHT: i32 = 320;

#[derive(Clone, Debug)]
pub struct OpenCvFaceModelConfig {
    pub yunet_model_path: PathBuf,
    pub sface_model_path: PathBuf,
    pub score_threshold: f32,
    pub nms_threshold: f32,
    pub top_k: i32,
    pub match_threshold: f32,
}

impl OpenCvFaceModelConfig {
    pub fn new(yunet_model_path: PathBuf, sface_model_path: PathBuf) -> Self {
        Self {
            yunet_model_path,
            sface_model_path,
            score_threshold: 0.9,
            nms_threshold: 0.3,
            top_k: 5000,
            match_threshold: SFACE_COSINE_MATCH_THRESHOLD,
        }
    }
}

pub struct OpenCvFaceModelProvider {
    config: OpenCvFaceModelConfig,
    detector: Option<core::Ptr<FaceDetectorYN>>,
    recognizer: Option<core::Ptr<FaceRecognizerSF>>,
}

impl OpenCvFaceModelProvider {
    pub fn new(config: OpenCvFaceModelConfig) -> Self {
        Self {
            config,
            detector: None,
            recognizer: None,
        }
    }

    pub fn read_image_frame(image_path: &str) -> Result<VideoFrame, FaceEngineError> {
        let image = imgcodecs::imread_def(image_path).map_err(|_| FaceEngineError::InvalidFrame)?;
        mat_to_video_frame(&image).map_err(|_| FaceEngineError::InvalidFrame)
    }

    fn detector_mut(&mut self) -> Result<&mut core::Ptr<FaceDetectorYN>, FaceEngineError> {
        self.detector
            .as_mut()
            .ok_or(FaceEngineError::ModelNotLoaded)
    }

    fn recognizer_mut(&mut self) -> Result<&mut core::Ptr<FaceRecognizerSF>, FaceEngineError> {
        self.recognizer
            .as_mut()
            .ok_or(FaceEngineError::ModelNotLoaded)
    }
}

impl FaceModelProvider for OpenCvFaceModelProvider {
    fn load_models(&mut self) -> Result<(), FaceEngineError> {
        if !self.config.yunet_model_path.exists() || !self.config.sface_model_path.exists() {
            return Err(FaceEngineError::ModelPathMissing);
        }

        let yunet_model_path = self.config.yunet_model_path.to_string_lossy();
        let sface_model_path = self.config.sface_model_path.to_string_lossy();
        let detector = FaceDetectorYN::create(
            &yunet_model_path,
            "",
            Size::new(YUNET_INPUT_WIDTH, YUNET_INPUT_HEIGHT),
            self.config.score_threshold,
            self.config.nms_threshold,
            self.config.top_k,
            0,
            0,
        )
        .map_err(|_| FaceEngineError::ModelLoadFailed)?;
        let recognizer = FaceRecognizerSF::create_def(&sface_model_path, "")
            .map_err(|_| FaceEngineError::ModelLoadFailed)?;

        self.detector = Some(detector);
        self.recognizer = Some(recognizer);
        Ok(())
    }

    fn unload_models(&mut self) {
        self.detector = None;
        self.recognizer = None;
    }

    fn detect(&mut self, frame: &VideoFrame) -> Result<Vec<DetectedFace>, FaceEngineError> {
        frame
            .validate()
            .map_err(|_| FaceEngineError::InvalidFrame)?;
        let image = video_frame_to_mat(frame)?;
        let image_size = image.size().map_err(|_| FaceEngineError::InvalidFrame)?;
        let detector = self.detector_mut()?;
        detector
            .set_input_size(image_size)
            .map_err(|_| FaceEngineError::InferenceFailed)?;

        let mut faces = Mat::default();
        detector
            .detect(&image, &mut faces)
            .map_err(|_| FaceEngineError::InferenceFailed)?;

        detected_faces_from_mat(&faces)
    }

    fn extract(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FaceEmbedding, FaceEngineError> {
        frame
            .validate()
            .map_err(|_| FaceEngineError::InvalidFrame)?;
        let image = video_frame_to_mat(frame)?;
        let face_row = detected_face_to_mat(face)?;
        let recognizer = self.recognizer_mut()?;

        let mut aligned_face = Mat::default();
        recognizer
            .align_crop(&image, &face_row, &mut aligned_face)
            .map_err(|_| FaceEngineError::InferenceFailed)?;

        let mut feature = Mat::default();
        recognizer
            .feature(&aligned_face, &mut feature)
            .map_err(|_| FaceEngineError::InferenceFailed)?;
        embedding_from_feature_mat(&feature)
    }

    fn compare(&self, enrolled: &FaceEmbedding, candidate: &FaceEmbedding) -> FaceMatch {
        let score = cosine_similarity(&enrolled.values, &candidate.values).unwrap_or(0.0);
        let decision = if score >= self.config.match_threshold {
            FaceMatchDecision::MatchAccepted
        } else {
            FaceMatchDecision::MatchRejectedBelowThreshold
        };

        FaceMatch { score, decision }
    }
}

fn video_frame_to_mat(frame: &VideoFrame) -> Result<Mat, FaceEngineError> {
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

    if frame.format == PixelFormat::Rgb8 {
        let mut bgr = Mat::default();
        imgproc::cvt_color(
            &mat,
            &mut bgr,
            imgproc::COLOR_RGB2BGR,
            0,
            AlgorithmHint::ALGO_HINT_DEFAULT,
        )
        .map_err(|_| FaceEngineError::InvalidFrame)?;
        mat = bgr;
    }

    Ok(mat)
}

fn mat_to_video_frame(mat: &Mat) -> Result<VideoFrame, FaceEngineError> {
    if mat.empty() || mat.cols() <= 0 || mat.rows() <= 0 {
        return Err(FaceEngineError::InvalidFrame);
    }

    let format = match mat.channels() {
        1 => PixelFormat::Gray8,
        3 => PixelFormat::Bgr8,
        _ => return Err(FaceEngineError::InvalidFrame),
    };

    let data = mat
        .data_bytes()
        .map_err(|_| FaceEngineError::InvalidFrame)?
        .to_vec();
    let frame = VideoFrame {
        width: mat.cols() as u32,
        height: mat.rows() as u32,
        format,
        data,
    };
    frame
        .validate()
        .map_err(|_| FaceEngineError::InvalidFrame)?;
    Ok(frame)
}

fn detected_faces_from_mat(faces: &Mat) -> Result<Vec<DetectedFace>, FaceEngineError> {
    if faces.empty() || faces.rows() <= 0 {
        return Ok(Vec::new());
    }

    let mut detected = Vec::with_capacity(faces.rows() as usize);
    for row_index in 0..faces.rows() {
        let row = faces
            .row(row_index)
            .map_err(|_| FaceEngineError::InferenceFailed)?;
        let values = row
            .data_typed::<f32>()
            .map_err(|_| FaceEngineError::InferenceFailed)?;
        if values.len() < 15 {
            return Err(FaceEngineError::InferenceFailed);
        }

        detected.push(DetectedFace {
            bounds: FaceBox {
                x: values[0],
                y: values[1],
                width: values[2],
                height: values[3],
            },
            landmarks: vec![
                FaceLandmark {
                    x: values[4],
                    y: values[5],
                },
                FaceLandmark {
                    x: values[6],
                    y: values[7],
                },
                FaceLandmark {
                    x: values[8],
                    y: values[9],
                },
                FaceLandmark {
                    x: values[10],
                    y: values[11],
                },
                FaceLandmark {
                    x: values[12],
                    y: values[13],
                },
            ],
            confidence: values[14],
        });
    }

    Ok(detected)
}

fn detected_face_to_mat(face: &DetectedFace) -> Result<Mat, FaceEngineError> {
    if face.landmarks.len() < 5 {
        return Err(FaceEngineError::InferenceFailed);
    }

    let values = [
        face.bounds.x,
        face.bounds.y,
        face.bounds.width,
        face.bounds.height,
        face.landmarks[0].x,
        face.landmarks[0].y,
        face.landmarks[1].x,
        face.landmarks[1].y,
        face.landmarks[2].x,
        face.landmarks[2].y,
        face.landmarks[3].x,
        face.landmarks[3].y,
        face.landmarks[4].x,
        face.landmarks[4].y,
        face.confidence,
    ];
    let mat = Mat::from_slice(&values).map_err(|_| FaceEngineError::InferenceFailed)?;
    mat.try_clone()
        .map_err(|_| FaceEngineError::InferenceFailed)
}

fn embedding_from_feature_mat(feature: &Mat) -> Result<FaceEmbedding, FaceEngineError> {
    let values = feature
        .data_typed::<f32>()
        .map_err(|_| FaceEngineError::InferenceFailed)?
        .to_vec();
    FaceEmbedding::new(values)
}
