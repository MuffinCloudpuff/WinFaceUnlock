//! Serializes automatic Windows password submissions and latches Windows rejection.
//! No method here authenticates a password. Clearing requires an explicit credential update.

#[cfg(windows)]
mod windows_store;

use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsLogonResult {
    pub status: i32,
    pub substatus: i32,
}

impl WindowsLogonResult {
    pub fn succeeded(self) -> bool {
        self.status == 0 && self.substatus == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogonBlock {
    AwaitingWindowsResult,
    WindowsRejected(WindowsLogonResult),
    StaleAttempt,
    StorageUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubmissionToken(u64);

pub trait LogonSubmissionGuard: Send + Sync {
    fn ready_generation(&self) -> Result<u64, LogonBlock>;
    fn reserve(&self, generation: u64) -> Result<SubmissionToken, LogonBlock>;
    fn complete(
        &self,
        token: SubmissionToken,
        result: WindowsLogonResult,
    ) -> Result<(), LogonBlock>;
    fn cancel_unsubmitted(&self, token: SubmissionToken) -> Result<(), LogonBlock>;
    fn credentials_updated(&self) -> Result<(), LogonBlock>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Phase {
    #[default]
    Ready,
    Submitted,
    Rejected(WindowsLogonResult),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct GuardState {
    generation: u64,
    phase: Phase,
}

impl GuardState {
    fn ready_generation(self) -> Result<u64, LogonBlock> {
        match self.phase {
            Phase::Ready => Ok(self.generation),
            Phase::Submitted => Err(LogonBlock::AwaitingWindowsResult),
            Phase::Rejected(result) => Err(LogonBlock::WindowsRejected(result)),
        }
    }

    fn advance(&mut self) -> Result<(), LogonBlock> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(LogonBlock::StorageUnavailable)?;
        Ok(())
    }

    fn reserve(&mut self, generation: u64) -> Result<SubmissionToken, LogonBlock> {
        if self.ready_generation()? != generation {
            return Err(LogonBlock::StaleAttempt);
        }
        self.advance()?;
        self.phase = Phase::Submitted;
        Ok(SubmissionToken(self.generation))
    }

    fn complete(
        &mut self,
        token: SubmissionToken,
        result: WindowsLogonResult,
    ) -> Result<(), LogonBlock> {
        if self.phase != Phase::Submitted || self.generation != token.0 {
            return Err(LogonBlock::StaleAttempt);
        }
        self.phase = if result.succeeded() {
            Phase::Ready
        } else {
            Phase::Rejected(result)
        };
        Ok(())
    }

    fn credentials_updated(&mut self) -> Result<(), LogonBlock> {
        self.advance()?;
        self.phase = Phase::Ready;
        Ok(())
    }
}

/// Test/embedded guard. Production Credential Provider uses PersistentLogonGuard.
#[derive(Default)]
pub struct MemoryLogonGuard(Mutex<GuardState>);

impl LogonSubmissionGuard for MemoryLogonGuard {
    fn ready_generation(&self) -> Result<u64, LogonBlock> {
        self.0
            .lock()
            .map_err(|_| LogonBlock::StorageUnavailable)?
            .ready_generation()
    }
    fn reserve(&self, generation: u64) -> Result<SubmissionToken, LogonBlock> {
        self.0
            .lock()
            .map_err(|_| LogonBlock::StorageUnavailable)?
            .reserve(generation)
    }
    fn complete(
        &self,
        token: SubmissionToken,
        result: WindowsLogonResult,
    ) -> Result<(), LogonBlock> {
        self.0
            .lock()
            .map_err(|_| LogonBlock::StorageUnavailable)?
            .complete(token, result)
    }
    fn cancel_unsubmitted(&self, token: SubmissionToken) -> Result<(), LogonBlock> {
        self.complete(
            token,
            WindowsLogonResult {
                status: 0,
                substatus: 0,
            },
        )
    }
    fn credentials_updated(&self) -> Result<(), LogonBlock> {
        self.0
            .lock()
            .map_err(|_| LogonBlock::StorageUnavailable)?
            .credentials_updated()
    }
}

#[derive(Default)]
pub struct PersistentLogonGuard;

impl PersistentLogonGuard {
    fn transaction<T>(
        &self,
        operation: impl FnOnce(&mut GuardState) -> Result<T, LogonBlock>,
    ) -> Result<T, LogonBlock> {
        #[cfg(windows)]
        {
            windows_store::transaction(operation)
        }
        #[cfg(not(windows))]
        {
            let _ = operation;
            Err(LogonBlock::StorageUnavailable)
        }
    }
}

impl LogonSubmissionGuard for PersistentLogonGuard {
    fn ready_generation(&self) -> Result<u64, LogonBlock> {
        self.transaction(|state| state.ready_generation())
    }
    fn reserve(&self, generation: u64) -> Result<SubmissionToken, LogonBlock> {
        self.transaction(|state| state.reserve(generation))
    }
    fn complete(
        &self,
        token: SubmissionToken,
        result: WindowsLogonResult,
    ) -> Result<(), LogonBlock> {
        self.transaction(|state| state.complete(token, result))
    }
    fn cancel_unsubmitted(&self, token: SubmissionToken) -> Result<(), LogonBlock> {
        self.complete(
            token,
            WindowsLogonResult {
                status: 0,
                substatus: 0,
            },
        )
    }
    fn credentials_updated(&self) -> Result<(), LogonBlock> {
        self.transaction(GuardState::credentials_updated)
    }
}

#[cfg(test)]
mod tests;
