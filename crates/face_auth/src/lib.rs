mod authenticator;
#[cfg(feature = "enrollment")]
mod enrollment;
#[cfg(feature = "enrollment")]
mod enrollment_steps;
#[cfg(feature = "enrollment")]
mod guided_enrollment;
mod policy;
#[cfg(feature = "enrollment")]
mod quality;

pub use authenticator::{AuthenticationOutcome, FaceAuthenticator, RecognitionTemplates};
#[cfg(feature = "enrollment")]
pub use enrollment::{EnrollmentOutcome, FaceEnrollmentService};
#[cfg(feature = "enrollment")]
pub use enrollment_steps::GuidedEnrollmentStep;
#[cfg(feature = "enrollment")]
pub use guided_enrollment::{
    GuidedEnrollmentConfig, GuidedEnrollmentReport, GuidedFaceEnrollmentService,
    GuidedFrameObservation, PoseGroupCount, RejectReasonCount, build_guided_enrollment_report,
};
pub use policy::{AttemptPolicy, AttemptPolicyConfig, AttemptPolicyDecision, AttemptPolicyState};
#[cfg(feature = "enrollment")]
pub use quality::{FaceQualityPolicy, reject_reason_for_quality, score_face_sample};
