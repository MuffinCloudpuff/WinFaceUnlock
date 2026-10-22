use super::*;
use std::sync::Arc;

const OK: WindowsLogonResult = WindowsLogonResult {
    status: 0,
    substatus: 0,
};
const LOCKED: WindowsLogonResult = WindowsLogonResult {
    status: 0xC0000234u32 as i32,
    substatus: 0,
};

#[test]
fn windows_rejection_blocks_every_later_submission_until_explicit_update() -> Result<(), LogonBlock>
{
    for result in [
        LOCKED,
        WindowsLogonResult {
            status: 0xC000006Du32 as i32,
            substatus: 0xC000006Au32 as i32,
        },
        WindowsLogonResult {
            status: 0,
            substatus: 1,
        },
    ] {
        let guard = MemoryLogonGuard::default();
        let token = guard.reserve(guard.ready_generation()?)?;
        guard.complete(token, result)?;
        for _ in 0..100 {
            assert_eq!(guard.reserve(0), Err(LogonBlock::WindowsRejected(result)));
        }
        guard.credentials_updated()?;
        assert!(guard.reserve(guard.ready_generation()?).is_ok());
    }
    Ok(())
}

#[test]
fn concurrent_provider_instances_can_only_reserve_once() -> Result<(), LogonBlock> {
    let guard = Arc::new(MemoryLogonGuard::default());
    let generation = guard.ready_generation()?;
    let joins: Vec<_> = (0..16)
        .map(|_| {
            let guard = guard.clone();
            std::thread::spawn(move || guard.reserve(generation))
        })
        .collect();
    let mut successful = 0;
    for join in joins {
        if join
            .join()
            .map_err(|_| LogonBlock::StorageUnavailable)?
            .is_ok()
        {
            successful += 1;
        }
    }
    assert_eq!(successful, 1);
    assert_eq!(
        guard.ready_generation(),
        Err(LogonBlock::AwaitingWindowsResult)
    );
    Ok(())
}

#[test]
fn credential_update_invalidates_old_worker_and_late_windows_result() -> Result<(), LogonBlock> {
    let guard = MemoryLogonGuard::default();
    let old_generation = guard.ready_generation()?;
    let token = guard.reserve(old_generation)?;
    guard.credentials_updated()?;
    assert_eq!(guard.complete(token, LOCKED), Err(LogonBlock::StaleAttempt));
    assert_eq!(guard.reserve(old_generation), Err(LogonBlock::StaleAttempt));
    assert!(guard.ready_generation().is_ok());
    Ok(())
}

#[test]
fn success_does_not_allow_replay_of_the_previous_generation() -> Result<(), LogonBlock> {
    let guard = MemoryLogonGuard::default();
    let generation = guard.ready_generation()?;
    let token = guard.reserve(generation)?;
    guard.complete(token, OK)?;
    assert_eq!(guard.reserve(generation), Err(LogonBlock::StaleAttempt));
    assert_eq!(guard.complete(token, LOCKED), Err(LogonBlock::StaleAttempt));
    Ok(())
}

#[test]
fn abandoned_submission_stays_blocked_without_a_password_probe() -> Result<(), LogonBlock> {
    let guard = MemoryLogonGuard::default();
    let _token = guard.reserve(guard.ready_generation()?)?;
    for _ in 0..100 {
        assert_eq!(
            guard.ready_generation(),
            Err(LogonBlock::AwaitingWindowsResult)
        );
    }
    Ok(())
}
