use std::path::PathBuf;

use crate::opencv_model::{OpenCvYuNetDetectorConfig, OpenCvYuNetDetectorProvider};
use crate::ort_model::{OrtGhostFaceNetConfig, OrtGhostFaceNetProvider};
use crate::{
    DetectedFace, FaceEmbedding, FaceEngineError, FaceMatch, FaceModelDescriptor,
    FaceModelPipeline, FaceModelProvider,
};
use video_provider::VideoFrame;

#[derive(Clone, Debug)]
pub struct HybridFaceModelConfig {
    pub detector: OpenCvYuNetDetectorConfig,
    pub recognizer: OrtGhostFaceNetConfig,
}

impl HybridFaceModelConfig {
    pub fn new(yunet_model_path: PathBuf, ghostfacenet_model_path: PathBuf) -> Self {
        Self {
            detector: OpenCvYuNetDetectorConfig::new(yunet_model_path),
            recognizer: OrtGhostFaceNetConfig::new(ghostfacenet_model_path),
        }
    }
}

pub struct HybridFaceModelProvider {
    pipeline: FaceModelPipeline<OpenCvYuNetDetectorProvider, OrtGhostFaceNetProvider>,
}

impl HybridFaceModelProvider {
    pub fn new(config: HybridFaceModelConfig) -> Self {
        Self {
            pipeline: FaceModelPipeline::new(
                OpenCvYuNetDetectorProvider::new(config.detector),
                OrtGhostFaceNetProvider::new(config.recognizer),
            ),
        }
    }

    pub fn swap_detector(
        &mut self,
        detector: OpenCvYuNetDetectorProvider,
    ) -> Result<OpenCvYuNetDetectorProvider, FaceEngineError> {
        self.pipeline.swap_detector(detector)
    }

    pub fn swap_recognizer(
        &mut self,
        recognizer: OrtGhostFaceNetProvider,
    ) -> Result<OrtGhostFaceNetProvider, FaceEngineError> {
        self.pipeline.swap_recognizer(recognizer)
    }

    pub fn read_image_frame(image_path: &str) -> Result<VideoFrame, FaceEngineError> {
        crate::opencv_model::OpenCvFaceModelProvider::read_image_frame(image_path)
    }

    pub fn write_detection_debug_frame(
        frame: &VideoFrame,
        faces: &[DetectedFace],
        image_path: &std::path::Path,
    ) -> Result<(), FaceEngineError> {
        crate::opencv_model::OpenCvFaceModelProvider::write_detection_debug_frame(
            frame, faces, image_path,
        )
    }

    pub fn write_aligned_face(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
        image_path: &std::path::Path,
    ) -> Result<(), FaceEngineError> {
        // Since align_crop in ORT provider returns a Mat, we'd need it to be pub(crate).
        // Let's implement it by extracting the Mat first, or we can just ask the recognizer.
        let aligned_face = self.pipeline.recognizer().align_crop(frame, face)?;

        let path = image_path.to_string_lossy();
        opencv::imgcodecs::imwrite(&path, &aligned_face, &opencv::core::Vector::new())
            .map_err(|_| FaceEngineError::InferenceFailed)?
            .then_some(())
            .ok_or(FaceEngineError::InferenceFailed)
    }
}

impl FaceModelProvider for HybridFaceModelProvider {
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
