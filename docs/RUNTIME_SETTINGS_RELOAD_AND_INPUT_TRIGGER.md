# Runtime Settings Reload and Input Trigger

## Problem

Two settings are user-visible but were not runtime-clean:

- `presence_lock_enabled` was persisted, but an already-running presence monitor kept using the old value.
- `input_triggered` wake was described as ignoring `Win+L`, which is the wrong model. The provider should be a lock-screen state machine: only input observed after the credential provider is advised by Windows counts as an unlock trigger.

## Decision

Do not restart the Windows service for settings changes. The service owns the running presence monitor, so the cheap fix is to send the monitor controller a reload command after settings are persisted.

Runtime behavior:

- UI sends `ApplyControlSettings`.
- Service persists the patch.
- If `presence_lock_enabled` changed, service stops the current monitor and starts a new one for the active user session only when the new config enables it.
- Provider input-triggered wake starts from the Windows credential-provider state: `SetUsageScenario(CPUS_LOGON | CPUS_UNLOCK_WORKSTATION)` then `Advise`. It captures an input baseline at `Advise` and only treats a later `GetLastInputInfo` tick change as the trigger.
- Default install mode is `input-triggered`. `background-policy` and `hybrid` are explicit user choices because they intentionally allow camera activity as soon as the lock UI loads.

Skipped: service process restart. Add it only if a future setting affects process-level state that cannot be reloaded in place.
