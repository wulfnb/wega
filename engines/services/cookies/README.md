# services/cookies

**Status: IMPLEMENTED (Wave 1) — package `wega-cookies`**

## Purpose

Cookie jar and cookie policy.

## Intended responsibilities

- Storage, SameSite and partitioning rules
- Cookie access API

## Expected dependencies

`foundation/*`; platform and network crates in `engines/platform` and `engines/network` through narrow traits. Specifically: `foundation/*`.

## Dependencies to avoid

`dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`.

## Current status

IMPLEMENTED. Member of the `engines` workspace. Extracted verbatim from `engines/network/src/cookies.rs` (plus `public_suffix_list.dat`); `wega-network` re-exports `Cookies`, `CookieEntry` and `SiteContext`, so existing `wega_network::…` imports keep working. See `docs/architecture/module-map.md`.
