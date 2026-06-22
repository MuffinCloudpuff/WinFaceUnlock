use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const SNAPSHOT_SOURCE_GET_LAST_INPUT_INFO: &str = "interactive-session-get-last-input-info";
const RUNTIME_DIR_NAME: &str = "runtime";
const SNAPSHOT_FILE_NAME: &str = "desktop-input-state.json";

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Eq, PartialEq)]
pub struct DesktopInputSnapshot {
    pub schema_version: u32,
    pub source: String,
    pub session_id: u32,
    pub agent_process_id: u32,
    pub last_input_tick_ms: u64,
    pub human_input_quiet_duration_ms: u64,
    pub sampled_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopInputSnapshotError {
    Missing,
    ReadFailed,
    DecodeFailed,
    WriteFailed,
}

pub fn default_snapshot_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(snapshot_path_from_install_dir))
        .unwrap_or_else(|| {
            std::env::temp_dir()
                .join("WinFaceUnlock")
                .join(RUNTIME_DIR_NAME)
                .join(SNAPSHOT_FILE_NAME)
        })
}

pub fn snapshot_path_from_install_dir(install_dir: &Path) -> PathBuf {
    install_dir.join(RUNTIME_DIR_NAME).join(SNAPSHOT_FILE_NAME)
}

pub fn unix_time_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

pub fn read_snapshot(path: &Path) -> Result<DesktopInputSnapshot, DesktopInputSnapshotError> {
    let bytes = fs::read(path).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            DesktopInputSnapshotError::Missing
        } else {
            DesktopInputSnapshotError::ReadFailed
        }
    })?;
    serde_json::from_slice(&bytes).map_err(|_| DesktopInputSnapshotError::DecodeFailed)
}

pub fn write_snapshot_atomic(
    path: &Path,
    snapshot: &DesktopInputSnapshot,
) -> Result<(), DesktopInputSnapshotError> {
    write_snapshot_atomic_inner(path, snapshot).map_err(|_| DesktopInputSnapshotError::WriteFailed)
}

fn write_snapshot_atomic_inner(path: &Path, snapshot: &DesktopInputSnapshot) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        snapshot.sampled_at_unix_ms
    ));
    let bytes = serde_json::to_vec_pretty(snapshot).map_err(io::Error::other)?;
    fs::write(&temp_path, bytes)?;
    replace_file(&temp_path, path)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    let _ = fs::remove_file(target_path);
    fs::rename(temp_path, target_path)
}

#[cfg(not(windows))]
fn replace_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    fs::rename(temp_path, target_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trips_through_atomic_write() {
        let dir = std::env::temp_dir().join(format!(
            "winfaceunlock-desktop-input-test-{}",
            unix_time_ms_now()
        ));
        let path = dir.join("desktop-input-state.json");
        let snapshot = DesktopInputSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            source: SNAPSHOT_SOURCE_GET_LAST_INPUT_INFO.to_owned(),
            session_id: 7,
            agent_process_id: 123,
            last_input_tick_ms: 456,
            human_input_quiet_duration_ms: 789,
            sampled_at_unix_ms: unix_time_ms_now(),
        };

        let write_result = write_snapshot_atomic(&path, &snapshot);

        assert!(write_result.is_ok(), "{write_result:?}");
        assert_eq!(read_snapshot(&path), Ok(snapshot));
        let _ = fs::remove_dir_all(dir);
    }
}
