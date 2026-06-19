# Control Frontend Decoupling Plan

## Purpose

The runtime control frontend must be replaceable. React components should be a
UI shell, not the owner of WinFaceUnlock protocol details, Tauri command names,
template paths, registry keys, pipe names, or backend orchestration rules.

This document defines the boundary for moving the current React/Tauri control
integration into reusable layers before merging new visual UI work.

## Current Problem

The current Tauri React app already calls real backend operations, but those
calls live under `apps/control-tauri/src`. If that folder is replaced by a new
React UI, the typed protocol adapter and runtime orchestration can be lost.

The new `winfaceunlock/` folder is useful as a visual/state reference, but it
currently contains mock behavior:

1. simulated enrollment instructions
2. simulated success and failure buttons
3. direct browser camera access
4. local fake face-template state
5. local fake credential state

Those pieces must not replace the real backend integration.

## Target Layers

```text
crates/control_protocol
  Rust source of truth for runtime-control protocol

packages/control-client
  TypeScript protocol types and backend operation client
  no React dependency
  no Tauri dependency

packages/control-tauri-transport
  Tauri invoke transport for the TypeScript control client
  no React dependency

apps/control-tauri/src/bindings
  React hooks and view-model adapters
  maps backend state to UI state

apps/control-tauri/src/components
  visual components
  replaceable React shell
```

## Dependency Direction

Allowed:

```text
components -> bindings -> packages/control-client -> transport interface
bindings -> packages/control-tauri-transport
packages/control-tauri-transport -> @tauri-apps/api
```

Forbidden:

```text
components -> @tauri-apps/api
components -> backend command names
components -> template files / registry / pipe names
packages/control-client -> React
packages/control-client -> Tauri
packages/control-client -> filesystem or registry details
```

## Runtime Contracts

The shared TypeScript client owns:

1. request/response envelope types
2. operation names
3. typed payload and safe-details types
4. credential side-channel operation shape
5. face enrollment operation sequence primitives

The React bindings own:

1. enrollment UI state mapping
2. session-scoped status tracking after the user starts enrollment
3. automatic finish after backend completion
4. local UI events such as face-template-list refresh notifications

The visual components own:

1. layout
2. animation
3. buttons and labels
4. display of current instruction, progress, success, failure, and cancellation

## Replaceability Rules

When replacing the React UI:

1. keep `packages/control-client`
2. keep `packages/control-tauri-transport`
3. keep or reimplement `apps/control-tauri/src/bindings`
4. do not overwrite backend protocol types with mock API code
5. do not use direct `getUserMedia` for the production enrollment path

If the frontend becomes WinUI later, the shared TypeScript client is not the
primary reuse layer. WinUI should reuse the Rust/JSON runtime-control protocol
and backend handler, then implement its own C# transport/client.

## First Implementation Scope

1. Move `apps/control-tauri/src/controlProtocol.ts` into
   `packages/control-client` and `packages/control-tauri-transport`.
2. Update the Tauri React app to import the shared client through bindings.
3. Remove direct protocol/Tauri imports from the visual components.
4. Keep the existing visual surface functionally unchanged while moving the
   boundary.
5. After this boundary is stable, merge the new enrollment success/failure and
   pose-instruction UI as a pure UI/view-model change.

## Acceptance Criteria

1. `apps/control-tauri/src/components` does not import `@tauri-apps/api`.
2. `apps/control-tauri/src/components` does not import protocol operation
   functions directly.
3. Shared operation functions live outside `apps/control-tauri/src`.
4. Tauri invoke code lives outside `apps/control-tauri/src/components`.
5. Existing backend behavior remains verified by lint/build/tests.
