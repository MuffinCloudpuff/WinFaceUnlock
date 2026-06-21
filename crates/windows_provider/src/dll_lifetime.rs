use std::sync::atomic::{AtomicI32, Ordering};

static ACTIVE_WORKER_COUNT: AtomicI32 = AtomicI32::new(0);

pub struct DllWorkerGuard;

impl DllWorkerGuard {
    pub fn new() -> Self {
        ACTIVE_WORKER_COUNT.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for DllWorkerGuard {
    fn drop(&mut self) {
        ACTIVE_WORKER_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn active_worker_count() -> i32 {
    ACTIVE_WORKER_COUNT.load(Ordering::SeqCst)
}
