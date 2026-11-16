# Iframe security review

Scope: the engine-side iframe implementation (`engines/wega/src/frames.rs`, `view.rs`,
`dom.rs`, `dom_host.rs`, `dom/semantics.js`, `network.rs`, `adapters/blitz`, `adapters/v8`,
`services/security`). Evidence column names the test that demonstrates each guarantee
(`engines/wega/tests/iframes.rs` = **I**, `iframe_inspector.rs` = **N**, `frames.rs` = **F**,
`services/security` unit tests = **S**).

## 1. Findings made during the work

| # | Finding | Severity | Resolution |
|---|---|---|---|
| F-1 | The vendored Blitz already contained its own `<iframe>` loader. It fetched iframe URLs through Wega's `NetProvider` as *subresources* (`Destination::Image` classification, ambient cookies, no `frame-src`, no `X-Frame-Options`, no sandbox) and painted the result. The earlier "iframes are not implemented" message did not reflect that this path existed. A second, unaudited iframe implementation would have run next to the new one. | High | Disabled by configuration (`DocumentSetup::config` starts every document past Blitz's `MAX_SUBDOCUMENT_DEPTH`). Test I `embedding_policies_…` counts server requests: a `frame-src 'none'` page produces **zero** requests for the frame. There is now exactly one iframe path. |
| F-2 | `rusty_v8` enters an isolate when it is created and requires reverse-order disposal. Two realms on one thread left the *newest* isolate "current", so native callbacks of the older realm aborted the process (`Cannot create a handle without a HandleScope`). | High (process abort, reachable by any page with an iframe) | `V8Host` now exits the creation entry and re-enters only to dispose; all script steps already enter their own isolate. Covered by every multi-frame test; the crash reproduced with a single parser-created iframe before the fix. |
| F-3 | `PagePort::document_origin` reported a tuple origin from the document URL even for a sandboxed frame whose principal is opaque, which would let an inspector grant for a tuple origin authorise an opaque document. | Medium | Returns `None` for opaque frames, so only the explicit opaque override can authorise them. |

## 2. Guarantees demonstrated by tests

| Guarantee | Evidence |
|---|---|
| Parent and child have separate documents and separate JS realms (a global set in the child is invisible to the parent; parent `getElementById` does not see child nodes). | I `parser_created_same_origin_iframe_loads_…` |
| Sharing a content worker does not make frames same-origin: the cross-origin frame's facade throws `SecurityError` for `document`, `eval`, `localStorage`, and `contentDocument` is `null`. | I `cross_origin_frame_loads_but_its_window_is_a_restricted_facade`, F `frames_expose_only_host_backed_facades…` |
| Principal comes from the **final** URL after redirects, not the requested URL. | I `redirects_commit_the_final_principal` |
| `srcdoc`/`about:blank` inherit the creator's origin; a sandbox without `allow-same-origin` makes the origin opaque; opaque frames get no cookies (verified at the server), no `localStorage`/`sessionStorage`/`indexedDB`/`caches`/workers/`BroadcastChannel`/`navigator.serviceWorker`. | I `srcdoc_and_about_blank_…`, `opaque_frames_send_no_cookies_…` |
| A sandbox without `allow-scripts` creates **no realm** (evaluation fails, no handler can run). | I `srcdoc_and_about_blank_…` |
| Sandbox flags only accumulate restrictions down the tree; unenforced tokens never grant anything. | S/unit `sandbox_flags_only_ever_accumulate_restrictions` |
| `allow-forms`, `allow-popups`, `allow-modals` are enforced (no request on form submit, `window.open` → null, `confirm` → false); top navigation from a frame is always a no-op; engine internals (`__wega_call`, `Deno`) do not exist in the child. | I `sandbox_denies_forms_popups_modals_and_top_navigation` |
| `X-Frame-Options` (DENY, SAMEORIGIN, conflicting/unknown values fail closed) and CSP `frame-ancestors` (every ancestor checked, `'none'`) replace the response with a non-scriptable blocked page; parent CSP `frame-src` blocks before any request. | I `embedding_policies_block_frames…`, S `frame_policy_tests` |
| `postMessage` across frames: `targetOrigin` is matched against the **receiver's committed principal**; `event.origin` is computed by the host (the sender cannot choose it); `event.source` is a host-backed facade; transfers and uncloneable values fail with `DataCloneError`; messages to a removed frame vanish; opaque receivers match only `"*"`. | I `post_message_checks_target_origin_…`, `child_navigation_reload_and_stale_references`, unit `target_origin_is_matched…` |
| Frame identity is not document identity and is never reused; removal during a pending navigation drops the response; the parent is untouched. | I `script_created_frames_navigate_and_detach_independently`, `child_navigation_reload_and_stale_references` |
| Bounds: depth 8, 32 live frames per tab, 256 queued messages, 1 MiB per message; a document framing itself forever stays bounded; a 200-iframe flood stops at 32; an oversized `srcdoc` is refused. | I `frame_limits_bound_nesting_count_and_srcdoc_size` |
| A click inside a frame is consumed by the frame (the parent element behind it does not fire); keys go only to the focused frame; wheel scrolls the frame under the pointer and not the parent. | I `frames_composite_into_the_parent_and_receive_input…`, `keyboard_input_goes_only_to_the_focused_frame`, `wheel_scrolls_the_frame…` |
| DevTools: every frame is a separate inspector target with its own document stamp; node handles are refused across frames; a mutation grant on one frame does not authorise another; a frame navigation revokes handles and grants; a removed frame cannot be targeted. Page JS gets no extra authority. | N `each_frame_is_an_independent_inspector_target_with_its_own_authority` |

## 3. Security limitations (not hidden)

1. **No OS-process isolation per site.** All frames of a tab share the content worker process
   (which keeps its sandbox). Separation is isolate + committed principal + host-routed
   messaging. A renderer memory-safety bug or a speculative-execution attack in one frame
   could reach another frame's data. The protocol's `FrameTree` models `Placement::Isolated`
   for cross-origin frames; the engine does not yet honour it. `crossOriginIsolated` stays
   false; each realm has its own `SharedArrayBuffer` store.
2. **Not implemented, therefore absent rather than permissive:** same-origin synchronous
   DOM access (`contentDocument`, `frameElement`), Permissions Policy / `allow`, COOP/COEP/CORP,
   third-party cookie partitioning (the jar is per profile; cross-site frame navigations use the
   cross-site cookie context, so `SameSite=Strict`/`Lax` cookies are withheld, but
   `SameSite=None` and legacy cookies are sent), `csp` attribute, `credentialless`,
   `data:`/`blob:`/`javascript:` frame URLs (refused), `<object>/<embed>` frames.
3. **Unenforced sandbox tokens** (treated as *denied*): `allow-pointer-lock`,
   `allow-presentation`, `allow-downloads`, `allow-orientation-lock`,
   `allow-popups-to-escape-sandbox`, `allow-top-navigation*`, `allow-storage-access-by-user-activation`.
4. **Opaque-origin network identity:** requests of an opaque frame carry `Origin: null` and
   `credentials: omit`. Service-worker interception of an opaque frame's loads is not separately
   disabled (the realm cannot register one, but an existing controlling worker for the URL's
   scope can see the navigation). Treat as a gap.
5. **`srcdoc`/`about:blank` use the parent URL as their document URL** (so base URL and
   origin derive correctly). `document.URL` therefore differs from the specification
   (`about:srcdoc`).
6. **`event.source` is `null` for messages that climb more than one level** (`top.postMessage`
   from a grandchild). The origin is still host-computed.
7. **Meta-delivered `frame-ancestors`** is enforced (stricter than the specification, which
   ignores it); this fails closed.
8. **Cross-frame structured clone is JSON-compatible only** (`Map`, `Date` round-trip as
   JSON). Values that JSON cannot carry fail or degrade; nothing is shared by reference.
9. **Host/DevTools UI wiring:** the engine exposes the frame tree and per-frame inspector
   targets (`View::frame_tree`, `View::frame_target`), but the browser process does not yet
   issue per-frame session identities or show a frame selector, so a user cannot yet select a
   child frame in the native DevTools. The browser-side `FrameTree` in `src/wega_page.rs` is
   not fed by the engine's frames.
10. **Worker stall:** a child realm that hangs is bounded by the same per-realm script watchdog
    as the top realm (a fatal limit tears down only that child's realm), but a *native*
    layout hang inside a child shares the process with the parent, so the existing layout
    guard (depth 121) and the process-level kill are the protection.
