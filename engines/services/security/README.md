# services/security

**Status: IMPLEMENTED (Wave 1) — package `wega-security`**

## Purpose

Security policy enforcement.

## Intended responsibilities

- CSP, mixed content, certificate policy hooks
- Origin and site-isolation primitives

## Expected dependencies

`foundation/*`; platform and network crates in `engines/platform` and `engines/network` through narrow traits. Specifically: `foundation/*`.

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`.

## Current status

IMPLEMENTED. Member of the `engines` workspace. Contains the Content Security Policy implementation (moved from `engines/wega/src/csp.rs`; `Csp` made `pub`) and the pure `trustworthy()` check (moved from `security_context.rs`). V8 secure-context configuration stays in `wega-engine`; CORS stays in `wega-network`. The engine keeps `crate::csp::Csp` and `crate::security_context::trustworthy` as re-exports. See `docs/architecture/module-map.md`.
