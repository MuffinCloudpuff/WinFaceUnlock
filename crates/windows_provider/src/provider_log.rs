use std::{
    fs::{OpenOptions, create_dir_all},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const LOG_DIR: &str = r"C:\ProgramData\WinFaceUnlock";
const LOG_FILE: &str = "provider.log";

pub fn write_provider_event(event_name: &str) {
    let _ = write_provider_event_inner(event_name);
}

pub fn write_provider_event_detail(event_name: &str, detail: impl AsRef<str>) {
    let _ = write_provider_event_inner(&format!("{} {}", event_name, detail.as_ref()));
}

fn write_provider_event_inner(event_name: &str) -> std::io::Result<()> {
    create_dir_all(LOG_DIR)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())?;
    writeln!(file, "{} {}", timestamp_unix_ms(), event_name)?;
    Ok(())
}

fn log_path() -> PathBuf {
    PathBuf::from(LOG_DIR).join(LOG_FILE)
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
    fn provider_log_path_uses_project_program_data_directory() {
        assert_eq!(
            log_path(),
            PathBuf::from(r"C:\ProgramData\WinFaceUnlock\provider.log")
        );
    }
}
