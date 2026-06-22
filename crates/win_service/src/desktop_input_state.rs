use std::{path::PathBuf, time::Duration};

use desktop_input::{
    DesktopInputSnapshot, SNAPSHOT_SCHEMA_VERSION, SNAPSHOT_SOURCE_GET_LAST_INPUT_INFO,
    read_snapshot, snapshot_path_from_install_dir, unix_time_ms_now,
};

use crate::presence_sampling_gate::{
    HumanInputActivityRead, HumanInputActivitySnapshot, HumanInputActivitySource,
    HumanInputStateUnavailableReason,
};

const DEFAULT_DESKTOP_INPUT_SNAPSHOT_MAX_AGE_MS: u64 = 15_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopInputSnapshotActivitySource {
    path: PathBuf,
    expected_session_id: u32,
    snapshot_max_age_ms: u64,
}

impl DesktopInputSnapshotActivitySource {
    pub fn new(expected_session_id: u32) -> Self {
        Self {
            path: desktop_input_snapshot_path(),
            expected_session_id,
            snapshot_max_age_ms: DEFAULT_DESKTOP_INPUT_SNAPSHOT_MAX_AGE_MS,
        }
    }

    pub fn with_path_for_tests(
        path: PathBuf,
        expected_session_id: u32,
        snapshot_max_age_ms: u64,
    ) -> Self {
        Self {
            path,
            expected_session_id,
            snapshot_max_age_ms,
        }
    }

    fn read_validated_snapshot(&self) -> HumanInputActivityRead {
        let snapshot = match read_snapshot(&self.path) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return HumanInputActivityRead::Unavailable {
                    reason: HumanInputStateUnavailableReason::SnapshotReadFailed,
                    detail: format!("path={} error={error:?}", self.path.display()),
                };
            }
        };
        validate_snapshot(
            &snapshot,
            self.expected_session_id,
            self.snapshot_max_age_ms,
            unix_time_ms_now(),
        )
    }
}

impl HumanInputActivitySource for DesktopInputSnapshotActivitySource {
    fn human_input_activity_snapshot(&self) -> HumanInputActivityRead {
        self.read_validated_snapshot()
    }
}

fn validate_snapshot(
    snapshot: &DesktopInputSnapshot,
    expected_session_id: u32,
    snapshot_max_age_ms: u64,
    now_unix_ms: u64,
) -> HumanInputActivityRead {
    if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return HumanInputActivityRead::Unavailable {
            reason: HumanInputStateUnavailableReason::SnapshotSchemaUnsupported,
            detail: format!(
                "schema_version={} expected_schema_version={SNAPSHOT_SCHEMA_VERSION}",
                snapshot.schema_version
            ),
        };
    }
    if snapshot.source != SNAPSHOT_SOURCE_GET_LAST_INPUT_INFO {
        return HumanInputActivityRead::Unavailable {
            reason: HumanInputStateUnavailableReason::SnapshotSourceUnexpected,
            detail: format!("source={}", snapshot.source),
        };
    }
    if snapshot.session_id != expected_session_id {
        return HumanInputActivityRead::Unavailable {
            reason: HumanInputStateUnavailableReason::SnapshotSessionMismatch,
            detail: format!(
                "snapshot_session_id={} expected_session_id={expected_session_id}",
                snapshot.session_id
            ),
        };
    }
    let snapshot_age_ms = now_unix_ms.saturating_sub(snapshot.sampled_at_unix_ms);
    if snapshot_age_ms > snapshot_max_age_ms {
        return HumanInputActivityRead::Unavailable {
            reason: HumanInputStateUnavailableReason::SnapshotStale,
            detail: format!(
                "snapshot_age_ms={snapshot_age_ms} snapshot_max_age_ms={snapshot_max_age_ms}"
            ),
        };
    }
    HumanInputActivityRead::Available(HumanInputActivitySnapshot {
        human_input_quiet_duration_ms: snapshot.human_input_quiet_duration_ms,
        input_session_id: Some(snapshot.session_id),
    })
}

pub fn desktop_input_agent_sample_interval() -> Duration {
    Duration::from_millis(5_000)
}

pub fn desktop_input_snapshot_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(snapshot_path_from_install_dir))
        .unwrap_or_else(desktop_input::default_snapshot_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(session_id: u32, sampled_at_unix_ms: u64) -> DesktopInputSnapshot {
        DesktopInputSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            source: SNAPSHOT_SOURCE_GET_LAST_INPUT_INFO.to_owned(),
            session_id,
            agent_process_id: 42,
            last_input_tick_ms: 10,
            human_input_quiet_duration_ms: 12_000,
            sampled_at_unix_ms,
        }
    }

    #[test]
    fn validates_fresh_matching_snapshot() {
        assert_eq!(
            validate_snapshot(&snapshot(1, 100_000), 1, 15_000, 105_000),
            HumanInputActivityRead::Available(HumanInputActivitySnapshot {
                human_input_quiet_duration_ms: 12_000,
                input_session_id: Some(1)
            })
        );
    }

    #[test]
    fn rejects_stale_snapshot() {
        let read = validate_snapshot(&snapshot(1, 100_000), 1, 15_000, 120_001);

        assert!(matches!(
            read,
            HumanInputActivityRead::Unavailable {
                reason: HumanInputStateUnavailableReason::SnapshotStale,
                ..
            }
        ));
    }

    #[test]
    fn rejects_session_mismatch() {
        let read = validate_snapshot(&snapshot(2, 100_000), 1, 15_000, 105_000);

        assert!(matches!(
            read,
            HumanInputActivityRead::Unavailable {
                reason: HumanInputStateUnavailableReason::SnapshotSessionMismatch,
                ..
            }
        ));
    }
}
