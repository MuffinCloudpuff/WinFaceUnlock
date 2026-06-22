# Multi Face Profile Plan

## Purpose

WinFaceUnlock currently treats the control surface as having one active face
identity for the configured Windows unlock credential. Guided enrollment may
collect multiple samples and poses, but those samples belong to one backend
`user_id` such as `dev-user`.

This document records the planned path for supporting multiple enrolled face
profiles without confusing that feature with multi-account Windows login.

## Scope Decision

Phase 1 should support:

```text
one Windows credential
-> multiple enabled face profiles
-> any matching enabled profile can authorize unlock
```

Phase 1 should not support:

```text
multiple Windows credentials
-> face A unlocks account A
-> face B unlocks account B
```

Multi-account unlock is a larger credential-provider and credential-store
feature. It should build on the multi-face profile model later, not be mixed
into the first implementation.

## Concepts

```text
credential_user_id
  Stable backend identity for the Windows credential binding.

face_profile_id
  Stable backend identity for one enrolled person's face profile.

display_name
  Human-facing label such as "用户1" or "用户2".

avatar_preview
  Non-secret display image selected by the backend from that profile's
  enrollment artifacts.

enabled
  Whether this face profile participates in unlock matching.
```

The frontend may render profile cards and submit user intent, but it must not
parse recognition templates or decide matching policy. Matching, profile
selection, and authorization remain backend-owned.

## Storage Shape

Move from a single active template file toward a profile library:

```text
face-library/
  profiles.json
  profile-user-1/
    selected_templates.json
    preview_frame.jpg
  profile-user-2/
    selected_templates.json
    preview_frame.jpg
```

`profiles.json` should be the stable index:

```json
{
  "credential_user_id": "dev-user",
  "profiles": [
    {
      "face_profile_id": "face-profile-user-1",
      "display_name": "用户1",
      "enabled": true,
      "template_path": "profile-user-1/selected_templates.json",
      "avatar_preview_path": "profile-user-1/preview_frame.jpg",
      "created_at_unix_ms": 1782000000000,
      "updated_at_unix_ms": 1782000000000
    }
  ]
}
```

Existing single-template installs can be migrated by creating one profile named
`用户1` that points at the current configured `selected_templates.json`.

## Matching Policy

At unlock time, matching should not require every profile to pass. The intended
policy is:

```text
capture frame
-> detect and align face
-> extract embedding
-> iterate enabled face profiles
-> compare against each profile's selected templates
-> choose the highest scoring match
-> if best score passes threshold and liveness passes, issue grant
```

The result should include the matched profile:

```json
{
  "matched_face_profile_id": "face-profile-user-1",
  "matched_display_name": "用户1",
  "match_score": 0.82
}
```

This gives the UI and logs enough context to distinguish "unlock succeeded"
from "which enrolled face authorized it" without exposing template material.

## Frontend Behavior

The face management view should become a list of profile cards:

```text
用户1  avatar  enabled
用户2  avatar  enabled

[添加人脸]
```

Expected operations:

- Add a face profile.
- Rename a face profile display name.
- Enable or disable a face profile.
- Delete a face profile.
- Show each profile's backend-provided avatar preview, falling back to the
  neutral icon.

The frontend should not present `user_id` or `face_profile_id` as the primary
display label.

## Backend API Shape

Control protocol additions should be explicit profile operations:

```text
list_face_profiles
start_face_profile_enrollment
finish_face_profile_enrollment
rename_face_profile
set_face_profile_enabled
delete_face_profile
```

Template summaries should evolve from a single active face template summary to
a profile-aware list. The compatibility layer may continue exposing the first
enabled profile as the active summary during migration, but new UI should use
the profile API.

## Deep Challenges

### Credential Binding

The first implementation must keep `credential_user_id` separate from
`face_profile_id`. A face profile authorizes access to the configured
credential; it is not itself a Windows account.

Mitigation: make the profile library root carry `credential_user_id`, and make
each profile carry only `face_profile_id`.

### Threshold and False Accept Risk

More enabled profiles means more comparisons per frame. The best-match policy
slightly increases the chance that one template passes by coincidence.

Mitigation: keep the threshold centralized, record best-match score and profile
id, and later support per-profile calibration if needed.

### Migration and Rollback

Existing installs already point the service registry at a single
`selected_templates.json`.

Mitigation: migration should be additive. Keep reading the current single file,
create a profile index beside it, and only switch service auth to the library
format after the reader supports both.

## Roadmap

### Phase 1: Multi Face Profiles For One Credential

1. Add `FaceProfileSummary` protocol types.
2. Add profile library index read/write in backend-owned storage code.
3. Migrate the current active template into `用户1`.
4. Update guided enrollment to create a new `face_profile_id`.
5. Update auth matching to iterate all enabled profiles and return the best
   passing profile.
6. Update control frontend to render multiple profile cards.

### Phase 2: Multi Account Unlock

1. Link each `face_profile_id` to a `credential_ref`.
2. Update Credential Provider behavior for account-specific credential
   material.
3. Add account selection and audit semantics.
4. Add stricter recovery, deletion, and administrative controls.

Phase 2 should not start until Phase 1 has stable profile storage, matching
telemetry, deletion behavior, and migration tests.
