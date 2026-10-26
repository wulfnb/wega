# DOM WPT subset

Unmodified Web Platform Tests sources pinned at `68d964a5f292bc4b2c2eba2f21e2b916190b6ad2`.
Upstream: https://github.com/web-platform-tests/wpt/tree/68d964a5f292bc4b2c2eba2f21e2b916190b6ad2/dom/nodes

The Rust View runner uses the actual upstream testharness.js, constants helper and
inline test programs. `cases.json` is a mechanical extraction: external reporting
scripts are omitted and script tags are removed from fixture HTML before the
original programs run. No assertions are rewritten or skipped. The entire four
selected files must pass; the baseline is 31 test cases (100% of this subset).
Tests involving iframes, shadow DOM, doctype and processing instructions are outside
this initial subset and are not claimed as passing. The subset may expand, and its
case count and pass rate may only increase. See UPSTREAM.json for file checksums.

License: upstream BSD-3-Clause, included as LICENSE.md.
