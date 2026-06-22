pub const USAGE_AUDIT_EVENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Eq, PartialEq)]
pub struct UsageAuditEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub event_type: UsageAuditEventType,
    pub session_id: u32,
    pub occurred_at_unix_ms: u64,
    pub actor_context: UsageAuditActorContext,
    pub details: UsageAuditEventDetails,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Eq, PartialEq)]
pub struct UsageAuditActorContext {
    pub windows_user_sid: Option<String>,
    pub is_owner_session: Option<bool>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UsageAuditEventType {
    SessionLogon,
    SessionUnlock,
    SessionLock,
    SessionLogoff,
    ForegroundWindowChanged,
    ProcessStarted,
    ProcessExited,
    UsbDeviceArrived,
    UsbDeviceRemoved,
    FileAccessObserved,
    FileChangeObserved,
    HumanInputActivitySummary,
    UnknownFaceObserved,
    ScreenSnapshotCaptured,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UsageAuditEventDetails {
    ForegroundWindow {
        process_name: String,
        window_title: Option<String>,
    },
    Process {
        process_id: u32,
        process_name: String,
        executable_path: Option<String>,
    },
    Device {
        device_id: String,
        device_name: Option<String>,
    },
    File {
        path: String,
        operation: String,
    },
    HumanInputActivitySummary {
        keyboard_event_count: u64,
        mouse_event_count: u64,
        interval_ms: u64,
    },
    UnknownFace {
        audit_record_id: String,
    },
    ScreenSnapshot {
        snapshot_ref: String,
    },
    Session,
}

impl UsageAuditEvent {
    pub fn new(
        event_id: String,
        event_type: UsageAuditEventType,
        session_id: u32,
        occurred_at_unix_ms: u64,
        actor_context: UsageAuditActorContext,
        details: UsageAuditEventDetails,
    ) -> Self {
        Self {
            schema_version: USAGE_AUDIT_EVENT_SCHEMA_VERSION,
            event_id,
            event_type,
            session_id,
            occurred_at_unix_ms,
            actor_context,
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_input_activity_summary_serializes_with_stable_names() {
        let event = UsageAuditEvent::new(
            "audit-1".to_owned(),
            UsageAuditEventType::HumanInputActivitySummary,
            4,
            123_456,
            UsageAuditActorContext {
                windows_user_sid: None,
                is_owner_session: Some(true),
            },
            UsageAuditEventDetails::HumanInputActivitySummary {
                keyboard_event_count: 5,
                mouse_event_count: 9,
                interval_ms: 60_000,
            },
        );

        let serialized = serde_json::to_string(&event);

        assert!(
            serialized.as_ref().is_ok_and(
                |value| value.contains("\"event_type\":\"human_input_activity_summary\"")
            ),
            "{serialized:?}"
        );
        assert!(
            serialized
                .as_ref()
                .is_ok_and(|value| value.contains("\"kind\":\"human_input_activity_summary\"")),
            "{serialized:?}"
        );
    }
}
