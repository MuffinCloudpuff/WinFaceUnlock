use video_provider::VideoFrame;

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

pub trait FaceModelProvider {
    fn load_models(&mut self) -> Result<(), FaceEngineError>;
    fn unload_models(&mut self);
    fn detect(&mut self, frame: &VideoFrame) -> Result<Vec<DetectedFace>, FaceEngineError>;
    fn extract(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FaceEmbedding, FaceEngineError>;
    fn compare(&self, enrolled: &FaceEmbedding, candidate: &FaceEmbedding) -> FaceMatch;
}
