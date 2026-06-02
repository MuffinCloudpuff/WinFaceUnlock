use crate::{LivenessDecision, LivenessResult};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LivenessPolicyDecision {
    ContinueRecognition,
    RejectAsSpoof,
    ContinueBecauseProviderUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LivenessPolicy {
    pub reject_on_spoof_evidence: bool,
    pub provider_unavailable_allows_recognition: bool,
}

impl Default for LivenessPolicy {
    fn default() -> Self {
        Self {
            reject_on_spoof_evidence: true,
            provider_unavailable_allows_recognition: true,
        }
    }
}

impl LivenessPolicy {
    pub fn decide(&self, result: &LivenessResult) -> LivenessPolicyDecision {
        match result.liveness_decision {
            LivenessDecision::SpoofRejected if self.reject_on_spoof_evidence => {
                LivenessPolicyDecision::RejectAsSpoof
            }
            LivenessDecision::ProviderUnavailable
                if self.provider_unavailable_allows_recognition =>
            {
                LivenessPolicyDecision::ContinueBecauseProviderUnavailable
            }
            _ => LivenessPolicyDecision::ContinueRecognition,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LivenessWindowPolicy {
    pub reject_on_any_spoof_evidence: bool,
}

impl Default for LivenessWindowPolicy {
    fn default() -> Self {
        Self {
            reject_on_any_spoof_evidence: true,
        }
    }
}

impl LivenessWindowPolicy {
    pub fn decide(&self, frame_results: &[LivenessResult]) -> LivenessPolicyDecision {
        if self.reject_on_any_spoof_evidence
            && frame_results
                .iter()
                .any(|result| result.liveness_decision == LivenessDecision::SpoofRejected)
        {
            return LivenessPolicyDecision::RejectAsSpoof;
        }

        LivenessPolicyDecision::ContinueRecognition
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoof_evidence_rejects_before_recognition() {
        let result = LivenessResult {
            liveness_decision: LivenessDecision::SpoofRejected,
            liveness_score: Some(0.9),
            evidence: Vec::new(),
        };

        assert_eq!(
            LivenessPolicy::default().decide(&result),
            LivenessPolicyDecision::RejectAsSpoof
        );
    }

    #[test]
    fn inconclusive_screen_detection_continues_recognition() {
        let result = LivenessResult {
            liveness_decision: LivenessDecision::Inconclusive,
            liveness_score: None,
            evidence: Vec::new(),
        };

        assert_eq!(
            LivenessPolicy::default().decide(&result),
            LivenessPolicyDecision::ContinueRecognition
        );
    }

    #[test]
    fn window_rejects_when_any_frame_has_spoof_evidence() {
        let frame_results = vec![
            LivenessResult {
                liveness_decision: LivenessDecision::Inconclusive,
                liveness_score: None,
                evidence: Vec::new(),
            },
            LivenessResult {
                liveness_decision: LivenessDecision::SpoofRejected,
                liveness_score: Some(0.9),
                evidence: Vec::new(),
            },
            LivenessResult {
                liveness_decision: LivenessDecision::Inconclusive,
                liveness_score: None,
                evidence: Vec::new(),
            },
        ];

        assert_eq!(
            LivenessWindowPolicy::default().decide(&frame_results),
            LivenessPolicyDecision::RejectAsSpoof
        );
    }

    #[test]
    fn window_continues_when_no_frame_has_spoof_evidence() {
        let frame_results = vec![
            LivenessResult {
                liveness_decision: LivenessDecision::Inconclusive,
                liveness_score: None,
                evidence: Vec::new(),
            },
            LivenessResult {
                liveness_decision: LivenessDecision::ProviderUnavailable,
                liveness_score: None,
                evidence: Vec::new(),
            },
        ];

        assert_eq!(
            LivenessWindowPolicy::default().decide(&frame_results),
            LivenessPolicyDecision::ContinueRecognition
        );
    }
}
