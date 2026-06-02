mod embedding;
mod opencv_model;
mod template;
mod types;

pub use embedding::cosine_similarity;
pub use opencv_model::{
    FaceModelPipeline, HotSwappableFaceModelPipeline, OpenCvFaceModelConfig,
    OpenCvFaceModelProvider, OpenCvSFaceRecognitionProvider, OpenCvSFaceRecognizerConfig,
    OpenCvYuNetDetectorConfig, OpenCvYuNetDetectorProvider, SFACE_COSINE_MATCH_THRESHOLD,
};
pub use template::{
    FacePoseGroup, FaceSampleRejectReason, FaceTemplate, FaceTemplateCodecError, FaceTemplateMatch,
    FaceTemplateMatcher, FaceTemplateQualityScores, FaceTemplateRef, FaceTemplateSampleMetadata,
    FaceTemplateSet, FaceTemplateSetQualitySummary, FaceTemplateThresholdProfile,
};
pub use types::{
    DetectedFace, FaceBox, FaceDetectionModelProvider, FaceEmbedding, FaceEngineError,
    FaceLandmark, FaceMatch, FaceMatchDecision, FaceModelDescriptor, FaceModelProvider,
    FaceRecognitionModelProvider,
};
