# trak.in and new-tab fixes — 27 October 2026

These fixes stay in the same working tree as native containers for a single PR.

## New-tab crash

`Browser::redraw` temporarily takes the GUI out of `Browser` while widgets run.
The sidebar/top-strip New tab button calls `new_tab` during that interval, and
`focus_content` previously unwrapped the absent GUI. Keyboard and older synthetic
UI tests retained the GUI and therefore missed the crash.

Focus now clears immediately when the GUI is available, or records a request
that redraw consumes after `gui.run`. The page receives keyboard focus after the
address editor releases it. The new-tab control also exposes an accessible button
name. A native regression exercises both tab layouts with the GUI taken out,
checks that the new internal page is loaded, and verifies that the source Wega
worker and tab survive. It reproduces the old unwrap panic and passes after the
fix, together with the existing smoke and dual-engine suites.

## trak.in compatibility and the stale app bundle

The existing local `target/Wega.app` was built on 25 October. Its 87,920,800-byte
worker still contained QuickJS and the exact reported text, `JavaScript failed
or exceeded its limit`. Current source uses V8, so that bundle could not serve as
a test of the current engine. The macOS bundle has now been rebuilt with the
current browser and worker. Running instances must be quit and reopened to load
the replacement executable.

Live testing of the current engine found additional real DOM gaps used by
trak.in and its scripts. The bindings now provide:

- Active-document `Document.location`, using the same navigation path as
  `window.location`, and null for detached documents.
- `Document.getElementsByName`, a live, case-sensitive NodeList of HTML elements
  in tree order, excluding SVG elements.
- The legacy `HTMLDocument` alias and a Window prototype in the global object's
  existing EventTarget/named-property chain. Window cannot be constructed, and
  prototype extensions are visible on the actual global object.

These follow the HTML standard's [document location](https://html.spec.whatwg.org/multipage/nav-history-apis.html#dom-document-location)
and [named element collection](https://html.spec.whatwg.org/multipage/dom.html#dom-document-getelementsbyname)
contracts. An offline test uses trak.in's unmodified, pinned jQuery 3.6.3 browser
bundle in a fresh realm and checks actual DOM creation/insertion/querying. Other
tests cover location identity/fragment navigation, detached documents, live name
collections, Window prototype extensions, event inheritance and illegal construction.

## Validation and limits

The engine workspace checkpoint passes 112 tests (one upstream generated doctest
is ignored). After the final Window/fresh-realm additions, 30 DOM, framework,
script, worker and pinned WPT regression tests pass. Native smoke and engine
suites pass, including the regression which failed before the GUI fix. Desktop
strict Clippy, formatting and workflow checks pass. The rebuilt macOS bundle
passes deep, strict signature verification; it remains an ad-hoc signed debug
package, not a notarized release.

The final live trak.in retest no longer reports the createElement, Document.location
or missing Window/HTMLDocument errors. It renders its document and title, but
advertising/push SDKs still produce an exception and the asynchronous script loop
can reach its execution limit. This ends that page's script context. The safety
limits have not been raised or disabled, and this is not a full functional site
pass. Servo fallback remains available. Windows/Linux runtime and remote CI for
these uncommitted fixes remain unrun.

The [machine-readable record](validation/trak-new-tab-2026-10-27.json) records
before/after evidence, source and binary fingerprints, logs and the remaining
site limitation. Raw artifacts are under `target/trak-debug/`.
