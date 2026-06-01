use std::path::PathBuf;

use opencv::{
    core::{self, AlgorithmHint, Mat, MatTraitConst, Size},
    imgcodecs, imgproc,
    objdetect::{FaceDetectorYN, FaceDetectorYNTrait, FaceRecognizerSF, FaceRecognizerSFTrait},
    prelude::{FaceRecognizerSFTraitConst, MatTraitConstManual},
};
use video_provider::{PixelFormat, VideoFrame};

use crate::{
    DetectedFace, FaceBox, FaceDetectionModelProvider, FaceEmbedding, FaceEngineError,
    FaceLandmark, FaceMatch, FaceMatchDecision, FaceModelDescriptor, FaceModelProvider,
    FaceRecognitionModelProvider, cosine_similarity,
};

pub const SFACE_COSINE_MATCH_THRESHOLD: f32 = 0.363;
const YUNET_INPUT_WIDTH: i32 = 320;
const YUNET_INPUT_HEIGHT: i32 = 320;

#[derive(Clone, Debug)]
pub struct OpenCvYuNetDetectorConfig {
    pub model_path: PathBuf,
    pub score_threshold: f32,
    pub nms_threshold: f32,
    pub top_k: i32,
}

impl OpenCvYuNetDetectorConfig {
    pub fn new(model_path: PathBuf) -> Self {
        Self {
            model_path,
            score_threshold: 0.9,
            nms_threshold: 0.3,
            top_k: 5000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct OpenCvSFaceRecognizerConfig {
    pub model_path: PathBuf,
    pub model: FaceModelDescriptor,
    pub match_threshold: f32,
}

impl OpenCvSFaceRecognizerConfig {
    pub fn new(model_path: PathBuf) -> Self {
        Self {
            model_path,
            model: FaceModelDescriptor {
                model_family: "sface".to_owned(),
                model_version: "2021dec".to_owned(),
            },
            match_threshold: SFACE_COSINE_MATCH_THRESHOLD,
        }
    }
}

#[derive(Clone, Debug)]
pub struct OpenCvFaceModelConfig {
    pub detector: OpenCvYuNetDetectorConfig,
    pub recognizer: OpenCvSFaceRecognizerConfig,
}

impl OpenCvFaceModelConfig {
    pub fn new(yunet_model_path: PathBuf, sface_model_path: PathBuf) -> Self {
        Self {
            detector: OpenCvYuNetDetectorConfig::new(yunet_model_path),
            recognizer: OpenCvSFaceRecognizerConfig::new(sface_model_path),
        }
    }
}

pub struct OpenCvYuNetDetectorProvider {
    config: OpenCvYuNetDetectorConfig,
    detector: Option<core::Ptr<FaceDetectorYN>>,
}

impl OpenCvYuNetDetectorProvider {
    pub fn new(config: OpenCvYuNetDetectorConfig) -> Self {
        Self {
            config,
            detector: None,
        }
    }

    fn detector_mut(&mut self) -> Result<&mut core::Ptr<FaceDetectorYN>, FaceEngineError> {
        self.detector
            .as_mut()
            .ok_or(FaceEngineError::ModelNotLoaded)
    }
}

impl FaceDetectionModelProvider for OpenCvYuNetDetectorProvider {
    fn load_detection_model(&mut self) -> Result<(), FaceEngineError> {
        if !self.config.model_path.exists() {
            return Err(FaceEngineError::ModelPathMissing);
        }

        let model_path = self.config.model_path.to_string_lossy();
        let detector = FaceDetectorYN::create(
            &model_path,
            "",
            Size::new(YUNET_INPUT_WIDTH, YUNET_INPUT_HEIGHT),
            self.config.score_threshold,
            self.config.nms_threshold,
            self.config.top_k,
            0,
            0,
        )
        .map_err(|_| FaceEngineError::ModelLoadFailed)?;

        self.detector = Some(detector);
        Ok(())
    }

    fn unload_detection_model(&mut self) {
        self.detector = None;
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
}

pub struct OpenCvSFaceRecognitionProvider {
    config: OpenCvSFaceRecognizerConfig,
    recognizer: Option<core::Ptr<FaceRecognizerSF>>,
}

impl OpenCvSFaceRecognitionProvider {
    pub fn new(config: OpenCvSFaceRecognizerConfig) -> Self {
        Self {
            config,
            recognizer: None,
        }
    }

    fn recognizer_mut(&mut self) -> Result<&mut core::Ptr<FaceRecognizerSF>, FaceEngineError> {
        self.recognizer
            .as_mut()
            .ok_or(FaceEngineError::ModelNotLoaded)
    }
}

impl FaceRecognitionModelProvider for OpenCvSFaceRecognitionProvider {
    fn load_recognition_model(&mut self) -> Result<(), FaceEngineError> {
        if !self.config.model_path.exists() {
            return Err(FaceEngineError::ModelPathMissing);
        }

        let model_path = self.config.model_path.to_string_lossy();
        let recognizer = FaceRecognizerSF::create_def(&model_path, "")
            .map_err(|_| FaceEngineError::ModelLoadFailed)?;

        self.recognizer = Some(recognizer);
        Ok(())
    }

    fn unload_recognition_model(&mut self) {
        self.recognizer = None;
    }

    fn recognition_model(&self) -> &FaceModelDescriptor {
        &self.config.model
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

pub struct FaceModelPipeline<D, R> {
    detector: D,
    recognizer: R,
}

pub type HotSwappableFaceModelPipeline =
    FaceModelPipeline<Box<dyn FaceDetectionModelProvider>, Box<dyn FaceRecognitionModelProvider>>;

impl<D, R> FaceModelPipeline<D, R> {
    pub fn new(detector: D, recognizer: R) -> Self {
        Self {
            detector,
            recognizer,
        }
    }

    pub fn detector(&self) -> &D {
        &self.detector
    }

    pub fn recognizer(&self) -> &R {
        &self.recognizer
    }
}

impl<D, R> FaceModelPipeline<D, R>
where
    D: FaceDetectionModelProvider,
    R: FaceRecognitionModelProvider,
{
    pub fn swap_detector(&mut self, mut detector: D) -> Result<D, FaceEngineError> {
        detector.load_detection_model()?;
        let mut previous_detector = std::mem::replace(&mut self.detector, detector);
        previous_detector.unload_detection_model();
        Ok(previous_detector)
    }

    pub fn swap_recognizer(&mut self, mut recognizer: R) -> Result<R, FaceEngineError> {
        recognizer.load_recognition_model()?;
        let mut previous_recognizer = std::mem::replace(&mut self.recognizer, recognizer);
        previous_recognizer.unload_recognition_model();
        Ok(previous_recognizer)
    }
}

impl<D, R> FaceModelProvider for FaceModelPipeline<D, R>
where
    D: FaceDetectionModelProvider,
    R: FaceRecognitionModelProvider,
{
    fn load_models(&mut self) -> Result<(), FaceEngineError> {
        self.detector.load_detection_model()?;
        if let Err(error) = self.recognizer.load_recognition_model() {
            self.detector.unload_detection_model();
            return Err(error);
        }
        Ok(())
    }

    fn unload_models(&mut self) {
        self.detector.unload_detection_model();
        self.recognizer.unload_recognition_model();
    }

    fn recognition_model(&self) -> &FaceModelDescriptor {
        self.recognizer.recognition_model()
    }

    fn detect(&mut self, frame: &VideoFrame) -> Result<Vec<DetectedFace>, FaceEngineError> {
        self.detector.detect(frame)
    }

    fn extract(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FaceEmbedding, FaceEngineError> {
        self.recognizer.extract(frame, face)
    }

    fn compare(&self, enrolled: &FaceEmbedding, candidate: &FaceEmbedding) -> FaceMatch {
        self.recognizer.compare(enrolled, candidate)
    }
}

pub struct OpenCvFaceModelProvider {
    pipeline: FaceModelPipeline<OpenCvYuNetDetectorProvider, OpenCvSFaceRecognitionProvider>,
}

impl OpenCvFaceModelProvider {
    pub fn new(config: OpenCvFaceModelConfig) -> Self {
        Self {
            pipeline: FaceModelPipeline::new(
                OpenCvYuNetDetectorProvider::new(config.detector),
                OpenCvSFaceRecognitionProvider::new(config.recognizer),
            ),
        }
    }

    pub fn read_image_frame(image_path: &str) -> Result<VideoFrame, FaceEngineError> {
        let image = imgcodecs::imread_def(image_path).map_err(|_| FaceEngineError::InvalidFrame)?;
        mat_to_video_frame(&image).map_err(|_| FaceEngineError::InvalidFrame)
    }

    pub fn swap_detector(
        &mut self,
        detector: OpenCvYuNetDetectorProvider,
    ) -> Result<OpenCvYuNetDetectorProvider, FaceEngineError> {
        self.pipeline.swap_detector(detector)
    }

    pub fn swap_recognizer(
        &mut self,
        recognizer: OpenCvSFaceRecognitionProvider,
    ) -> Result<OpenCvSFaceRecognitionProvider, FaceEngineError> {
        self.pipeline.swap_recognizer(recognizer)
    }
}

impl FaceModelProvider for OpenCvFaceModelProvider {
    fn load_models(&mut self) -> Result<(), FaceEngineError> {
        self.pipeline.load_models()
    }

    fn unload_models(&mut self) {
        self.pipeline.unload_models();
    }

    fn recognition_model(&self) -> &FaceModelDescriptor {
        self.pipeline.recognition_model()
    }

    fn detect(&mut self, frame: &VideoFrame) -> Result<Vec<DetectedFace>, FaceEngineError> {
        self.pipeline.detect(frame)
    }

    fn extract(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FaceEmbedding, FaceEngineError> {
        self.pipeline.extract(frame, face)
    }

    fn compare(&self, enrolled: &FaceEmbedding, candidate: &FaceEmbedding) -> FaceMatch {
        self.pipeline.compare(enrolled, candidate)
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

#[cfg(test)]
mod tests {
    use super::*;

    struct StubDetector {
        loaded: bool,
        load_error: Option<FaceEngineError>,
    }

    impl FaceDetectionModelProvider for StubDetector {
        fn load_detection_model(&mut self) -> Result<(), FaceEngineError> {
            if let Some(error) = &self.load_error {
                return Err(error.clone());
            }
            self.loaded = true;
            Ok(())
        }

        fn unload_detection_model(&mut self) {
            self.loaded = false;
        }

        fn detect(&mut self, _frame: &VideoFrame) -> Result<Vec<DetectedFace>, FaceEngineError> {
            Ok(Vec::new())
        }
    }

    struct StubRecognizer {
        loaded: bool,
        model: FaceModelDescriptor,
        load_error: Option<FaceEngineError>,
    }

    impl FaceRecognitionModelProvider for StubRecognizer {
        fn load_recognition_model(&mut self) -> Result<(), FaceEngineError> {
            if let Some(error) = &self.load_error {
                return Err(error.clone());
            }
            self.loaded = true;
            Ok(())
        }

        fn unload_recognition_model(&mut self) {
            self.loaded = false;
        }

        fn recognition_model(&self) -> &FaceModelDescriptor {
            &self.model
        }

        fn extract(
            &mut self,
            _frame: &VideoFrame,
            _face: &DetectedFace,
        ) -> Result<FaceEmbedding, FaceEngineError> {
            FaceEmbedding::new(vec![1.0])
        }

        fn compare(&self, enrolled: &FaceEmbedding, candidate: &FaceEmbedding) -> FaceMatch {
            FaceMatch {
                score: cosine_similarity(&enrolled.values, &candidate.values).unwrap_or(0.0),
                decision: FaceMatchDecision::MatchAccepted,
            }
        }
    }

    #[test]
    fn pipeline_exposes_recognition_model_without_detector_metadata() -> Result<(), FaceEngineError>
    {
        let detector = StubDetector {
            loaded: false,
            load_error: None,
        };
        let recognizer = StubRecognizer {
            loaded: false,
            model: FaceModelDescriptor {
                model_family: "recognizer-family".to_owned(),
                model_version: "recognizer-version".to_owned(),
            },
            load_error: None,
        };
        let mut pipeline = FaceModelPipeline::new(detector, recognizer);

        pipeline.load_models()?;

        assert_eq!(
            pipeline.recognition_model(),
            &FaceModelDescriptor {
                model_family: "recognizer-family".to_owned(),
                model_version: "recognizer-version".to_owned(),
            }
        );
        assert!(pipeline.detector().loaded);
        assert!(pipeline.recognizer().loaded);
        Ok(())
    }

    #[test]
    fn detector_swap_keeps_previous_detector_when_new_model_fails_to_load()
    -> Result<(), FaceEngineError> {
        let detector = StubDetector {
            loaded: false,
            load_error: None,
        };
        let recognizer = StubRecognizer {
            loaded: false,
            model: FaceModelDescriptor {
                model_family: "recognizer-family".to_owned(),
                model_version: "recognizer-version".to_owned(),
            },
            load_error: None,
        };
        let mut pipeline = FaceModelPipeline::new(detector, recognizer);
        pipeline.load_models()?;

        let result = pipeline.swap_detector(StubDetector {
            loaded: false,
            load_error: Some(FaceEngineError::ModelLoadFailed),
        });

        assert!(matches!(result, Err(FaceEngineError::ModelLoadFailed)));
        assert!(pipeline.detector().loaded);
        Ok(())
    }

    #[test]
    fn recognizer_swap_replaces_recognition_model_independently() -> Result<(), FaceEngineError> {
        let detector = StubDetector {
            loaded: false,
            load_error: None,
        };
        let recognizer = StubRecognizer {
            loaded: false,
            model: FaceModelDescriptor {
                model_family: "recognizer-family".to_owned(),
                model_version: "v1".to_owned(),
            },
            load_error: None,
        };
        let mut pipeline = FaceModelPipeline::new(detector, recognizer);
        pipeline.load_models()?;

        let previous = pipeline.swap_recognizer(StubRecognizer {
            loaded: false,
            model: FaceModelDescriptor {
                model_family: "recognizer-family".to_owned(),
                model_version: "v2".to_owned(),
            },
            load_error: None,
        })?;

        assert!(!previous.loaded);
        assert!(pipeline.detector().loaded);
        assert!(pipeline.recognizer().loaded);
        assert_eq!(pipeline.recognition_model().model_version, "v2");
        Ok(())
    }
}
