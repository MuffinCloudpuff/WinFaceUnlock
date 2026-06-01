use crate::{FaceEmbedding, FaceMatch, FaceMatchDecision, FaceModelDescriptor, cosine_similarity};

#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceTemplateRef(pub String);

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceTemplate {
    pub template_ref: FaceTemplateRef,
    pub user_id: String,
    pub model_family: String,
    pub model_version: String,
    pub embedding: FaceEmbedding,
}

impl FaceTemplate {
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, FaceTemplateCodecError> {
        serde_json::to_vec(self).map_err(|_| FaceTemplateCodecError::SerializeFailed)
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, FaceTemplateCodecError> {
        serde_json::from_slice(bytes).map_err(|_| FaceTemplateCodecError::DeserializeFailed)
    }

    pub fn is_compatible_with(&self, recognition_model: &FaceModelDescriptor) -> bool {
        self.model_family == recognition_model.model_family
            && self.model_version == recognition_model.model_version
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FaceTemplateCodecError {
    SerializeFailed,
    DeserializeFailed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceTemplateMatch {
    pub template_ref: FaceTemplateRef,
    pub user_id: String,
    pub score: f32,
    pub decision: FaceMatchDecision,
}

pub struct FaceTemplateMatcher {
    threshold: f32,
}

impl FaceTemplateMatcher {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }

    pub fn compare_embeddings(
        &self,
        enrolled: &FaceEmbedding,
        candidate: &FaceEmbedding,
    ) -> FaceMatch {
        let score = cosine_similarity(&enrolled.values, &candidate.values).unwrap_or(0.0);
        let decision = if score >= self.threshold {
            FaceMatchDecision::MatchAccepted
        } else {
            FaceMatchDecision::MatchRejectedBelowThreshold
        };

        FaceMatch { score, decision }
    }

    pub fn best_match(
        &self,
        templates: &[FaceTemplate],
        candidate: &FaceEmbedding,
    ) -> Option<FaceTemplateMatch> {
        templates
            .iter()
            .map(|template| {
                let face_match = self.compare_embeddings(&template.embedding, candidate);
                FaceTemplateMatch {
                    template_ref: template.template_ref.clone(),
                    user_id: template.user_id.clone(),
                    score: face_match.score,
                    decision: face_match.decision,
                }
            })
            .max_by(|left, right| left.score.total_cmp(&right.score))
    }

    pub fn best_compatible_match(
        &self,
        templates: &[FaceTemplate],
        recognition_model: &FaceModelDescriptor,
        candidate: &FaceEmbedding,
    ) -> Option<FaceTemplateMatch> {
        let compatible_templates = templates
            .iter()
            .filter(|template| template.is_compatible_with(recognition_model));

        compatible_templates
            .map(|template| {
                let face_match = self.compare_embeddings(&template.embedding, candidate);
                FaceTemplateMatch {
                    template_ref: template.template_ref.clone(),
                    user_id: template.user_id.clone(),
                    score: face_match.score,
                    decision: face_match.decision,
                }
            })
            .max_by(|left, right| left.score.total_cmp(&right.score))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_round_trips_as_json_bytes() -> Result<(), FaceTemplateCodecError> {
        let template = FaceTemplate {
            template_ref: FaceTemplateRef("face-1".to_owned()),
            user_id: "user-1".to_owned(),
            model_family: "sface".to_owned(),
            model_version: "2021dec".to_owned(),
            embedding: FaceEmbedding {
                values: vec![1.0, 0.0, 1.0],
            },
        };

        let encoded = template.to_json_bytes()?;
        let decoded = FaceTemplate::from_json_bytes(&encoded)?;

        assert_eq!(decoded, template);
        Ok(())
    }

    #[test]
    fn best_match_uses_explicit_threshold_decision() -> Result<(), &'static str> {
        let matcher = FaceTemplateMatcher::new(0.82);
        let templates = vec![FaceTemplate {
            template_ref: FaceTemplateRef("face-1".to_owned()),
            user_id: "user-1".to_owned(),
            model_family: "sface".to_owned(),
            model_version: "2021dec".to_owned(),
            embedding: FaceEmbedding {
                values: vec![1.0, 0.0, 1.0],
            },
        }];

        let matched = matcher.best_match(
            &templates,
            &FaceEmbedding {
                values: vec![1.0, 0.0, 1.0],
            },
        );
        let matched = matched.ok_or("test template should match")?;

        assert_eq!(matched.decision, FaceMatchDecision::MatchAccepted);
        assert_eq!(matched.template_ref, FaceTemplateRef("face-1".to_owned()));
        Ok(())
    }

    #[test]
    fn compatible_match_ignores_template_from_another_recognition_model() {
        let matcher = FaceTemplateMatcher::new(0.82);
        let templates = vec![FaceTemplate {
            template_ref: FaceTemplateRef("face-1".to_owned()),
            user_id: "user-1".to_owned(),
            model_family: "other-recognizer".to_owned(),
            model_version: "v2".to_owned(),
            embedding: FaceEmbedding {
                values: vec![1.0, 0.0, 1.0],
            },
        }];

        let matched = matcher.best_compatible_match(
            &templates,
            &FaceModelDescriptor {
                model_family: "sface".to_owned(),
                model_version: "2021dec".to_owned(),
            },
            &FaceEmbedding {
                values: vec![1.0, 0.0, 1.0],
            },
        );

        assert_eq!(matched, None);
    }
}
