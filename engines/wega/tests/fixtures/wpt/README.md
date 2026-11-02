# DOM WPT subset

Unmodified Web Platform Tests sources pinned at `68d964a5f292bc4b2c2eba2f21e2b916190b6ad2`.
Upstream: https://github.com/web-platform-tests/wpt/tree/68d964a5f292bc4b2c2eba2f21e2b916190b6ad2

The Rust View runner uses the actual upstream testharness.js, the upstream helper
scripts each file loads by `src` (recorded per case as `helpers`) and the inline
test programs. `cases.json` is a mechanical extraction: external reporting
scripts are omitted and script tags are removed from fixture HTML before the
original programs run. No assertions are rewritten or skipped. Every selected
file must pass entirely; the baseline is 35 files and 1685 test cases (100% of
this subset). Files come from `dom/nodes`, `dom/events`, `dom/collections` and
`html/browsers/history/the-location-interface`. Files that need iframes,
DOMImplementation-created or XML documents, ProcessingInstruction nodes or named
properties are outside the subset and listed as blockers in
`docs/step-08-dom-events-web-apis.md`. The subset may expand, and its case count
and pass rate may only increase. See UPSTREAM.json for file checksums.

License: upstream BSD-3-Clause, included as LICENSE.md.
