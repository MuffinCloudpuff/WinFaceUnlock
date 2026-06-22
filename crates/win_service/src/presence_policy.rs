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
    pub presence_person_stable_interval_ms: u64,
    pub presence_person_suspect_interval_ms: u64,
    pub presence_person_confirmed_present_required_frames: u32,
    pub presence_person_absent_required_frames: u32,
    pub presence_person_boundary_margin_ratio: f32,
    pub presence_person_movement_delta_ratio: f32,
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
            presence_person_stable_interval_ms: 500,
            presence_person_suspect_interval_ms: 200,
            presence_person_confirmed_present_required_frames: 2,
            presence_person_absent_required_frames: 6,
            presence_person_boundary_margin_ratio: 0.12,
            presence_person_movement_delta_ratio: 0.04,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresenceMonitorState {
    StableOwnerPresent,
    NoFaceSuspect,
    UnknownFaceSuspect,
    PersonPresent,
    PersonAbsenceSuspect,
    LockRequested,
    CameraUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PresenceObservation {
    OwnerPresent {
        owner_match_score: f32,
    },
    NoFaceDetected,
    UnknownFace {
        owner_match_score: f32,
    },
    PersonPresent {
        confidence: f32,
        bbox_center_x_ratio: f32,
        bbox_area_ratio: f32,
    },
    PersonAbsent,
    CameraUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresenceLockReason {
    ConsecutiveNoFace,
    ConsecutiveUnknownFace,
    PersonLeftFrame,
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
    person_present_consecutive_count: u32,
    person_confirmed_present_observed: bool,
    person_absent_consecutive_count: u32,
    person_departure_evidence_count: u32,
    last_person_center_x_ratio: Option<f32>,
    last_person_area_ratio: Option<f32>,
}

impl PresencePolicy {
    pub fn new(config: PresencePolicyConfig) -> Self {
        Self {
            config,
            stable_owner_observation_count: 0,
            no_face_consecutive_count: 0,
            unknown_face_consecutive_count: 0,
            unknown_face_audit_already_requested: false,
            person_present_consecutive_count: 0,
            person_confirmed_present_observed: false,
            person_absent_consecutive_count: 0,
            person_departure_evidence_count: 0,
            last_person_center_x_ratio: None,
            last_person_area_ratio: None,
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
            PresenceObservation::PersonPresent {
                confidence,
                bbox_center_x_ratio,
                bbox_area_ratio,
            } => self.record_person_present(confidence, bbox_center_x_ratio, bbox_area_ratio),
            PresenceObservation::PersonAbsent => self.record_person_absent(),
            PresenceObservation::CameraUnavailable => self.record_camera_unavailable(),
        }
    }

    fn record_owner_present(&mut self, owner_match_score: f32) -> PresencePolicyDecision {
        self.stable_owner_observation_count = self.stable_owner_observation_count.saturating_add(1);
        self.no_face_consecutive_count = 0;
        self.unknown_face_consecutive_count = 0;
        self.unknown_face_audit_already_requested = false;
        self.reset_person_departure_state();

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
        self.reset_person_departure_state();

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
        self.reset_person_departure_state();
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

    fn record_person_present(
        &mut self,
        _confidence: f32,
        bbox_center_x_ratio: f32,
        bbox_area_ratio: f32,
    ) -> PresencePolicyDecision {
        self.stable_owner_observation_count = 0;
        self.no_face_consecutive_count = 0;
        self.unknown_face_consecutive_count = 0;
        self.unknown_face_audit_already_requested = false;
        self.person_present_consecutive_count =
            self.person_present_consecutive_count.saturating_add(1);
        if self.person_present_consecutive_count
            >= self
                .config
                .presence_person_confirmed_present_required_frames
                .max(1)
        {
            self.person_confirmed_present_observed = true;
        }
        self.person_absent_consecutive_count = 0;

        let departure_evidence_detected =
            self.person_observation_suggests_departure(bbox_center_x_ratio, bbox_area_ratio);
        if departure_evidence_detected {
            self.person_departure_evidence_count =
                self.person_departure_evidence_count.saturating_add(1);
        } else {
            self.person_departure_evidence_count = 0;
        }

        self.last_person_center_x_ratio = Some(bbox_center_x_ratio.clamp(0.0, 1.0));
        self.last_person_area_ratio = Some(bbox_area_ratio.clamp(0.0, 1.0));

        PresencePolicyDecision {
            monitor_state: PresenceMonitorState::PersonPresent,
            next_check_interval_ms: if departure_evidence_detected {
                self.config.presence_person_suspect_interval_ms
            } else {
                self.config.presence_person_stable_interval_ms
            },
            owner_match_score: None,
            no_face_consecutive_count: self.no_face_consecutive_count,
            unknown_face_consecutive_count: self.unknown_face_consecutive_count,
            unknown_face_audit_capture_requested: false,
            lock_requested: false,
            lock_reason: None,
        }
    }

    fn record_person_absent(&mut self) -> PresencePolicyDecision {
        self.stable_owner_observation_count = 0;
        self.no_face_consecutive_count = 0;
        self.unknown_face_consecutive_count = 0;
        self.unknown_face_audit_already_requested = false;
        self.person_present_consecutive_count = 0;
        self.person_absent_consecutive_count =
            self.person_absent_consecutive_count.saturating_add(1);

        let lock_requested = self.person_absent_consecutive_count
            >= self.config.presence_person_absent_required_frames;

        PresencePolicyDecision {
            monitor_state: if lock_requested {
                PresenceMonitorState::LockRequested
            } else {
                PresenceMonitorState::PersonAbsenceSuspect
            },
            next_check_interval_ms: self.config.presence_person_suspect_interval_ms,
            owner_match_score: None,
            no_face_consecutive_count: self.no_face_consecutive_count,
            unknown_face_consecutive_count: self.unknown_face_consecutive_count,
            unknown_face_audit_capture_requested: false,
            lock_requested,
            lock_reason: lock_requested.then_some(PresenceLockReason::PersonLeftFrame),
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

    fn person_observation_suggests_departure(
        &self,
        bbox_center_x_ratio: f32,
        bbox_area_ratio: f32,
    ) -> bool {
        let center = bbox_center_x_ratio.clamp(0.0, 1.0);
        let area = bbox_area_ratio.clamp(0.0, 1.0);
        let near_boundary = center <= self.config.presence_person_boundary_margin_ratio
            || center >= 1.0 - self.config.presence_person_boundary_margin_ratio;
        let Some(previous_center) = self.last_person_center_x_ratio else {
            return near_boundary;
        };
        let center_delta = center - previous_center;
        let moving_left_to_boundary = center < previous_center
            && center_delta.abs() >= self.config.presence_person_movement_delta_ratio;
        let moving_right_to_boundary = center > previous_center
            && center_delta.abs() >= self.config.presence_person_movement_delta_ratio;
        let boundary_movement = (center <= 0.5 && moving_left_to_boundary)
            || (center > 0.5 && moving_right_to_boundary);
        let area_shrinking = self.last_person_area_ratio.is_some_and(|previous_area| {
            previous_area - area >= self.config.presence_person_movement_delta_ratio
        });

        near_boundary || boundary_movement || area_shrinking
    }

    fn reset_person_departure_state(&mut self) {
        self.person_present_consecutive_count = 0;
        self.person_confirmed_present_observed = false;
        self.person_absent_consecutive_count = 0;
        self.person_departure_evidence_count = 0;
        self.last_person_center_x_ratio = None;
        self.last_person_area_ratio = None;
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

    #[test]
    fn person_absence_after_boundary_departure_requests_lock() {
        let mut policy = PresencePolicy::new(PresencePolicyConfig {
            presence_person_absent_required_frames: 2,
            presence_person_boundary_margin_ratio: 0.20,
            presence_person_movement_delta_ratio: 0.05,
            presence_no_face_suspect_interval_ms: 500,
            ..PresencePolicyConfig::default()
        });

        let _ = policy.record_observation(PresenceObservation::PersonPresent {
            confidence: 0.80,
            bbox_center_x_ratio: 0.35,
            bbox_area_ratio: 0.40,
        });
        let present = policy.record_observation(PresenceObservation::PersonPresent {
            confidence: 0.82,
            bbox_center_x_ratio: 0.15,
            bbox_area_ratio: 0.30,
        });
        let first_absent = policy.record_observation(PresenceObservation::PersonAbsent);
        let second_absent = policy.record_observation(PresenceObservation::PersonAbsent);

        assert_eq!(present.monitor_state, PresenceMonitorState::PersonPresent);
        assert_eq!(present.next_check_interval_ms, 200);
        assert!(!first_absent.lock_requested);
        assert!(second_absent.lock_requested);
        assert_eq!(
            second_absent.lock_reason,
            Some(PresenceLockReason::PersonLeftFrame)
        );
    }

    #[test]
    fn person_absence_after_area_shrink_departure_requests_lock() {
        let mut policy = PresencePolicy::new(PresencePolicyConfig {
            presence_person_absent_required_frames: 2,
            presence_person_boundary_margin_ratio: 0.12,
            presence_person_movement_delta_ratio: 0.04,
            ..PresencePolicyConfig::default()
        });

        let _ = policy.record_observation(PresenceObservation::PersonPresent {
            confidence: 0.80,
            bbox_center_x_ratio: 0.62,
            bbox_area_ratio: 0.59,
        });
        let _ = policy.record_observation(PresenceObservation::PersonPresent {
            confidence: 0.82,
            bbox_center_x_ratio: 0.64,
            bbox_area_ratio: 0.44,
        });
        let _ = policy.record_observation(PresenceObservation::PersonAbsent);
        let decision = policy.record_observation(PresenceObservation::PersonAbsent);

        assert!(decision.lock_requested);
        assert_eq!(
            decision.lock_reason,
            Some(PresenceLockReason::PersonLeftFrame)
        );
    }

    #[test]
    fn consecutive_person_absence_requests_lock_without_prior_presence() {
        let mut policy = PresencePolicy::new(PresencePolicyConfig {
            presence_person_absent_required_frames: 2,
            ..PresencePolicyConfig::default()
        });

        let first_absent = policy.record_observation(PresenceObservation::PersonAbsent);
        let decision = policy.record_observation(PresenceObservation::PersonAbsent);

        assert_eq!(first_absent.next_check_interval_ms, 200);
        assert_eq!(decision.monitor_state, PresenceMonitorState::LockRequested);
        assert!(decision.lock_requested);
        assert_eq!(
            decision.lock_reason,
            Some(PresenceLockReason::PersonLeftFrame)
        );
    }

    #[test]
    fn confirmed_person_absence_without_departure_evidence_requests_lock() {
        let mut policy = PresencePolicy::new(PresencePolicyConfig {
            presence_person_confirmed_present_required_frames: 2,
            presence_person_absent_required_frames: 2,
            ..PresencePolicyConfig::default()
        });

        let _ = policy.record_observation(PresenceObservation::PersonPresent {
            confidence: 0.80,
            bbox_center_x_ratio: 0.50,
            bbox_area_ratio: 0.40,
        });
        let _ = policy.record_observation(PresenceObservation::PersonPresent {
            confidence: 0.82,
            bbox_center_x_ratio: 0.51,
            bbox_area_ratio: 0.40,
        });
        let first_absent = policy.record_observation(PresenceObservation::PersonAbsent);
        let second_absent = policy.record_observation(PresenceObservation::PersonAbsent);

        assert!(!first_absent.lock_requested);
        assert!(second_absent.lock_requested);
        assert_eq!(
            second_absent.lock_reason,
            Some(PresenceLockReason::PersonLeftFrame)
        );
    }

    #[test]
    fn stable_person_present_uses_low_frequency_interval() {
        let mut policy = PresencePolicy::new(PresencePolicyConfig {
            presence_person_stable_interval_ms: 500,
            presence_person_suspect_interval_ms: 200,
            ..PresencePolicyConfig::default()
        });

        let decision = policy.record_observation(PresenceObservation::PersonPresent {
            confidence: 0.80,
            bbox_center_x_ratio: 0.50,
            bbox_area_ratio: 0.40,
        });

        assert_eq!(decision.monitor_state, PresenceMonitorState::PersonPresent);
        assert_eq!(decision.next_check_interval_ms, 500);
    }
}
