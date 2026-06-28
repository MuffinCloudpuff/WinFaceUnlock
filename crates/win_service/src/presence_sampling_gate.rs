use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crate::{
    presence_monitor::{PresenceMonitorError, PresenceObservationSource},
    presence_policy::PresenceObservation,
    service_log::write_service_event_detail,
};

const DEFAULT_HUMAN_INPUT_QUIET_THRESHOLD_MS: u64 = 60_000;
const DEFAULT_GATE_RECHECK_INTERVAL_MS: u64 = 60_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HumanInputActivitySnapshot {
    pub human_input_quiet_duration_ms: u64,
    pub input_session_id: Option<u32>,
}

pub trait HumanInputActivitySource {
    fn human_input_activity_snapshot(&self) -> HumanInputActivityRead;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HumanInputActivityRead {
    Available(HumanInputActivitySnapshot),
    Unavailable {
        reason: HumanInputStateUnavailableReason,
        detail: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HumanInputStateUnavailableReason {
    SnapshotReadFailed,
    SnapshotSchemaUnsupported,
    SnapshotSourceUnexpected,
    SnapshotSessionMismatch,
    SnapshotStale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresenceSamplingGateConfig {
    pub human_input_quiet_threshold_ms: u64,
    pub gate_recheck_interval_ms: u64,
}

impl Default for PresenceSamplingGateConfig {
    fn default() -> Self {
        Self {
            human_input_quiet_threshold_ms: DEFAULT_HUMAN_INPUT_QUIET_THRESHOLD_MS,
            gate_recheck_interval_ms: DEFAULT_GATE_RECHECK_INTERVAL_MS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PresenceSamplingGateDecision {
    SkipSamplingBecauseRecentHumanInput {
        human_input_quiet_duration_ms: u64,
        human_input_quiet_threshold_ms: u64,
    },
    AllowSamplingBecauseHumanInputQuiet {
        human_input_quiet_duration_ms: u64,
        human_input_quiet_threshold_ms: u64,
    },
    SkipSamplingBecauseHumanInputStateUnavailable {
        reason: HumanInputStateUnavailableReason,
        detail: String,
    },
}

pub struct PresenceSamplingGate<S> {
    config: PresenceSamplingGateConfig,
    human_input_source: S,
}

impl<S> PresenceSamplingGate<S>
where
    S: HumanInputActivitySource,
{
    pub fn new(config: PresenceSamplingGateConfig, human_input_source: S) -> Self {
        Self {
            config,
            human_input_source,
        }
    }

    pub fn evaluate(&self) -> PresenceSamplingGateDecision {
        let snapshot = match self.human_input_source.human_input_activity_snapshot() {
            HumanInputActivityRead::Available(snapshot) => snapshot,
            HumanInputActivityRead::Unavailable { reason, detail } => {
                return PresenceSamplingGateDecision::SkipSamplingBecauseHumanInputStateUnavailable {
                    reason,
                    detail,
                };
            }
        };
        if snapshot.human_input_quiet_duration_ms < self.config.human_input_quiet_threshold_ms {
            PresenceSamplingGateDecision::SkipSamplingBecauseRecentHumanInput {
                human_input_quiet_duration_ms: snapshot.human_input_quiet_duration_ms,
                human_input_quiet_threshold_ms: self.config.human_input_quiet_threshold_ms,
            }
        } else {
            PresenceSamplingGateDecision::AllowSamplingBecauseHumanInputQuiet {
                human_input_quiet_duration_ms: snapshot.human_input_quiet_duration_ms,
                human_input_quiet_threshold_ms: self.config.human_input_quiet_threshold_ms,
            }
        }
    }

    pub fn gate_recheck_interval_ms(&self) -> u64 {
        self.config.gate_recheck_interval_ms.max(1)
    }
}

pub struct HumanInputGatedObservationSource<O, H> {
    observation_source: O,
    sampling_gate: PresenceSamplingGate<H>,
    stop_requested: Arc<AtomicBool>,
}

impl<O, H> HumanInputGatedObservationSource<O, H>
where
    O: PresenceObservationSource,
    H: HumanInputActivitySource,
{
    pub fn new(
        observation_source: O,
        sampling_gate: PresenceSamplingGate<H>,
        stop_requested: Arc<AtomicBool>,
    ) -> Self {
        Self {
            observation_source,
            sampling_gate,
            stop_requested,
        }
    }

    fn wait_until_next_gate_check_or_stop(&self) -> bool {
        let mut remaining_ms = self.sampling_gate.gate_recheck_interval_ms();
        while remaining_ms > 0 {
            if self.stop_requested.load(Ordering::SeqCst) {
                return false;
            }
            let chunk_ms = remaining_ms.min(500);
            thread::sleep(Duration::from_millis(chunk_ms));
            remaining_ms -= chunk_ms;
        }
        !self.stop_requested.load(Ordering::SeqCst)
    }
}

impl<O, H> PresenceObservationSource for HumanInputGatedObservationSource<O, H>
where
    O: PresenceObservationSource,
    H: HumanInputActivitySource,
{
    fn next_observation(&mut self) -> Result<Option<PresenceObservation>, PresenceMonitorError> {
        loop {
            if self.stop_requested.load(Ordering::SeqCst) {
                return Ok(None);
            }

            match self.sampling_gate.evaluate() {
                PresenceSamplingGateDecision::SkipSamplingBecauseRecentHumanInput {
                    human_input_quiet_duration_ms,
                    human_input_quiet_threshold_ms,
                } => {
                    write_service_event_detail(
                        "PresenceSamplingGate.SkipSampling",
                        format!(
                            "reason=recent-human-input human_input_quiet_duration_ms={human_input_quiet_duration_ms} human_input_quiet_threshold_ms={human_input_quiet_threshold_ms}"
                        ),
                    );
                    if !self.wait_until_next_gate_check_or_stop() {
                        return Ok(None);
                    }
                }
                PresenceSamplingGateDecision::AllowSamplingBecauseHumanInputQuiet {
                    human_input_quiet_duration_ms,
                    human_input_quiet_threshold_ms,
                } => {
                    write_service_event_detail(
                        "PresenceSamplingGate.AllowSampling",
                        format!(
                            "reason=human-input-quiet human_input_quiet_duration_ms={human_input_quiet_duration_ms} human_input_quiet_threshold_ms={human_input_quiet_threshold_ms}"
                        ),
                    );
                    return self.observation_source.next_observation();
                }
                PresenceSamplingGateDecision::SkipSamplingBecauseHumanInputStateUnavailable {
                    reason,
                    detail,
                } => {
                    write_service_event_detail(
                        "PresenceSamplingGate.SkipSampling",
                        format!(
                            "reason=human-input-state-unavailable unavailable_reason={reason:?} {detail}"
                        ),
                    );
                    if !self.wait_until_next_gate_check_or_stop() {
                        return Ok(None);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct FixedHumanInputSource {
        snapshot: HumanInputActivityRead,
    }

    impl HumanInputActivitySource for FixedHumanInputSource {
        fn human_input_activity_snapshot(&self) -> HumanInputActivityRead {
            self.snapshot.clone()
        }
    }

    #[test]
    fn gate_skips_sampling_when_human_input_is_recent() {
        let gate = PresenceSamplingGate::new(
            PresenceSamplingGateConfig {
                human_input_quiet_threshold_ms: 60_000,
                gate_recheck_interval_ms: 5_000,
            },
            FixedHumanInputSource {
                snapshot: HumanInputActivityRead::Available(HumanInputActivitySnapshot {
                    human_input_quiet_duration_ms: 12_000,
                    input_session_id: Some(1),
                }),
            },
        );

        assert_eq!(
            gate.evaluate(),
            PresenceSamplingGateDecision::SkipSamplingBecauseRecentHumanInput {
                human_input_quiet_duration_ms: 12_000,
                human_input_quiet_threshold_ms: 60_000
            }
        );
    }

    #[test]
    fn gate_allows_sampling_when_human_input_has_been_quiet() {
        let gate = PresenceSamplingGate::new(
            PresenceSamplingGateConfig {
                human_input_quiet_threshold_ms: 60_000,
                gate_recheck_interval_ms: 5_000,
            },
            FixedHumanInputSource {
                snapshot: HumanInputActivityRead::Available(HumanInputActivitySnapshot {
                    human_input_quiet_duration_ms: 60_000,
                    input_session_id: Some(1),
                }),
            },
        );

        assert_eq!(
            gate.evaluate(),
            PresenceSamplingGateDecision::AllowSamplingBecauseHumanInputQuiet {
                human_input_quiet_duration_ms: 60_000,
                human_input_quiet_threshold_ms: 60_000
            }
        );
    }

    #[test]
    fn gate_skips_sampling_when_human_input_state_is_unavailable() {
        let gate = PresenceSamplingGate::new(
            PresenceSamplingGateConfig::default(),
            FixedHumanInputSource {
                snapshot: HumanInputActivityRead::Unavailable {
                    reason: HumanInputStateUnavailableReason::SnapshotStale,
                    detail: "snapshot_age_ms=20000".to_owned(),
                },
            },
        );

        assert_eq!(
            gate.evaluate(),
            PresenceSamplingGateDecision::SkipSamplingBecauseHumanInputStateUnavailable {
                reason: HumanInputStateUnavailableReason::SnapshotStale,
                detail: "snapshot_age_ms=20000".to_owned()
            }
        );
    }
}
