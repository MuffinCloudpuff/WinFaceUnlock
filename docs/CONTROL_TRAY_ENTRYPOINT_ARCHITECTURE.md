# Control Tray Entrypoint Architecture

## Intent

WinFaceUnlock needs a user-visible entrypoint after installation. The Windows
Service continues to own authentication, presence lock, camera policy, runtime
state, and privileged operations. The control frontend is only a visual settings
and enrollment client.

The tray entrypoint must therefore be a lightweight user-session process, not a
hidden Tauri/WebView window.

## Process Model

```text
win_service.exe
  Long-running Windows Service. It runs without the control frontend.

control_tray.exe
  Lightweight per-user tray process. It starts at user logon and stays resident.
  It does not host WebView, face models, or camera access.

WinFaceUnlock.exe
  Tauri control panel. It is launched on demand from the tray and exits when the
  user closes the window.
```

Closing the control panel must not stop the tray process or the service:

```text
user closes WinFaceUnlock.exe
=> control panel process exits
=> control_tray.exe remains in the tray
=> win_service.exe continues running
```

Opening from tray:

```text
user clicks tray icon or "Open control panel"
=> control_tray.exe launches installed WinFaceUnlock.exe
=> control panel talks to win_service.exe through the existing control protocol
```

## Ownership Boundaries

`win_service`
  Backend capability core. It must not show UI or own tray behavior.

`control_tray`
  User-session launcher. It owns tray icon, tray menu, and starting the control
  panel process. It may read lightweight status in a later phase, but it must
  not duplicate settings logic or camera logic.

`apps/control-tauri`
  Visual control panel. It owns settings UI, enrollment UI, and protocol client
  mapping. It should exit normally when the window closes.

`installer_cli`
  Installs the tray executable and registers the per-user auto-start entry.

## First Version Behavior

Tray process:

```text
- starts at user logon
- creates a WinFaceUnlock tray icon
- left click opens the control panel
- right click menu:
  - Open control panel
  - Exit tray
- exits only when the user chooses Exit tray or the process is terminated
```

Control panel:

```text
- starts on demand
- if closed, exits
- does not remain hidden in the tray
```

Service:

```text
- unaffected by control panel close
- unaffected by tray close
```

## Startup Registration

Use a per-user startup entry:

```text
HKCU\Software\Microsoft\Windows\CurrentVersion\Run
  WinFaceUnlockTray = "<install_dir>\control_tray.exe"
```

This is intentionally not an elevated Windows Service concern. The tray belongs
to the interactive user desktop.

Uninstall must remove this value. Repair/install may overwrite it with the
current install path.

## First Version Non-Goals

```text
- no WebView hosted by the tray process
- no camera access from the tray process
- no credential access from the tray process
- no duplicate frontend settings implementation in tray
- no service-to-desktop UI popup
```

## Future Extensions

The tray can later gain lightweight status and quick toggles, still through the
stable control protocol:

```text
- show service running/not running
- show presence lock enabled/disabled
- quick toggle presence lock
- open logs folder
- restart service action gated by elevation
```

Those additions should be implemented as protocol commands or installer/control
operations, not by moving backend state into the tray.

## Acceptance Criteria

1. Installing WinFaceUnlock stages `control_tray.exe`.
2. Installing or repairing registers the HKCU Run entry for the current user.
3. Running `control_tray.exe` shows a tray icon without launching the control
   panel automatically.
4. Clicking the tray entry launches `WinFaceUnlock.exe`.
5. Closing `WinFaceUnlock.exe` exits the control panel process while the tray
   process remains.
6. Exiting the tray does not stop `win_service.exe`.
