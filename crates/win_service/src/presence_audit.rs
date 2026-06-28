use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub struct PresenceAuditConfig {
    pub audit_dir: PathBuf,
    pub presence_audit_enabled: bool,
    pub presence_audit_save_full_frame_thumbnail: bool,
    pub presence_audit_save_screen_snapshot: bool,
    pub presence_audit_max_record_count: usize,
}

impl PresenceAuditConfig {
    pub fn program_data_default() -> Self {
        let audit_dir = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join("presence-audit")))
            .unwrap_or_else(|| {
                std::env::temp_dir()
                    .join("WinFaceUnlock")
                    .join("presence-audit")
            });
        Self {
            audit_dir,
            presence_audit_enabled: true,
            presence_audit_save_full_frame_thumbnail: false,
            presence_audit_save_screen_snapshot: true,
            presence_audit_max_record_count: 50,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct UnknownFaceAuditEvent {
    pub event_id: String,
    pub captured_at_unix_ms: i64,
    pub decision: String,
    pub match_score: f32,
    pub presence_owner_match_threshold: f32,
    pub face_crop_path: Option<PathBuf>,
    pub optional_frame_thumbnail_path: Option<PathBuf>,
    pub optional_screen_snapshot_path: Option<PathBuf>,
    pub lock_requested: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub enum PresenceAuditError {
    IoFailed,
    SerializeFailed,
}

pub struct PresenceAuditStore {
    config: PresenceAuditConfig,
}

impl PresenceAuditStore {
    pub fn new(config: PresenceAuditConfig) -> Self {
        Self { config }
    }

    pub fn save_unknown_face_event(
        &self,
        event: &UnknownFaceAuditEvent,
    ) -> Result<PathBuf, PresenceAuditError> {
        if !self.config.presence_audit_enabled {
            return Err(PresenceAuditError::IoFailed);
        }

        fs::create_dir_all(&self.config.audit_dir).map_err(|_| PresenceAuditError::IoFailed)?;
        let path = self
            .config
            .audit_dir
            .join(format!("{}.json", sanitize_event_id(&event.event_id)));
        let bytes =
            serde_json::to_vec_pretty(event).map_err(|_| PresenceAuditError::SerializeFailed)?;
        fs::write(&path, bytes).map_err(|_| PresenceAuditError::IoFailed)?;
        self.enforce_record_limit()?;
        Ok(path)
    }

    pub fn screen_snapshot_enabled(&self) -> bool {
        self.config.presence_audit_enabled && self.config.presence_audit_save_screen_snapshot
    }

    fn enforce_record_limit(&self) -> Result<(), PresenceAuditError> {
        if self.config.presence_audit_max_record_count == 0 {
            return Ok(());
        }

        let mut records = audit_json_records(&self.config.audit_dir)?;
        if records.len() <= self.config.presence_audit_max_record_count {
            return Ok(());
        }

        records.sort_by_key(|record| record.modified_unix_ms);
        let remove_count = records.len() - self.config.presence_audit_max_record_count;
        for record in records.into_iter().take(remove_count) {
            fs::remove_file(record.path).map_err(|_| PresenceAuditError::IoFailed)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct AuditRecordFile {
    path: PathBuf,
    modified_unix_ms: i64,
}

fn audit_json_records(audit_dir: &Path) -> Result<Vec<AuditRecordFile>, PresenceAuditError> {
    let entries = fs::read_dir(audit_dir).map_err(|_| PresenceAuditError::IoFailed)?;
    let mut records = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|_| PresenceAuditError::IoFailed)?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let modified_unix_ms = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        records.push(AuditRecordFile {
            path,
            modified_unix_ms,
        });
    }

    Ok(records)
}

fn sanitize_event_id(event_id: &str) -> String {
    event_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_enables_screen_snapshot_when_presence_lock_is_enabled() {
        let store = PresenceAuditStore::new(PresenceAuditConfig::program_data_default());

        assert!(store.screen_snapshot_enabled());
    }

    #[test]
    fn audit_store_sanitizes_event_id_and_writes_metadata() -> Result<(), PresenceAuditError> {
        let dir = unique_temp_dir("sanitize");
        let store = PresenceAuditStore::new(PresenceAuditConfig {
            audit_dir: dir.clone(),
            presence_audit_enabled: true,
            presence_audit_save_full_frame_thumbnail: false,
            presence_audit_save_screen_snapshot: true,
            presence_audit_max_record_count: 10,
        });

        let path = store.save_unknown_face_event(&UnknownFaceAuditEvent {
            event_id: "unknown:face/1".to_owned(),
            captured_at_unix_ms: 100,
            decision: "unknown_face_detected".to_owned(),
            match_score: 0.12,
            presence_owner_match_threshold: 0.50,
            face_crop_path: None,
            optional_frame_thumbnail_path: None,
            optional_screen_snapshot_path: None,
            lock_requested: false,
        })?;

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("unknown_face_1.json")
        );
        assert!(path.exists());
        let _ = fs::remove_dir_all(dir);
        Ok(())
    }

    #[test]
    fn audit_store_enforces_record_limit() -> Result<(), PresenceAuditError> {
        let dir = unique_temp_dir("limit");
        let store = PresenceAuditStore::new(PresenceAuditConfig {
            audit_dir: dir.clone(),
            presence_audit_enabled: true,
            presence_audit_save_full_frame_thumbnail: false,
            presence_audit_save_screen_snapshot: true,
            presence_audit_max_record_count: 2,
        });

        for index in 0..3 {
            let _ = store.save_unknown_face_event(&UnknownFaceAuditEvent {
                event_id: format!("unknown-face-{index}"),
                captured_at_unix_ms: index,
                decision: "unknown_face_detected".to_owned(),
                match_score: 0.12,
                presence_owner_match_threshold: 0.50,
                face_crop_path: None,
                optional_frame_thumbnail_path: None,
                optional_screen_snapshot_path: None,
                lock_requested: false,
            })?;
        }

        let record_count = audit_json_records(&dir)?.len();
        assert_eq!(record_count, 2);
        let _ = fs::remove_dir_all(dir);
        Ok(())
    }

    fn unique_temp_dir(test_name: &str) -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("winfaceunlock-presence-audit-{test_name}-{suffix}"))
    }
}
