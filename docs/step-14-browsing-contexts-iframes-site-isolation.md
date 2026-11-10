# Step 14 Contract: Browsing Contexts, IFrames, and Site Isolation

## 1. Overview & Objectives

Step 14 connects browsing context hierarchy and site isolation into the host supervisor and standalone Wega engine per the execution plan:
- Direct integration of `FrameTree` into the browser supervisor (`Snapshot` in `src/wega_page.rs`), deriving principal identity from host-observed commits rather than untrusted renderer strings.
- Bounded browsing context tree with host-enforced admission limits (`MAX_CONTEXTS = 128`), maximum nesting depth (`MAX_DEPTH = 16`), and structured message byte caps (`MAX_MESSAGE_BYTES = 1 MB`).
- Cross-origin property access enforcement restricting cross-origin Window access exclusively to permitted properties (`postMessage`, `location`, `parent`, `top`, `window`, `frames`, `closed`, `opener`) and blocking sensitive properties (`document`, `eval`, `cookie`, `localStorage`).
- Cross-document messaging via `Command::FramePostMessage` and `Event::FramePostMessage` routing with authenticated origins, strict `targetOrigin` matching (wildcard `*`, `/`, or explicit URI origin), and structured clone transfer support.
- Frame lifecycle coordination (`Event::FrameAttached`, `Event::FrameDetached`, `Event::FrameNavigated`, `Command::FrameNavigate`) with automatic descendant revocation and queued message invalidation upon detachment or navigation commit.
- Advertise `Capability::SiteIsolationFrames` in protocol v15 capabilities negotiation.

## 2. Architecture & Protocol Additions

### Protocol Layer (`engines/protocol/src/browsing.rs`, `lib.rs`, `authority.rs`)
- `Capability::SiteIsolationFrames` added to protocol capabilities at version 15.
- `FrameTree` coordinating contexts, placements (`SameAgent` vs `Isolated`), sandbox inheritance, opaque origin derivation (`about:srcdoc` / sandboxed), and message acceptance validation.
- `FrameAuthority::session(&self)` exposed for supervisor session identification.
- Standard cross-origin property policy verified (`is_permitted_cross_origin_property`).

### Browser Supervisor Integration (`src/wega_page.rs`)
- `Snapshot` holds active `frame_tree: Option<protocol::browsing::FrameTree>` initialized on document commit and principal binding with top-level context and URL.
- Reader loop safely handles frame lifecycle events:
  - `Event::FrameAttached`: validates parent identity, computes inherited sandbox policy, and attaches frame.
  - `Event::FrameDetached`: detaches frame context and prunes all descendant frames.
  - `Event::FrameNavigated`: validates navigation commit against pending state and advances child revision.
  - `Event::FramePostMessage`: authenticates source and target contexts, validates targetOrigin, and dispatches `Request::FramePostMessage`.

### Engine View & Worker (`engines/wega/src/view.rs`, `main.rs`, `dom/semantics.js`)
- `PageEvent` extended with `FrameAttached`, `FrameDetached`, `FrameNavigated`, `FramePostMessage` and wired in `main.rs` to top-level `Event` messages.
- `View::post_frame_message(target, origin, target_origin, message)` implemented to dispatch `MessageEvent` onto `window` with authenticated origin and data, respecting `targetOrigin` constraints.
- `globalThis.MessageEvent` exposed and configured on window prototype.
- Worker loop handles `Command::FramePostMessage` and `Command::FrameNavigate`.

## 3. Verification & Evidence

- Protocol test suite (`cargo test -p wega-protocol --manifest-path engines/Cargo.toml`):
  - `browsing::tests::identity_admission_and_opaque_origins_are_not_renderer_strings` — PASSED
  - `browsing::tests::provisional_contexts_and_foreign_partitions_do_not_grant_access` — PASSED
  - `browsing::tests::requested_navigation_never_commits_and_detach_revokes_descendants` — PASSED
  - `browsing::tests::messages_authenticate_both_documents_and_normalize_target_origin` — PASSED
  - `browsing::tests::sandbox_inheritance_cannot_relax_ancestors_or_enable_top_navigation` — PASSED
  - `browsing::tests::frame_depth_and_population_have_explicit_bounds` — PASSED
  - `tests::site_isolation_frames_capability_and_origin_policy` — PASSED
  - Total: 33/33 protocol tests passing.
- Engine Frame test suite (`cargo test --test frames -p wega-engine --manifest-path engines/Cargo.toml`):
  - `frame_tree_and_origin_policy_isolation` — PASSED
  - `post_frame_message_dispatches_message_event_with_origin_and_target_filtering` — PASSED
  - `unbound_frames_never_expose_synthetic_documents_or_window_proxies` — PASSED
  - `frame_attributes_remain_native_dom_attributes_without_creating_a_context` — PASSED
  - `window_messages_use_tasks_structured_clone_and_native_transfer` — PASSED
  - `failed_or_duplicate_transfers_leave_the_callers_buffers_attached` — PASSED
  - Total: 6/6 frame tests passing.
- Custom Elements & Page suites:
  - `tests/custom_elements.rs`: 9/9 tests passing.
  - `tests/pages.rs`: 13/13 tests passing.
- Host compilation:
  - `cargo check --bin wega`: compiled cleanly with zero errors.
