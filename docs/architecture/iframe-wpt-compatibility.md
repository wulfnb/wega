# Iframe WPT compatibility matrix

**Result: no upstream iframe WPT file was executed in this milestone.** This document does not
claim any WPT pass rate for iframes, and it does not claim standards compliance.

## Why nothing was run

* The repository has no WPT checkout. The only WPT material is the pinned DOM subset in
  `engines/wega/tests/fixtures/wpt` (35 files, 1685 cases, from `dom/nodes`, `dom/events`,
  `dom/collections` and one `history` file). Its README states that files needing iframes are
  outside the subset.
* The WPT runner (`engines/wega/tests/…` harness) runs one document per `View`. It has no
  `<iframe>`-hosted test support, no `testdriver`, no `promise_test`/`step_timeout` support and no
  cross-window harness messaging (`window.opener`/`parent` reporting), all of which the
  `html/browsers/browsing-the-web`, `html/browsers/sandboxing`, `html/cross-origin-*`,
  `html/semantics/embedded-content/the-iframe-element` and `html/browsers/windows` suites rely on.
* Fetching upstream WPT at the pinned hash needs network access to
  `github.com/web-platform-tests/wpt`, which was not part of this task's authorised actions.

Running those suites is blocked on S1 in `post-round12-roadmap.md` (a WPT checkout, an
expectations manifest and a harness with `promise_test`, `step_timeout`, reftests and
cross-frame reporting). Until then the classification below is a **plan, not a result**.

## Area-by-area status

| Area | Upstream location | Status | Reason / what is needed |
|---|---|---|---|
| HTML iframe element (attributes, `src`, `srcdoc`, `name`) | `html/semantics/embedded-content/the-iframe-element` | Skipped (not run) | runner lacks iframe-hosted tests; local equivalents pass (see below) |
| `load` event ordering for frames | same | Skipped (not run) | same; local test `script_created_frames_…` checks one `load` per navigation |
| Browsing contexts / navigation | `html/browsers/browsing-the-web`, `html/browsers/windows` | Skipped; **unsupported** parts: joint session history for frame navigations, `window.open` targets by name, `target=` navigation of named frames | no joint session history (design B-4) |
| Same-origin policy / WindowProxy | `html/browsers/origin`, `html/cross-origin-*`, `html/browsers/the-window-object` | Skipped; **unsupported**: same-origin synchronous DOM access, `frameElement`, `window.frames === window`, full cross-origin property table (`[[GetOwnProperty]]` semantics) | B-1: no cross-isolate DOM bridge. The facade exposes only a safe subset and is a `Proxy`, not an exotic WindowProxy |
| postMessage | `html/webappapis/…/postmessage`, `webmessaging` | Skipped; partial: JSON-compatible cloning only, no transfer lists across frames, `event.source` null beyond one hop, no `MessagePort`s via `postMessage` | local tests cover origin checks |
| Sandbox | `html/semantics/embedded-content/the-iframe-element/sandbox-*`, `html/browsers/sandboxing` | Skipped; enforced flags: scripts, same-origin, forms, popups, modals (top navigation always denied); **unsupported**: pointer-lock, presentation, downloads, orientation-lock, storage-access, popups-to-escape-sandbox | unsupported tokens stay denied |
| Nested frame rendering | reftests under `html/rendering/replaced-elements` | Skipped | no reftest runner for frames; local pixel tests cover clipping/compositing |
| CSP `frame-src`/`frame-ancestors`, `X-Frame-Options` | `content-security-policy/frame-src`, `frame-ancestors`, `x-frame-options` | Skipped | harness needs header-controlled server (`.sub.headers`), unsupported by the runner |

## Local equivalents (loopback fixtures, not WPT)

`engines/wega/tests/iframes.rs` (16 tests), `iframe_inspector.rs` (1), `frames.rs` (6) and
`services/security` frame-policy unit tests (3) exercise the behaviours above. They are written
for Wega and are **not** a substitute for the upstream suites.

## Existing WPT gates

The pinned DOM subset (35 files / 1685 cases) and CSS-WPT/pixel-reftest gates are part of the
regression run; see `iframe-implementation-report.md` for their results.
