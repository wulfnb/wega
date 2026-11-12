# services/permissions

**Status: IMPLEMENTED (Wave 1) — package `wega-permissions`**

## Purpose

Permission model and prompts.

## Intended responsibilities

- Permission state storage and queries
- Prompt requests surfaced to the UI layer

## Expected dependencies

`foundation/*`; platform and network crates in `engines/platform` and `engines/network` through narrow traits. Specifically: `services/security`.

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`. Also: Implementation of the permission UI.

## Current status

IMPLEMENTED. Member of the `engines` workspace. Moved from `engines/wega/src/permissions/{registry,device}.rs`. It depends on `wega-network::Policy` (the registry reads `blocked_hosts`) and `wega-protocol`; this temporary coupling is intentional and no new policy abstraction was introduced. The engine re-exports the previous `permissions::*` paths. One behaviour-preserving `if let` chain was collapsed to satisfy `clippy::collapsible_if`, which the engine crate had allowed crate-wide. See `docs/architecture/module-map.md`.
