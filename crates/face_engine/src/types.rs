use video_provider::VideoFrame;

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceModelDescriptor {
    pub model_family: String,
    pub model_version: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceLandmark {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetectedFace {
    pub bounds: FaceBox,
    pub landmarks: Vec<FaceLandmark>,
    pub confidence: f32,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceEmbedding {
    pub values: Vec<f32>,
}

impl FaceEmbedding {
    pub fn new(values: Vec<f32>) -> Result<Self, FaceEngineError> {
        if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
            return Err(FaceEngineError::InvalidEmbedding);
        }

        Ok(Self { values })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceMatch {
    pub score: f32,
    pub decision: FaceMatchDecision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FaceMatchDecision {
    MatchAccepted,
    MatchRejectedBelowThreshold,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FaceEngineError {
    ModelNotLoaded,
    ModelLoadFailed,
    ModelPathMissing,
    NoFaceDetected,
    MultipleFacesDetected,
    InvalidFrame,
    InvalidEmbedding,
    InferenceFailed,
}

pub trait FaceDetectionModelProvider {
    fn load_detection_model(&mut self) -> Result<(), FaceEngineError>;
    fn unload_detection_model(&mut self);
    fn detect(&mut self, frame: &VideoFrame) -> Result<Vec<DetectedFace>, FaceEngineError>;
}

impl<T> FaceDetectionModelProvider for Box<T>
where
    T: FaceDetectionModelProvider + ?Sized,
{
    fn load_detection_model(&mut self) -> Result<(), FaceEngineError> {
        (**self).load_detection_model()
    }

    fn unload_detection_model(&mut self) {
        (**self).unload_detection_model();
    }

    fn detect(&mut self, frame: &VideoFrame) -> Result<Vec<DetectedFace>, FaceEngineError> {
        (**self).detect(frame)
    }
}

pub trait FaceRecognitionModelProvider {
    fn load_recognition_model(&mut self) -> Result<(), FaceEngineError>;
    fn unload_recognition_model(&mut self);
    fn recognition_model(&self) -> &FaceModelDescriptor;
    fn extract(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FaceEmbedding, FaceEngineError>;
    fn compare(&self, enrolled: &FaceEmbedding, candidate: &FaceEmbedding) -> FaceMatch;
}

impl<T> FaceRecognitionModelProvider for Box<T>
where
    T: FaceRecognitionModelProvider + ?Sized,
{
    fn load_recognition_model(&mut self) -> Result<(), FaceEngineError> {
        (**self).load_recognition_model()
    }

    fn unload_recognition_model(&mut self) {
        (**self).unload_recognition_model();
    }

    fn recognition_model(&self) -> &FaceModelDescriptor {
        (**self).recognition_model()
    }

    fn extract(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FaceEmbedding, FaceEngineError> {
        (**self).extract(frame, face)
    }

    fn compare(&self, enrolled: &FaceEmbedding, candidate: &FaceEmbedding) -> FaceMatch {
        (**self).compare(enrolled, candidate)
    }
}

pub trait FaceModelProvider {
    fn load_models(&mut self) -> Result<(), FaceEngineError>;
    fn unload_models(&mut self);
    fn recognition_model(&self) -> &FaceModelDescriptor;
    fn detect(&mut self, frame: &VideoFrame) -> Result<Vec<DetectedFace>, FaceEngineError>;
    fn extract(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FaceEmbedding, FaceEngineError>;
    fn compare(&self, enrolled: &FaceEmbedding, candidate: &FaceEmbedding) -> FaceMatch;
}
