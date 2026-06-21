use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    thread,
};

use common_protocol::{AuthFailureReason, AuthGrant, AuthSource, AuthTriggerSource, SessionId};
use ipc::{AuthGrantIssueResult, AuthGrantIssuer};

use crate::service_log::write_service_event_detail;

#[derive(Clone)]
pub struct CameraAuthOrchestrator<I> {
    issuer: Arc<Mutex<I>>,
    running_sessions: Arc<Mutex<HashSet<SessionId>>>,
    cached_grant_outcomes: Arc<Mutex<HashMap<SessionId, Result<AuthGrant, AuthFailureReason>>>>,
}

impl<I> CameraAuthOrchestrator<I>
where
    I: AuthGrantIssuer + Send + 'static,
{
    pub fn new(issuer: I) -> Self {
        Self {
            issuer: Arc::new(Mutex::new(issuer)),
            running_sessions: Arc::new(Mutex::new(HashSet::new())),
            cached_grant_outcomes: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<I> AuthGrantIssuer for CameraAuthOrchestrator<I>
where
    I: AuthGrantIssuer + Send + 'static,
{
    fn issue_auth_grant(
        &mut self,
        session_id: &SessionId,
        source: AuthSource,
        trigger_source: AuthTriggerSource,
        issued_at_unix_ms: i64,
    ) -> AuthGrantIssueResult {
        if source != AuthSource::LocalCamera {
            return self.issuer.lock().map_or(
                AuthGrantIssueResult::Failed(AuthFailureReason::InternalError),
                |mut issuer| {
                    issuer.issue_auth_grant(session_id, source, trigger_source, issued_at_unix_ms)
                },
            );
        }

        {
            let mut cached_grant_outcomes = match self.cached_grant_outcomes.lock() {
                Ok(cached_grant_outcomes) => cached_grant_outcomes,
                Err(_) => {
                    return AuthGrantIssueResult::Failed(AuthFailureReason::InternalError);
                }
            };
            match cached_grant_outcomes.get(session_id) {
                Some(Ok(grant)) => {
                    if !grant.is_expired_at(issued_at_unix_ms) {
                        write_service_event_detail(
                            "CameraAuthOrchestrator.AuthGrantCacheHit",
                            format!("session_id={}", session_id.0),
                        );
                        return AuthGrantIssueResult::Issued(grant.clone());
                    }
                    write_service_event_detail(
                        "CameraAuthOrchestrator.AuthGrantCacheExpired",
                        format!("session_id={}", session_id.0),
                    );
                    cached_grant_outcomes.remove(session_id);
                }
                Some(Err(_)) => {
                    write_service_event_detail(
                        "CameraAuthOrchestrator.PreviousRejectionCleared",
                        format!("session_id={}", session_id.0),
                    );
                    cached_grant_outcomes.remove(session_id);
                }
                None => {}
            }
        }

        {
            let mut running_sessions = match self.running_sessions.lock() {
                Ok(running_sessions) => running_sessions,
                Err(_) => {
                    return AuthGrantIssueResult::Failed(AuthFailureReason::InternalError);
                }
            };
            if running_sessions.contains(session_id) {
                write_service_event_detail(
                    "CameraAuthOrchestrator.JobAlreadyRunning",
                    format!("session_id={}", session_id.0),
                );
                return AuthGrantIssueResult::Started;
            }
            running_sessions.insert(session_id.clone());
        }

        let session_id_for_worker = session_id.clone();
        let issuer = Arc::clone(&self.issuer);
        let running_sessions = Arc::clone(&self.running_sessions);
        let cached_grant_outcomes = Arc::clone(&self.cached_grant_outcomes);
        match thread::Builder::new()
            .name("winfaceunlock-auth-job".to_owned())
            .spawn(move || {
                write_service_event_detail(
                    "CameraAuthJob.Started",
                    format!("session_id={}", session_id_for_worker.0),
                );
                let result = issuer
                    .lock()
                    .map_err(|_| AuthFailureReason::InternalError)
                    .and_then(|mut issuer| {
                        match issuer.issue_auth_grant(
                            &session_id_for_worker,
                            source,
                            trigger_source,
                            issued_at_unix_ms,
                        ) {
                            AuthGrantIssueResult::Issued(grant) => Ok(grant),
                            AuthGrantIssueResult::Failed(reason) => Err(reason),
                            AuthGrantIssueResult::Started => Err(AuthFailureReason::InternalError),
                        }
                    });
                match &result {
                    Ok(_) => write_service_event_detail(
                        "CameraAuthJob.AuthGrantIssued",
                        format!("session_id={}", session_id_for_worker.0),
                    ),
                    Err(reason) => write_service_event_detail(
                        "CameraAuthJob.AuthRejected",
                        format!("session_id={} reason={reason:?}", session_id_for_worker.0),
                    ),
                }
                if let Ok(mut cached_grant_outcomes) = cached_grant_outcomes.lock() {
                    cached_grant_outcomes.insert(session_id_for_worker.clone(), result);
                }
                if let Ok(mut running_sessions) = running_sessions.lock() {
                    running_sessions.remove(&session_id_for_worker);
                }
            }) {
            Ok(_) => AuthGrantIssueResult::Started,
            Err(_) => {
                if let Ok(mut running_sessions) = self.running_sessions.lock() {
                    running_sessions.remove(session_id);
                }
                AuthGrantIssueResult::Failed(AuthFailureReason::InternalError)
            }
        }
    }

    fn fetch_auth_result(
        &mut self,
        session_id: &SessionId,
        _issued_at_unix_ms: i64,
    ) -> Option<Result<AuthGrant, AuthFailureReason>> {
        self.cached_grant_outcomes
            .lock()
            .ok()?
            .get(session_id)
            .cloned()
    }

    fn cancel_auth(&mut self, session_id: &SessionId) {
        if let Ok(mut running_sessions) = self.running_sessions.lock() {
            running_sessions.remove(session_id);
        }
        if let Ok(mut cached_grant_outcomes) = self.cached_grant_outcomes.lock() {
            cached_grant_outcomes.remove(session_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use common_protocol::{AuthScore, GrantId, Nonce, UserId};

    use super::*;

    struct ImmediateIssuer;

    impl AuthGrantIssuer for ImmediateIssuer {
        fn issue_auth_grant(
            &mut self,
            session_id: &SessionId,
            source: AuthSource,
            _trigger_source: AuthTriggerSource,
            issued_at_unix_ms: i64,
        ) -> AuthGrantIssueResult {
            AuthGrantIssueResult::Issued(AuthGrant {
                grant_id: GrantId("grant-1".to_owned()),
                nonce: Nonce("nonce-1".to_owned()),
                session_id: session_id.clone(),
                user_id: UserId("user-1".to_owned()),
                source,
                score: AuthScore {
                    match_score: 0.9,
                    liveness_score: None,
                },
                issued_at_unix_ms,
                expires_at_unix_ms: issued_at_unix_ms + 5_000,
            })
        }
    }

    #[test]
    fn local_camera_auth_runs_as_background_job() {
        let mut runner = CameraAuthOrchestrator::new(ImmediateIssuer);
        let session_id = SessionId("session-1".to_owned());

        assert_eq!(
            runner.issue_auth_grant(
                &session_id,
                AuthSource::LocalCamera,
                AuthTriggerSource::InputTriggered,
                1_000,
            ),
            AuthGrantIssueResult::Started
        );

        let mut completed = None;
        for _ in 0..100 {
            completed = runner.fetch_auth_result(&session_id, 1_000);
            if completed.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(matches!(completed, Some(Ok(_))));
    }

    #[test]
    fn completed_local_camera_auth_can_be_fetched_by_next_provider_instance() {
        let mut runner = CameraAuthOrchestrator::new(ImmediateIssuer);
        let session_id = SessionId("stable-session".to_owned());

        assert_eq!(
            runner.issue_auth_grant(
                &session_id,
                AuthSource::LocalCamera,
                AuthTriggerSource::InputTriggered,
                1_000,
            ),
            AuthGrantIssueResult::Started
        );

        let mut completed = None;
        for _ in 0..100 {
            completed = runner.fetch_auth_result(&session_id, 1_000);
            if completed.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(completed.is_some());

        assert!(matches!(
            runner.issue_auth_grant(
                &session_id,
                AuthSource::LocalCamera,
                AuthTriggerSource::InputTriggered,
                1_000,
            ),
            AuthGrantIssueResult::Issued(_)
        ));
    }

    #[test]
    fn expired_completed_local_camera_auth_starts_a_new_job() {
        let mut runner = CameraAuthOrchestrator::new(ImmediateIssuer);
        let session_id = SessionId("stable-session".to_owned());

        assert_eq!(
            runner.issue_auth_grant(
                &session_id,
                AuthSource::LocalCamera,
                AuthTriggerSource::InputTriggered,
                1_000,
            ),
            AuthGrantIssueResult::Started
        );

        for _ in 0..100 {
            if runner.fetch_auth_result(&session_id, 1_000).is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert_eq!(
            runner.issue_auth_grant(
                &session_id,
                AuthSource::LocalCamera,
                AuthTriggerSource::InputTriggered,
                6_000,
            ),
            AuthGrantIssueResult::Started
        );
    }

    #[test]
    fn duplicate_local_camera_trigger_does_not_start_second_job() {
        struct SlowIssuer {
            started_count: Arc<AtomicUsize>,
        }

        impl AuthGrantIssuer for SlowIssuer {
            fn issue_auth_grant(
                &mut self,
                session_id: &SessionId,
                source: AuthSource,
                _trigger_source: AuthTriggerSource,
                issued_at_unix_ms: i64,
            ) -> AuthGrantIssueResult {
                self.started_count.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(50));
                AuthGrantIssueResult::Issued(AuthGrant {
                    grant_id: GrantId("grant-1".to_owned()),
                    nonce: Nonce("nonce-1".to_owned()),
                    session_id: session_id.clone(),
                    user_id: UserId("user-1".to_owned()),
                    source,
                    score: AuthScore {
                        match_score: 0.9,
                        liveness_score: None,
                    },
                    issued_at_unix_ms,
                    expires_at_unix_ms: issued_at_unix_ms + 5_000,
                })
            }
        }

        let started_count = Arc::new(AtomicUsize::new(0));
        let mut runner = CameraAuthOrchestrator::new(SlowIssuer {
            started_count: Arc::clone(&started_count),
        });
        let session_id = SessionId("stable-session".to_owned());

        assert_eq!(
            runner.issue_auth_grant(
                &session_id,
                AuthSource::LocalCamera,
                AuthTriggerSource::InputTriggered,
                1_000,
            ),
            AuthGrantIssueResult::Started
        );
        assert_eq!(
            runner.issue_auth_grant(
                &session_id,
                AuthSource::LocalCamera,
                AuthTriggerSource::InputTriggered,
                1_000,
            ),
            AuthGrantIssueResult::Started
        );

        for _ in 0..100 {
            if runner.fetch_auth_result(&session_id, 1_000).is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(started_count.load(Ordering::SeqCst), 1);
    }
}
