#[derive(Clone, Debug, PartialEq)]
pub struct PresencePolicyConfig {
    pub presence_stable_initial_interval_ms: u64,
    pub presence_stable_second_interval_ms: u64,
    pub presence_stable_max_interval_ms: u64,
    pub presence_no_face_suspect_interval_ms: u64,
    pub presence_unknown_face_suspect_interval_ms: u64,
    pub presence_no_face_required_count: u32,
    pub presence_unknown_face_required_count: u32,
    pub presence_owner_match_threshold: f32,
}

impl Default for PresencePolicyConfig {
    fn default() -> Self {
        Self {
            presence_stable_initial_interval_ms: 10_000,
            presence_stable_second_interval_ms: 30_000,
            presence_stable_max_interval_ms: 60_000,
            presence_no_face_suspect_interval_ms: 10_000,
            presence_unknown_face_suspect_interval_ms: 1_000,
            presence_no_face_required_count: 3,
            presence_unknown_face_required_count: 3,
            presence_owner_match_threshold: 0.50,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresenceMonitorState {
    StableOwnerPresent,
    NoFaceSuspect,
    UnknownFaceSuspect,
    LockRequested,
    CameraUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PresenceObservation {
    OwnerPresent { owner_match_score: f32 },
    NoFaceDetected,
    UnknownFace { owner_match_score: f32 },
    CameraUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresenceLockReason {
    ConsecutiveNoFace,
    ConsecutiveUnknownFace,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresencePolicyDecision {
    pub monitor_state: PresenceMonitorState,
    pub next_check_interval_ms: u64,
    pub owner_match_score: Option<f32>,
    pub no_face_consecutive_count: u32,
    pub unknown_face_consecutive_count: u32,
    pub unknown_face_audit_capture_requested: bool,
    pub lock_requested: bool,
    pub lock_reason: Option<PresenceLockReason>,
}

pub struct PresencePolicy {
    config: PresencePolicyConfig,
    stable_owner_observation_count: u32,
    no_face_consecutive_count: u32,
    unknown_face_consecutive_count: u32,
    unknown_face_audit_already_requested: bool,
}

impl PresencePolicy {
    pub fn new(config: PresencePolicyConfig) -> Self {
        Self {
            config,
            stable_owner_observation_count: 0,
            no_face_consecutive_count: 0,
            unknown_face_consecutive_count: 0,
            unknown_face_audit_already_requested: false,
        }
    }

    pub fn record_observation(
        &mut self,
        observation: PresenceObservation,
    ) -> PresencePolicyDecision {
        match observation {
            PresenceObservation::OwnerPresent { owner_match_score } => {
                self.record_owner_present(owner_match_score)
            }
            PresenceObservation::NoFaceDetected => self.record_no_face(),
            PresenceObservation::UnknownFace { owner_match_score } => {
                self.record_unknown_face(owner_match_score)
            }
            PresenceObservation::CameraUnavailable => self.record_camera_unavailable(),
        }
    }

    fn record_owner_present(&mut self, owner_match_score: f32) -> PresencePolicyDecision {
        self.stable_owner_observation_count = self.stable_owner_observation_count.saturating_add(1);
        self.no_face_consecutive_count = 0;
        self.unknown_face_consecutive_count = 0;
        self.unknown_face_audit_already_requested = false;

        PresencePolicyDecision {
            monitor_state: PresenceMonitorState::StableOwnerPresent,
            next_check_interval_ms: self.stable_owner_next_interval_ms(),
            owner_match_score: Some(owner_match_score),
            no_face_consecutive_count: self.no_face_consecutive_count,
            unknown_face_consecutive_count: self.unknown_face_consecutive_count,
            unknown_face_audit_capture_requested: false,
            lock_requested: false,
            lock_reason: None,
        }
    }

    fn record_no_face(&mut self) -> PresencePolicyDecision {
        self.stable_owner_observation_count = 0;
        self.no_face_consecutive_count = self.no_face_consecutive_count.saturating_add(1);
        self.unknown_face_consecutive_count = 0;
        self.unknown_face_audit_already_requested = false;

        let lock_requested =
            self.no_face_consecutive_count >= self.config.presence_no_face_required_count;

        PresencePolicyDecision {
            monitor_state: if lock_requested {
                PresenceMonitorState::LockRequested
            } else {
                PresenceMonitorState::NoFaceSuspect
            },
            next_check_interval_ms: self.config.presence_no_face_suspect_interval_ms,
            owner_match_score: None,
            no_face_consecutive_count: self.no_face_consecutive_count,
            unknown_face_consecutive_count: self.unknown_face_consecutive_count,
            unknown_face_audit_capture_requested: false,
            lock_requested,
            lock_reason: lock_requested.then_some(PresenceLockReason::ConsecutiveNoFace),
        }
    }

    fn record_unknown_face(&mut self, owner_match_score: f32) -> PresencePolicyDecision {
        self.stable_owner_observation_count = 0;
        self.no_face_consecutive_count = 0;
        self.unknown_face_consecutive_count = self.unknown_face_consecutive_count.saturating_add(1);
        let audit_capture_requested = !self.unknown_face_audit_already_requested;
        self.unknown_face_audit_already_requested = true;

        let lock_requested =
            self.unknown_face_consecutive_count >= self.config.presence_unknown_face_required_count;

        PresencePolicyDecision {
            monitor_state: if lock_requested {
                PresenceMonitorState::LockRequested
            } else {
                PresenceMonitorState::UnknownFaceSuspect
            },
            next_check_interval_ms: self.config.presence_unknown_face_suspect_interval_ms,
            owner_match_score: Some(owner_match_score),
            no_face_consecutive_count: self.no_face_consecutive_count,
            unknown_face_consecutive_count: self.unknown_face_consecutive_count,
            unknown_face_audit_capture_requested: audit_capture_requested,
            lock_requested,
            lock_reason: lock_requested.then_some(PresenceLockReason::ConsecutiveUnknownFace),
        }
    }

    fn record_camera_unavailable(&mut self) -> PresencePolicyDecision {
        PresencePolicyDecision {
            monitor_state: PresenceMonitorState::CameraUnavailable,
            next_check_interval_ms: self.config.presence_stable_initial_interval_ms,
            owner_match_score: None,
            no_face_consecutive_count: self.no_face_consecutive_count,
            unknown_face_consecutive_count: self.unknown_face_consecutive_count,
            unknown_face_audit_capture_requested: false,
            lock_requested: false,
            lock_reason: None,
        }
    }

    fn stable_owner_next_interval_ms(&self) -> u64 {
        match self.stable_owner_observation_count {
            0 | 1 => self.config.presence_stable_initial_interval_ms,
            2 => self.config.presence_stable_second_interval_ms,
            _ => self.config.presence_stable_max_interval_ms,
        }
    }

    pub fn owner_match_passes_presence_threshold(&self, owner_match_score: f32) -> bool {
        owner_match_score >= self.config.presence_owner_match_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_present_observations_ramp_to_max_interval() {
        let mut policy = PresencePolicy::new(PresencePolicyConfig::default());

        let first = policy.record_observation(PresenceObservation::OwnerPresent {
            owner_match_score: 0.61,
        });
        let second = policy.record_observation(PresenceObservation::OwnerPresent {
            owner_match_score: 0.62,
        });
        let third = policy.record_observation(PresenceObservation::OwnerPresent {
            owner_match_score: 0.63,
        });

        assert_eq!(first.next_check_interval_ms, 10_000);
        assert_eq!(second.next_check_interval_ms, 30_000);
        assert_eq!(third.next_check_interval_ms, 60_000);
        assert_eq!(
            third.monitor_state,
            PresenceMonitorState::StableOwnerPresent
        );
    }

    #[test]
    fn consecutive_no_face_requests_lock_without_unknown_face_audit() {
        let mut policy = PresencePolicy::new(PresencePolicyConfig::default());

        let first = policy.record_observation(PresenceObservation::NoFaceDetected);
        let second = policy.record_observation(PresenceObservation::NoFaceDetected);
        let third = policy.record_observation(PresenceObservation::NoFaceDetected);

        assert!(!first.lock_requested);
        assert!(!second.lock_requested);
        assert!(third.lock_requested);
        assert_eq!(
            third.lock_reason,
            Some(PresenceLockReason::ConsecutiveNoFace)
        );
        assert!(!third.unknown_face_audit_capture_requested);
    }

    #[test]
    fn unknown_face_requests_audit_once_then_locks_on_third_low_match() {
        let mut policy = PresencePolicy::new(PresencePolicyConfig::default());

        let first = policy.record_observation(PresenceObservation::UnknownFace {
            owner_match_score: 0.22,
        });
        let second = policy.record_observation(PresenceObservation::UnknownFace {
            owner_match_score: 0.23,
        });
        let third = policy.record_observation(PresenceObservation::UnknownFace {
            owner_match_score: 0.24,
        });

        assert!(first.unknown_face_audit_capture_requested);
        assert_eq!(first.next_check_interval_ms, 1_000);
        assert!(!second.unknown_face_audit_capture_requested);
        assert!(third.lock_requested);
        assert_eq!(
            third.lock_reason,
            Some(PresenceLockReason::ConsecutiveUnknownFace)
        );
    }

    #[test]
    fn owner_present_resets_suspect_counts_and_audit_gate() {
        let mut policy = PresencePolicy::new(PresencePolicyConfig::default());
        let _ = policy.record_observation(PresenceObservation::UnknownFace {
            owner_match_score: 0.22,
        });
        let owner = policy.record_observation(PresenceObservation::OwnerPresent {
            owner_match_score: 0.70,
        });
        let unknown_again = policy.record_observation(PresenceObservation::UnknownFace {
            owner_match_score: 0.21,
        });

        assert_eq!(owner.unknown_face_consecutive_count, 0);
        assert_eq!(owner.no_face_consecutive_count, 0);
        assert!(unknown_again.unknown_face_audit_capture_requested);
    }

    #[test]
    fn camera_unavailable_does_not_request_lock() {
        let mut policy = PresencePolicy::new(PresencePolicyConfig::default());

        let decision = policy.record_observation(PresenceObservation::CameraUnavailable);

        assert_eq!(
            decision.monitor_state,
            PresenceMonitorState::CameraUnavailable
        );
        assert!(!decision.lock_requested);
    }
}
