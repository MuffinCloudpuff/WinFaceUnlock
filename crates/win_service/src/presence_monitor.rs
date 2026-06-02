use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crate::{
    presence_policy::{
        PresenceLockReason, PresenceObservation, PresencePolicy, PresencePolicyConfig,
        PresencePolicyDecision,
    },
    session_lock::{SessionLockError, SessionLocker},
};

#[derive(Clone, Debug)]
pub struct PresenceMonitorConfig {
    pub presence_lock_enabled: bool,
    pub max_monitor_iteration_count: Option<u32>,
    pub sleep_between_checks: bool,
    pub stop_requested: Option<Arc<AtomicBool>>,
}

impl Default for PresenceMonitorConfig {
    fn default() -> Self {
        Self {
            presence_lock_enabled: false,
            max_monitor_iteration_count: None,
            sleep_between_checks: true,
            stop_requested: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PresenceMonitorStopReason {
    Disabled,
    ObservationSourceEnded,
    MaxIterationCountReached,
    StopRequested,
    LockRequested {
        reason: PresenceLockReason,
        iteration_count: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresenceMonitorSummary {
    pub iteration_count: u32,
    pub unknown_face_audit_request_count: u32,
    pub lock_request_count: u32,
    pub stop_reason: PresenceMonitorStopReason,
}

#[derive(Debug, Eq, PartialEq)]
pub enum PresenceMonitorError {
    ObservationFailed,
    UnknownFaceAuditFailed,
    SessionLockFailed(SessionLockError),
}

pub trait PresenceObservationSource {
    fn next_observation(&mut self) -> Result<Option<PresenceObservation>, PresenceMonitorError>;
}

pub trait UnknownFaceAuditSink {
    fn capture_unknown_face_audit(
        &mut self,
        decision: &PresencePolicyDecision,
    ) -> Result<(), PresenceMonitorError>;
}

pub struct NoopUnknownFaceAuditSink;

impl UnknownFaceAuditSink for NoopUnknownFaceAuditSink {
    fn capture_unknown_face_audit(
        &mut self,
        _decision: &PresencePolicyDecision,
    ) -> Result<(), PresenceMonitorError> {
        Ok(())
    }
}

pub struct PresenceMonitor<L, A, S> {
    config: PresenceMonitorConfig,
    policy: PresencePolicy,
    locker: L,
    audit_sink: A,
    observation_source: S,
}

impl<L, A, S> PresenceMonitor<L, A, S>
where
    L: SessionLocker,
    A: UnknownFaceAuditSink,
    S: PresenceObservationSource,
{
    pub fn new(
        config: PresenceMonitorConfig,
        policy_config: PresencePolicyConfig,
        locker: L,
        audit_sink: A,
        observation_source: S,
    ) -> Self {
        Self {
            config,
            policy: PresencePolicy::new(policy_config),
            locker,
            audit_sink,
            observation_source,
        }
    }

    pub fn run(&mut self) -> Result<PresenceMonitorSummary, PresenceMonitorError> {
        if !self.config.presence_lock_enabled {
            return Ok(PresenceMonitorSummary {
                iteration_count: 0,
                unknown_face_audit_request_count: 0,
                lock_request_count: 0,
                stop_reason: PresenceMonitorStopReason::Disabled,
            });
        }

        let mut iteration_count = 0_u32;
        let mut unknown_face_audit_request_count = 0_u32;
        let mut lock_request_count = 0_u32;

        loop {
            if self.stop_is_requested() {
                return Ok(PresenceMonitorSummary {
                    iteration_count,
                    unknown_face_audit_request_count,
                    lock_request_count,
                    stop_reason: PresenceMonitorStopReason::StopRequested,
                });
            }

            if self
                .config
                .max_monitor_iteration_count
                .is_some_and(|max_iteration_count| iteration_count >= max_iteration_count)
            {
                return Ok(PresenceMonitorSummary {
                    iteration_count,
                    unknown_face_audit_request_count,
                    lock_request_count,
                    stop_reason: PresenceMonitorStopReason::MaxIterationCountReached,
                });
            }

            let Some(observation) = self.observation_source.next_observation()? else {
                return Ok(PresenceMonitorSummary {
                    iteration_count,
                    unknown_face_audit_request_count,
                    lock_request_count,
                    stop_reason: PresenceMonitorStopReason::ObservationSourceEnded,
                });
            };

            iteration_count = iteration_count.saturating_add(1);
            let decision = self.policy.record_observation(observation);
            if decision.unknown_face_audit_capture_requested {
                self.audit_sink.capture_unknown_face_audit(&decision)?;
                unknown_face_audit_request_count =
                    unknown_face_audit_request_count.saturating_add(1);
            }

            if decision.lock_requested {
                self.locker
                    .request_lock_workstation()
                    .map_err(PresenceMonitorError::SessionLockFailed)?;
                lock_request_count = lock_request_count.saturating_add(1);
                return Ok(PresenceMonitorSummary {
                    iteration_count,
                    unknown_face_audit_request_count,
                    lock_request_count,
                    stop_reason: PresenceMonitorStopReason::LockRequested {
                        reason: decision
                            .lock_reason
                            .unwrap_or(PresenceLockReason::ConsecutiveNoFace),
                        iteration_count,
                    },
                });
            }

            if self.config.sleep_between_checks {
                self.sleep_until_next_check_or_stop(decision.next_check_interval_ms);
            }
        }
    }

    fn stop_is_requested(&self) -> bool {
        self.config
            .stop_requested
            .as_ref()
            .is_some_and(|stop_requested| stop_requested.load(Ordering::SeqCst))
    }

    fn sleep_until_next_check_or_stop(&self, interval_ms: u64) {
        let mut remaining_ms = interval_ms;
        while remaining_ms > 0 && !self.stop_is_requested() {
            let chunk_ms = remaining_ms.min(500);
            thread::sleep(Duration::from_millis(chunk_ms));
            remaining_ms -= chunk_ms;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SequenceObservationSource {
        observations: Vec<PresenceObservation>,
    }

    impl SequenceObservationSource {
        fn new(observations: Vec<PresenceObservation>) -> Self {
            Self { observations }
        }
    }

    impl PresenceObservationSource for SequenceObservationSource {
        fn next_observation(
            &mut self,
        ) -> Result<Option<PresenceObservation>, PresenceMonitorError> {
            if self.observations.is_empty() {
                return Ok(None);
            }
            Ok(Some(self.observations.remove(0)))
        }
    }

    #[derive(Default)]
    struct RecordingAuditSink {
        request_count: u32,
    }

    impl UnknownFaceAuditSink for RecordingAuditSink {
        fn capture_unknown_face_audit(
            &mut self,
            _decision: &PresencePolicyDecision,
        ) -> Result<(), PresenceMonitorError> {
            self.request_count = self.request_count.saturating_add(1);
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingLocker {
        lock_request_count: std::cell::Cell<u32>,
    }

    impl SessionLocker for RecordingLocker {
        fn request_lock_workstation(&self) -> Result<(), SessionLockError> {
            self.lock_request_count
                .set(self.lock_request_count.get().saturating_add(1));
            Ok(())
        }
    }

    #[test]
    fn disabled_monitor_does_not_consume_observations() -> Result<(), PresenceMonitorError> {
        let source = SequenceObservationSource::new(vec![PresenceObservation::NoFaceDetected]);
        let mut monitor = PresenceMonitor::new(
            PresenceMonitorConfig {
                presence_lock_enabled: false,
                max_monitor_iteration_count: None,
                sleep_between_checks: false,
                stop_requested: None,
            },
            PresencePolicyConfig::default(),
            RecordingLocker::default(),
            RecordingAuditSink::default(),
            source,
        );

        let summary = monitor.run()?;

        assert_eq!(summary.iteration_count, 0);
        assert_eq!(summary.stop_reason, PresenceMonitorStopReason::Disabled);
        Ok(())
    }

    #[test]
    fn stop_requested_monitor_does_not_consume_observations() -> Result<(), PresenceMonitorError> {
        let stop_requested = Arc::new(AtomicBool::new(true));
        let source = SequenceObservationSource::new(vec![PresenceObservation::NoFaceDetected]);
        let mut monitor = PresenceMonitor::new(
            PresenceMonitorConfig {
                presence_lock_enabled: true,
                max_monitor_iteration_count: None,
                sleep_between_checks: false,
                stop_requested: Some(stop_requested),
            },
            PresencePolicyConfig::default(),
            RecordingLocker::default(),
            RecordingAuditSink::default(),
            source,
        );

        let summary = monitor.run()?;

        assert_eq!(summary.iteration_count, 0);
        assert_eq!(
            summary.stop_reason,
            PresenceMonitorStopReason::StopRequested
        );
        Ok(())
    }

    #[test]
    fn monitor_locks_after_three_no_face_observations() -> Result<(), PresenceMonitorError> {
        let source = SequenceObservationSource::new(vec![
            PresenceObservation::NoFaceDetected,
            PresenceObservation::NoFaceDetected,
            PresenceObservation::NoFaceDetected,
        ]);
        let mut monitor = PresenceMonitor::new(
            test_config(),
            PresencePolicyConfig::default(),
            RecordingLocker::default(),
            RecordingAuditSink::default(),
            source,
        );

        let summary = monitor.run()?;

        assert_eq!(summary.iteration_count, 3);
        assert_eq!(summary.lock_request_count, 1);
        assert_eq!(
            summary.stop_reason,
            PresenceMonitorStopReason::LockRequested {
                reason: PresenceLockReason::ConsecutiveNoFace,
                iteration_count: 3
            }
        );
        Ok(())
    }

    #[test]
    fn monitor_requests_unknown_face_audit_once_then_locks() -> Result<(), PresenceMonitorError> {
        let source = SequenceObservationSource::new(vec![
            PresenceObservation::UnknownFace {
                owner_match_score: 0.2,
            },
            PresenceObservation::UnknownFace {
                owner_match_score: 0.2,
            },
            PresenceObservation::UnknownFace {
                owner_match_score: 0.2,
            },
        ]);
        let mut monitor = PresenceMonitor::new(
            test_config(),
            PresencePolicyConfig::default(),
            RecordingLocker::default(),
            RecordingAuditSink::default(),
            source,
        );

        let summary = monitor.run()?;

        assert_eq!(summary.unknown_face_audit_request_count, 1);
        assert_eq!(summary.lock_request_count, 1);
        assert_eq!(
            summary.stop_reason,
            PresenceMonitorStopReason::LockRequested {
                reason: PresenceLockReason::ConsecutiveUnknownFace,
                iteration_count: 3
            }
        );
        Ok(())
    }

    fn test_config() -> PresenceMonitorConfig {
        PresenceMonitorConfig {
            presence_lock_enabled: true,
            max_monitor_iteration_count: Some(10),
            sleep_between_checks: false,
            stop_requested: None,
        }
    }
}
