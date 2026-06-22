# Control Face Profile Display Plan

## Purpose

The control frontend needs a human-friendly face profile display. The current
surface can expose backend identifiers such as `dev-user` and uses a generic
icon for every enrolled face. That is acceptable for development diagnostics,
but it is not the product contract.

This document defines the boundary between backend-owned identity/template
state and frontend-owned rendering.

## Decision

Runtime control responses should distinguish these concepts:

```text
user_id
  Stable backend identity key used for credential binding and face templates.

display_name
  Human-facing label for the account or face profile.

avatar_preview
  Optional non-secret display image selected by the backend from enrollment
  artifacts.
```

The frontend must not display `user_id` as the primary label when a display
name is available. The frontend also must not choose a face sample by parsing
template internals. It can only render a backend-provided preview image and
fall back to a neutral icon when no preview exists.

## Protocol Shape

Account profile responses may include:

```json
{
  "windows_account_username": "Leo16",
  "display_name": "用户1",
  "user_id": "dev-user",
  "user_sid": "S-1-5-21-winfaceunlock-pending",
  "account_type": "local",
  "credential_ref": "windows-credential-dev-user",
  "credential_secret_state": "configured"
}
```

Face template summaries may include:

```json
{
  "face_template_ref": "active-service-template",
  "user_id": "dev-user",
  "display_name": "用户1",
  "avatar_preview": {
    "mime_type": "image/jpeg",
    "image_base64": "...",
    "updated_at_unix_ms": 1782000000500
  },
  "template_kind": "selected_template_set",
  "recognition_model": {
    "model_family": "opencv_sface",
    "model_version": "2021dec"
  },
  "selected_template_count": 5,
  "source_state": "active_service_template"
}
```

`avatar_preview` is intentionally optional. Old templates, broken avatar files,
or privacy-hardened deployments can omit it without breaking the UI.

## Avatar Source

The selected template JSON currently contains embedding and quality metadata,
not a display-safe image. Enrollment writes a bounded `preview_frame.jpg` in the
same output directory as `selected_templates.json`; the first implementation
uses that file as an optional avatar preview artifact. This is a stable
contract-level fallback, not the final best-sample selector.

The durable backend path should be:

1. During `finish_face_enrollment`, select the best frontal accepted sample.
2. Generate a small avatar artifact from an aligned crop or validated preview
   crop.
3. Store it beside the selected template set, for example:

```text
face-enrollment/
  selected_templates.json
  avatar_preview.jpg
```

4. Add the artifact reference or embedded preview metadata to the template
   summary returned by `list_face_templates`.
5. Keep full frames and raw enrollment samples out of the control protocol.

The current implementation reads `preview_frame.jpg` beside the active selected
template set and embeds it as `avatar_preview` when available. A later pass
should replace that source with a best frontal accepted sample selected from
guided enrollment metadata and aligned crops.

## Failure Handling

If an avatar file is missing, unreadable, too large, or has an unsupported MIME
type, `list_face_templates` should still return the template summary without
`avatar_preview`.

Display name fallback order:

1. `display_name`
2. Windows account username when rendering the account profile
3. localized default `用户1`

The UI should not fall back to `user_id` for the visible primary label except
in diagnostics surfaces.

## Testing

Phase 1 checks:

```powershell
cargo test -p control_protocol
npm --prefix apps/control-tauri run build
```

Phase 2 avatar artifact checks:

```powershell
cargo test -p control_status
cargo test -p control_backend
cargo test --workspace
```

Add a fixture with `preview_frame.jpg` beside a selected template file and a
fixture without it. Both should load successfully; only the former should
return `avatar_preview`.
