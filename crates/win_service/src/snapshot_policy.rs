use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

/// A revocable permission belonging to one authentication configuration generation.
/// Revoked generations are never enabled again, even if the user later opts in.
#[derive(Clone)]
pub(crate) struct SnapshotPolicy {
    state: Arc<SnapshotPolicyState>,
}

struct SnapshotPolicyState {
    enabled: AtomicBool,
    capture: Mutex<()>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum SnapshotOutcome<T> {
    Disabled,
    Captured(T),
}

impl SnapshotPolicy {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            state: Arc::new(SnapshotPolicyState {
                enabled: AtomicBool::new(enabled),
                capture: Mutex::new(()),
            }),
        }
    }

    /// Prevents new captures immediately and drains any previously permitted write.
    /// After this returns no worker with this policy can write another snapshot.
    pub(crate) fn revoke(&self) {
        self.state.enabled.store(false, Ordering::SeqCst);
        drop(self.state.capture.lock());
    }

    pub(crate) fn capture<T>(&self, capture: impl FnOnce() -> T) -> SnapshotOutcome<T> {
        let Ok(_guard) = self.state.capture.lock() else {
            // A panicking capture must never implicitly restore permission.
            return SnapshotOutcome::Disabled;
        };
        if !self.state.enabled.load(Ordering::SeqCst) {
            return SnapshotOutcome::Disabled;
        }
        SnapshotOutcome::Captured(capture())
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread, time::Duration};

    use super::*;

    #[test]
    fn disabled_policy_does_not_execute_capture() {
        let policy = SnapshotPolicy::new(false);
        let mut captured = false;
        assert_eq!(
            policy.capture(|| captured = true),
            SnapshotOutcome::Disabled
        );
        assert!(!captured);
    }

    #[test]
    fn revocation_reaches_old_workers_and_new_generation_does_not_reenable_them() {
        let policy = SnapshotPolicy::new(true);
        let old_worker = policy.clone();
        assert_eq!(old_worker.capture(|| 1), SnapshotOutcome::Captured(1));
        policy.revoke();
        let replacement = SnapshotPolicy::new(true);
        assert_eq!(replacement.capture(|| 2), SnapshotOutcome::Captured(2));
        assert_eq!(old_worker.capture(|| 3), SnapshotOutcome::Disabled);
    }

    #[test]
    fn revocation_waits_for_active_write_and_blocks_following_writes()
    -> Result<(), Box<dyn std::error::Error>> {
        let policy = SnapshotPolicy::new(true);
        let worker_policy = policy.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            worker_policy.capture(|| {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
            })
        });
        started_rx.recv_timeout(Duration::from_secs(2))?;
        let revoking_policy = policy.clone();
        let (revoked_tx, revoked_rx) = mpsc::channel();
        let revoker = thread::spawn(move || {
            revoking_policy.revoke();
            let _ = revoked_tx.send(());
        });
        assert!(revoked_rx.recv_timeout(Duration::from_millis(20)).is_err());
        release_tx.send(())?;
        revoked_rx.recv_timeout(Duration::from_secs(2))?;
        assert!(worker.join().is_ok());
        assert!(revoker.join().is_ok());
        assert_eq!(policy.capture(|| 1), SnapshotOutcome::Disabled);
        Ok(())
    }
}
