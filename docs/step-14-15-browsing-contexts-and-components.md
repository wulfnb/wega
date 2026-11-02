# Steps 14 and 15: browsing contexts and components

Updated 2 November 2026. Delivery branch: `feature/standalone-wega-step14`, PR #26.
Integrated baseline: `8de37835fe31b0b5817151e03cde0fe2fbc94568`.

**Both steps remain partial.** The original PR supplied synthetic iframe documents
and JavaScript component state, not native frame rendering or scoped Shadow DOM
layout. Those implementations did not meet the execution plan's acceptance gates.
This update fixes the unsafe exposure and establishes Rust ownership. Passing the
bounded regressions below does not establish browser parity or permit Servo removal.

## Delivered in this update

| Area | Implementation and boundary |
| --- | --- |
| Integration | Merged completed Step 05/06 sandbox, broker, Windows launch/metrics and document-host changes. Protocol v15 preserves v13/v14 inherited grants and recovery capabilities. |
| Frame coordination | `engines/protocol/src/browsing.rs`: host-assigned identities, bounded depth/population, provisional admission, process/partition validation, inherited and unique opaque origins, sandbox intersection, navigation request/commit separation, subtree revocation and document-bound message envelopes. This coordinator is not yet connected to the desktop supervisor. |
| Unsafe frame paths | Removed synthetic child documents, exposed `_frameContext`, reflective WindowProxy bypasses and JavaScript-interpolated frame commands. Reserved commands report unavailable; they do not execute text or claim a navigation commit. `SiteIsolationFrames` is not advertised. |
| Product fallback | Visible iframes again report compatibility degradation. A painted empty box cannot satisfy the native-frame gate. |
| Window messages | The current document can post messages to itself using V8/Deno structured clone and transfers, native URL/origin validation and task delivery. Clone errors propagate. This does not provide cross-frame messaging or MessagePort transport. |
| Custom elements | `engines/wega/src/components.rs` owns case-sensitive name validation, duplicate registration, frozen observed attributes, upgrade/failure states, bounded reactions and native node cleanup. Binding code retains JavaScript constructors/callbacks and invokes them; it does not own registration or upgrade admission. |
| Shadow APIs | Rust owns open/closed root associations, connected/root traversal, slot assignment/flattening and host-inclusive cycle rejection. No global strong set retains roots. |
| Events | Adapted the actual pinned Deno dispatcher for native shadow parents, assigned slots, uncomposed roots and closed paths. Every modified source anchor is checked at startup/build time. The unused fallback dispatcher is not acceptance evidence. |
| Visibility of limitations | Attaching a shadow root reports compatibility degradation because Blitz/Stylo scoped style and flattened layout remain absent. API support must not silently produce blank component content. |
| Validation wiring | The native three-platform workflow now runs for this PR branch as well as the integration branch. |

The separate `wega-step3` checkout and its uncommitted edits are preserved. Its
fragment-query changes are reviewed against the native selector implementation;
working-tree edits are not implicitly committed as part of this update.

## Step 14: work still required, in order

1. Connect `FrameTree` to the browser coordinator and supervisor. Derive principals
   from broker-observed commits, not renderer-provided URL or origin strings.
   Admit/revoke frame-specific network, storage, cookie, font and surface grants.
2. Create real same-origin realms/documents with independent globals, parser,
   lifecycle and history while preserving synchronous DOM/window access. Stable
   WindowProxy identity must survive navigation and reject every reflective bypass.
3. Admit cross-site workers under the required OS sandbox. Implement navigation
   redirects, inherited/opaque origins, sandbox/CSP/referrer/permissions policies,
   opener/noopener and popup lifetime. The coordinator's current exact pending URL
   check is deliberately restrictive; it is not a complete redirect controller.
4. Compose actual child surfaces with viewport clipping, scroll/zoom transforms,
   frame-credit budgets and stale-document rejection. Route hit testing, pointer,
   keyboard, IME, focus, accessibility and tool requests to the owning document.
5. Transport V8 structured-clone payloads, ArrayBuffer/MessagePort ownership and
   authenticated source/target contexts across agents. Enforce targetOrigin at
   delivery; revoke queued operations on replacement/crash/detach.
6. Prove those paths with real HTTP child pages and pixel/input assertions,
   synchronous same-origin interactions, forged IPC, reflective access attempts,
   frame crash/restart, deep-frame budgets and three-platform native confinement.
   Only then advertise `SiteIsolationFrames` and remove visible-frame fallback.

## Step 15: work still required, in order

1. Complete reaction integration for every parser/native mutation, construction
   edge case, adoption/document ownership and supported customized built-ins. Add
   relevant pinned upstream custom-element tests and constructor reentrancy cases.
2. Implement native shadow-root node metadata and Stylo `TShadowRoot`, containing
   root/host traversal and per-root author cascade data. Preserve document-tree
   selector boundaries; implement `:host`, `:host-context`, `::slotted`, inheritance
   and stylesheet ownership. Detached fragments alone cannot supply style scoping.
3. Build Blitz's flattened layout/paint tree from shadow children and assigned
   slots, with fallback content, dynamic reassignment and slotchange ordering.
   Preserve light DOM for DOM APIs and shadow-including traversal for lifecycle.
4. Integrate focus/delegatesFocus, selection, hit testing, native input and
   accessibility with retargeting and closed-root boundaries. No geometry or
   accessibility claim follows from synthetic event-dispatch tests alone.
5. Run representative component libraries and pinned upstream API/reftest subsets
   in direct Wega on all supported platforms. Remove shadow compatibility fallback
   only after real styled component pixels and interaction pass.

Step 15 also depends on Steps 08 and 13, and on real Step 14 contexts. Those
prerequisites are not marked complete by this PR.

## Verification record

Local macOS arm64 validation passed: 30 protocol, 17 engine unit, 9 custom-element,
27 DOM, 4 frame-boundary/message, 11 page and 17 worker tests (115 total).
Strict workspace Clippy across all targets, both formatting checks, browser-core
compilation, Actionlint and exact-patch verification of all 11 vendored crates passed.
The full workspace test attempt exceeded local disk capacity during parallel
linking; the relevant suites were then run serially. Full hosted regression and
native confinement runs remain separate pending checks for this delivery.


Current-head results are recorded in `docs/validation/standalone/progress.json`.
The earlier complete Windows/Linux/macOS acceptance at `efe5a87` remains evidence
for Steps 05/06 at that commit, not for these new frame/component changes.

The relevant commands are:

```sh
cargo test --locked --manifest-path engines/Cargo.toml -p wega-protocol
cargo test --locked --manifest-path engines/Cargo.toml -p wega-engine --lib --test custom_elements --test frames --test pages
cargo test --locked --workspace --manifest-path engines/Cargo.toml --all-targets
cargo clippy --locked --workspace --manifest-path engines/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path engines/Cargo.toml --all --check
```

The `frames` regressions check unbound-frame denial and current-document messages;
they do not count as real cross-frame browsing tests. The coordinator unit tests
exercise authority rules without claiming a live multiprocess renderer.
