use std::{
    fs::{self, OpenOptions, create_dir_all},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;
const LOG_BACKUPS: usize = 3;
static LOG_WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn write_service_event(event_name: &str) {
    let _ = write_service_event_inner(event_name);
}

pub fn write_service_event_detail(event_name: &str, detail: impl AsRef<str>) {
    let _ = write_service_event_inner(&format!("{} {}", event_name, detail.as_ref()));
}

fn write_service_event_inner(event_name: &str) -> std::io::Result<()> {
    let _guard = LOG_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = log_path();
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    append_event(&path, event_name, MAX_LOG_BYTES)?;
    Ok(())
}

fn append_event(path: &Path, event_name: &str, max_bytes: u64) -> std::io::Result<()> {
    let line = format!("{} {}\n", timestamp_unix_ms(), event_name);
    let size = match fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    if size > 0 && size.saturating_add(line.len() as u64) > max_bytes {
        rotate_log(path)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())
}

fn backup_path(path: &Path, index: usize) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

fn rotate_log(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(backup_path(path, LOG_BACKUPS)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    for index in (1..LOG_BACKUPS).rev() {
        match fs::rename(backup_path(path, index), backup_path(path, index + 1)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    fs::rename(path, backup_path(path, 1))
}

fn log_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("logs")))
        .unwrap_or_else(|| std::env::temp_dir().join("WinFaceUnlock").join("logs"))
        .join("service.log")
}

fn timestamp_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_is_nonzero() {
        assert!(timestamp_unix_ms() > 0);
    }

    #[test]
    fn rotation_keeps_recent_events_and_limits_backup_count() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!(
            "winfaceunlock-log-{}-{}",
            std::process::id(),
            timestamp_unix_ms()
        ));
        fs::create_dir_all(&dir)?;
        let path = dir.join("service.log");
        for index in 0..8 {
            append_event(&path, &format!("event-{index}"), 1)?;
        }
        assert!(fs::read_to_string(&path)?.contains("event-7"));
        for index in 1..=LOG_BACKUPS {
            assert!(
                fs::read_to_string(backup_path(&path, index))?
                    .contains(&format!("event-{}", 7 - index))
            );
        }
        assert!(!backup_path(&path, LOG_BACKUPS + 1).exists());
        fs::remove_dir_all(dir)
    }
}
