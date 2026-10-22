use windows::Win32::UI::Shell::CPUS_UNLOCK_WORKSTATION;

use super::*;

fn start_request(state: &ProviderState) -> u64 {
    let start = state.begin_wake_request(3);
    assert!(matches!(start, WakeRequestStart::Started { .. }));
    match start {
        WakeRequestStart::Started { request_id, .. } => request_id,
        _ => 0,
    }
}

fn material() -> CredentialMaterial {
    CredentialMaterial {
        domain: ".".into(),
        username: "test-user".into(),
        password: "test-only".into(),
    }
}

#[test]
fn usage_scenario_defaults_to_invalid_then_updates_explicitly() {
    let state = ProviderState::new();

    assert_eq!(state.usage_scenario(), CPUS_INVALID);
    state.set_usage_scenario(CPUS_UNLOCK_WORKSTATION);
    assert_eq!(state.usage_scenario(), CPUS_UNLOCK_WORKSTATION);
}

#[test]
fn visible_mode_shows_one_tile_before_credential_material_exists() {
    let state =
        ProviderState::with_tile_visibility(ProviderTileVisibility::VisibleBeforeCredentialReady);

    let plan = state.credential_count_plan();

    assert_eq!(plan.credential_count, 1);
    assert_eq!(
        plan.default_credential_index,
        CREDENTIAL_PROVIDER_NO_DEFAULT
    );
    assert!(!plan.auto_logon_with_default);
}

#[test]
fn hidden_mode_hides_tile_before_credential_material_exists() {
    let state =
        ProviderState::with_tile_visibility(ProviderTileVisibility::HiddenUntilCredentialReady);

    let plan = state.credential_count_plan();

    assert_eq!(plan.credential_count, 0);
    assert_eq!(
        plan.default_credential_index,
        CREDENTIAL_PROVIDER_NO_DEFAULT
    );
    assert!(!plan.auto_logon_with_default);
}

#[test]
fn wake_request_can_start_once_until_result_arrives() {
    let state = ProviderState::new();

    assert!(matches!(
        state.begin_wake_request(3),
        WakeRequestStart::Started { .. }
    ));
    assert_eq!(
        state.begin_wake_request(3),
        WakeRequestStart::Blocked {
            reason: WakeRequestBlockReason::WakeAlreadyRunning
        }
    );
    assert_eq!(
        state.credential_status_message(),
        "Local face authentication is running (1/3)"
    );
}

#[test]
fn hidden_mode_stays_hidden_while_wake_is_running() {
    let state =
        ProviderState::with_tile_visibility(ProviderTileVisibility::HiddenUntilCredentialReady);

    assert!(matches!(
        state.begin_wake_request(3),
        WakeRequestStart::Started { .. }
    ));
    let plan = state.credential_count_plan();

    assert_eq!(plan.credential_count, 0);
    assert_eq!(
        plan.default_credential_index,
        CREDENTIAL_PROVIDER_NO_DEFAULT
    );
    assert!(!plan.auto_logon_with_default);
}

#[test]
fn wake_session_id_is_stable_across_provider_instances() {
    assert_eq!(
        ProviderState::session_id_for_current_process(),
        SessionId("credential-provider-logon-auto-wake".to_owned())
    );
}

#[test]
fn credential_material_requests_default_auto_logon() {
    let state =
        ProviderState::with_tile_visibility(ProviderTileVisibility::HiddenUntilCredentialReady);

    state.prepare_credential_material(CredentialMaterial {
        domain: ".".to_owned(),
        username: "leo16".to_owned(),
        password: "secret".to_owned(),
    });
    let plan = state.credential_count_plan();

    assert_eq!(plan.credential_count, 1);
    assert_eq!(plan.default_credential_index, 0);
    assert!(plan.auto_logon_with_default);
}

#[test]
fn serialized_credential_material_is_consumed_after_use() {
    let state = ProviderState::new();
    state.prepare_credential_material(CredentialMaterial {
        domain: ".".to_owned(),
        username: "leo16".to_owned(),
        password: "secret".to_owned(),
    });

    assert!(state.take_credential_for_submission().is_some());
    let plan = state.credential_count_plan();

    assert_eq!(plan.credential_count, 1);
    assert_eq!(
        plan.default_credential_index,
        CREDENTIAL_PROVIDER_NO_DEFAULT
    );
    assert!(!plan.auto_logon_with_default);
    assert_eq!(state.take_credential_for_submission(), None);
}

#[test]
fn wake_failure_enforces_retry_cooldown() {
    let state = ProviderState::new();

    let request_id = start_request(&state);
    state.apply_wake_transport_error(request_id, ProtocolError::TransportUnavailable, 1, 3);

    assert_eq!(
        state.begin_wake_request(3),
        WakeRequestStart::Blocked {
            reason: WakeRequestBlockReason::RetryCooldownActive
        }
    );
}

#[test]
fn intermediate_auth_failure_reports_automatic_retry() {
    let state = ProviderState::new();
    let request_id = start_request(&state);

    state.apply_wake_outcome(
        request_id,
        ProviderWakeOutcome::AuthFailed {
            session_id: SessionId("session-1".to_owned()),
            auth_failure_reason: common_protocol::AuthFailureReason::LivenessFailed,
        },
        1,
        3,
        true,
    );

    assert_eq!(
        state.credential_status_message(),
        "Face authentication failed. Retrying automatically (1/3)"
    );

    assert!(state.mark_automatic_retry_started(request_id, 2, 3));
    assert_eq!(
        state.credential_status_message(),
        "Local face authentication is running (2/3)"
    );
}

#[test]
fn final_auth_failure_reports_pin_or_password_fallback() {
    let state = ProviderState::new();
    let request_id = start_request(&state);

    state.apply_wake_outcome(
        request_id,
        ProviderWakeOutcome::AuthFailed {
            session_id: SessionId("session-1".to_owned()),
            auth_failure_reason: common_protocol::AuthFailureReason::LivenessFailed,
        },
        3,
        3,
        false,
    );

    assert_eq!(
        state.credential_status_message(),
        "Face authentication failed. Use PIN or password fallback"
    );
}

#[test]
fn intermediate_auth_failure_keeps_face_tile_visible_during_retry() {
    let state = ProviderState::new();
    let request_id = start_request(&state);

    state.apply_wake_outcome(
        request_id,
        ProviderWakeOutcome::AuthFailed {
            session_id: SessionId("session-1".to_owned()),
            auth_failure_reason: common_protocol::AuthFailureReason::MatchBelowThreshold,
        },
        1,
        3,
        true,
    );

    let plan = state.credential_count_plan();

    assert_eq!(plan.credential_count, 1);
    assert_eq!(
        plan.default_credential_index,
        CREDENTIAL_PROVIDER_NO_DEFAULT
    );
    assert!(!plan.auto_logon_with_default);
}

#[test]
fn credential_material_debug_redacts_password() {
    let material = CredentialMaterial {
        domain: ".".to_owned(),
        username: "leo16".to_owned(),
        password: "secret".to_owned(),
    };

    let debug = format!("{material:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("secret"));
}

#[test]
fn account_lockout_stops_reselection_readvise_and_new_provider_instances() {
    let guard = Arc::new(logon_safety::MemoryLogonGuard::default());
    let state = ProviderState::with_guard(
        ProviderTileVisibility::HiddenUntilCredentialReady,
        guard.clone(),
    );
    let request_id = start_request(&state);
    state.apply_wake_outcome(
        request_id,
        ProviderWakeOutcome::CredentialMaterialReady {
            session_id: SessionId("test".into()),
            credential_material: material(),
        },
        1,
        3,
        false,
    );
    assert!(state.take_credential_for_submission().is_some());
    state.mark_report_result_received(WindowsLogonResult {
        status: 0xC0000234u32 as i32,
        substatus: 0,
    });
    state.set_events(None, 0);
    state.set_usage_scenario(CPUS_UNLOCK_WORKSTATION);
    for _ in 0..20 {
        assert!(matches!(
            state.begin_wake_request(3),
            WakeRequestStart::Blocked { .. }
        ));
        assert!(state.take_credential_for_submission().is_none());
        let recreated = ProviderState::with_guard(
            ProviderTileVisibility::HiddenUntilCredentialReady,
            guard.clone(),
        );
        assert!(matches!(
            recreated.begin_wake_request(3),
            WakeRequestStart::Blocked { .. }
        ));
    }
    assert!(!state.credential_count_plan().auto_logon_with_default);
}

#[test]
fn waiting_for_windows_never_launches_another_wake_or_serializes_twice() {
    let state = Arc::new(ProviderState::new());
    state.prepare_credential_material(material());
    let joins: Vec<_> = (0..10)
        .map(|_| {
            let state = state.clone();
            std::thread::spawn(move || state.take_credential_for_submission().is_some())
        })
        .collect();
    let accepted = joins
        .into_iter()
        .map(|join| {
            let result = join.join();
            assert!(result.is_ok());
            matches!(result, Ok(true))
        })
        .filter(|accepted| *accepted)
        .count();
    assert_eq!(accepted, 1);
    assert!(matches!(
        state.begin_wake_request(3),
        WakeRequestStart::Blocked { .. }
    ));
}

#[test]
fn late_worker_cannot_resurrect_credential_after_windows_rejection() {
    let state = ProviderState::new();
    let request_id = start_request(&state);
    state.prepare_credential_material(material());
    assert!(state.take_credential_for_submission().is_some());
    state.mark_report_result_received(WindowsLogonResult {
        status: 0xC000006Du32 as i32,
        substatus: 0xC000006Au32 as i32,
    });
    state.apply_wake_outcome(
        request_id,
        ProviderWakeOutcome::CredentialMaterialReady {
            session_id: SessionId("stale".into()),
            credential_material: material(),
        },
        2,
        3,
        false,
    );
    state.apply_wake_transport_error(request_id, ProtocolError::TransportUnavailable, 2, 3);
    assert!(!state.mark_automatic_retry_started(request_id, 3, 3));
    assert!(state.take_credential_for_submission().is_none());
    assert!(state.credential_status_message().contains("paused"));
}

#[test]
fn unadvised_worker_cannot_publish_into_a_new_wake_request() {
    let state = ProviderState::new();
    let old_id = start_request(&state);
    state.set_events(None, 0);
    let new_id = start_request(&state);
    assert_ne!(old_id, new_id);
    assert!(!state.is_wake_request_current(old_id));
    assert!(state.is_wake_request_current(new_id));
    state.apply_wake_outcome(
        old_id,
        ProviderWakeOutcome::CredentialMaterialReady {
            session_id: SessionId("stale".into()),
            credential_material: material(),
        },
        1,
        3,
        false,
    );
    assert!(state.take_credential_for_submission().is_none());
    state.apply_wake_outcome(
        new_id,
        ProviderWakeOutcome::CredentialMaterialReady {
            session_id: SessionId("current".into()),
            credential_material: material(),
        },
        1,
        3,
        false,
    );
    assert!(state.take_credential_for_submission().is_some());
}

#[test]
fn updated_credentials_invalidate_material_fetched_before_update() -> Result<(), LogonBlock> {
    let guard = Arc::new(logon_safety::MemoryLogonGuard::default());
    let state = ProviderState::with_guard(ProviderTileVisibility::default(), guard.clone());
    state.prepare_credential_material(material());
    guard.credentials_updated()?;
    assert!(state.take_credential_for_submission().is_none());
    Ok(())
}
