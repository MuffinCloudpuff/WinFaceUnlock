use common_protocol::UserId;
use face_engine::{
    FaceEngineError, FaceModelProvider, FaceTemplate, FaceTemplateRef as EngineFaceTemplateRef,
};
use video_provider::VideoFrame;

#[derive(Clone, Debug, PartialEq)]
pub struct EnrollmentOutcome {
    pub template: FaceTemplate,
    pub detected_face_count: usize,
}

pub struct FaceEnrollmentService<M> {
    model_provider: M,
}

impl<M> FaceEnrollmentService<M>
where
    M: FaceModelProvider,
{
    pub fn new(model_provider: M) -> Self {
        Self { model_provider }
    }

    pub fn enroll_frame(
        &mut self,
        frame: &VideoFrame,
        user_id: UserId,
        template_ref: EngineFaceTemplateRef,
        model_family: String,
        model_version: String,
    ) -> Result<EnrollmentOutcome, FaceEngineError> {
        let faces = self.model_provider.detect(frame)?;
        if faces.is_empty() {
            return Err(FaceEngineError::NoFaceDetected);
        }
        if faces.len() > 1 {
            return Err(FaceEngineError::MultipleFacesDetected);
        }

        let embedding = self.model_provider.extract(frame, &faces[0])?;
        Ok(EnrollmentOutcome {
            template: FaceTemplate {
                template_ref,
                user_id: user_id.0,
                model_family,
                model_version,
                embedding,
            },
            detected_face_count: faces.len(),
        })
    }
}
