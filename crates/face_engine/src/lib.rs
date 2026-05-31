mod embedding;
mod opencv_model;
mod template;
mod types;

pub use embedding::cosine_similarity;
pub use opencv_model::{
    OpenCvFaceModelConfig, OpenCvFaceModelProvider, SFACE_COSINE_MATCH_THRESHOLD,
};
pub use template::{
    FaceTemplate, FaceTemplateCodecError, FaceTemplateMatch, FaceTemplateMatcher, FaceTemplateRef,
};
pub use types::{
    DetectedFace, FaceBox, FaceEmbedding, FaceEngineError, FaceLandmark, FaceMatch,
    FaceMatchDecision, FaceModelProvider,
};
