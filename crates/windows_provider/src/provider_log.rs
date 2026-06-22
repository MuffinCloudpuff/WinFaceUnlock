use std::{
    fs::{OpenOptions, create_dir_all},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::provider_config::provider_dll_path_from_registry;

const LOG_FILE: &str = "provider.log";

pub fn write_provider_event(event_name: &str) {
    let _ = write_provider_event_inner(event_name);
}

pub fn write_provider_event_detail(event_name: &str, detail: impl AsRef<str>) {
    let _ = write_provider_event_inner(&format!("{} {}", event_name, detail.as_ref()));
}

fn write_provider_event_inner(event_name: &str) -> std::io::Result<()> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{} {}", timestamp_unix_ms(), event_name)?;
    Ok(())
}

fn log_path() -> PathBuf {
    provider_dll_path_from_registry()
        .and_then(|path| install_log_dir_from_provider_dll_path(&path))
        .unwrap_or_else(|| std::env::temp_dir().join("WinFaceUnlock").join("logs"))
        .join(LOG_FILE)
}

fn install_log_dir_from_provider_dll_path(provider_dll_path: &std::path::Path) -> Option<PathBuf> {
    let parent = provider_dll_path.parent()?;
    let install_dir = if parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("provider"))
    {
        parent.parent()?
    } else {
        parent
    };
    Some(install_dir.join("logs"))
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
    fn provider_log_path_follows_provider_install_directory() {
        assert_eq!(
            install_log_dir_from_provider_dll_path(std::path::Path::new(
                r"D:\tools\WinFaceUnlock\provider\windows_provider-hash.dll"
            )),
            Some(PathBuf::from(r"D:\tools\WinFaceUnlock\logs"))
        );
    }
}
