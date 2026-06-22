use std::{
    fs::{OpenOptions, create_dir_all},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn write_service_event(event_name: &str) {
    let _ = write_service_event_inner(event_name);
}

pub fn write_service_event_detail(event_name: &str, detail: impl AsRef<str>) {
    let _ = write_service_event_inner(&format!("{} {}", event_name, detail.as_ref()));
}

fn write_service_event_inner(event_name: &str) -> std::io::Result<()> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{} {}", timestamp_unix_ms(), event_name)?;
    Ok(())
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
}
