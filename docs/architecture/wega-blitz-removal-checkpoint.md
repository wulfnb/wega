# Blitz removal recovery checkpoint

- Starting/last verified commit: `8523150487a953394920e69459ad7f374e57297c`.
- Branch: `feature/dom-rounds-9-12`; initial worktree clean.
- Phase 0: inventory recorded before production edits; graph capture in progress.
- Active commands: graph/metadata capture; brokered public-page baseline capture
  (`WEGA_PUBLIC_COMPAT=1 WEGA_TEST_BROKER=1 WEGA_RENDER_BACKEND=native`).
- Evidence directory: `docs/architecture/baseline/blitz-removal-2026-11-17`;
  public baseline `/private/tmp/wega-blitz-before` and matching `.log`.
- Tests in this continuation: not yet complete. Prior totals are historical only.
- Remaining: diagnose public-page geometry; regression fixtures/fixes; design and
  implement real owner migration; runtime independence/security/Chrome/WPT;
  release measurements; safe removal; final full checks and reports.
- Do not push. Do not edit DevTools D4/inspector. Never delete Blitz before
  native-only runtime gates. Preserve budgets, CSP and exact 32-frame assertion.

## Owner storage milestone

- Implemented independent `wega-dom::Document`, consumed directly by the existing
  html5ever parser in tests; no runtime view constructs a second tree.
- Verified: `cargo test --manifest-path engines/Cargo.toml -p wega-dom -p
  wega-html --offline`: 19 passed (15 new owner/parser cases, 4 boundaries).
- Independent DOM/parser graph passes; actual browser graph fails independence
  with DOM/HTML/paint/traits/adapter still production dependencies. Recorded by
  `scripts/check-blitz-independence.py`; no runtime independence claim.
- Public layout implementation is still being validated in the worktree.
  Eight reduced Chrome fixtures; latest log `/private/tmp/wega-public-layout-strut.log`.
- Next: finish line-strut validation/public captures, checkpoint layout changes;
  remove native pipeline's bridge coupling; run full mandatory gates sequentially.

## Public layout milestone

- Verified eight new real-Chrome reduced fixtures with unchanged channel 8,
  pixel fraction 0.01 and geometry 0.5px gates, standards-mode identical HTML.
- Fixed indefinite percentage flex basis, overflow-dependent automatic minima,
  preferred-width cap, variable flex/font shorthand expansion, root rem metrics,
  border-box relative lengths, intrinsic inline-block width/baselines, padded
  flex sizing, nested replaced inline boxes and owning-block line struts.
- Mandatory gate `/private/tmp/wega-blitz-compat/gate.json` passed, including
  pinned 1685/1685 DOM WPT assertions; no fixture assertions/tolerances weakened.
- Fresh public captures still need refreshing for the final padding/image fixes;
  previous captures already show restored rustdoc 200px sidebar/600px main.
- Next: isolate native algorithms in an independent crate while retaining the
  authorized decoder in the resource bridge. Production owner integration remains.

## Independent algorithm milestone

- `wega-native` extracted from engine modules; production engine uses the extracted
  algorithms. Resource binding remains at the explicit existing bridge.
- Component graph is Blitz-free, including test/raster dependencies. Real owned
  HTML -> mutation -> native layout -> hit-test -> AnyRender/Skia pixels passes.
- Independent owner/parser/native tests: 25 passed, no ignored; strict component
  Clippy passes. Production SVG/canvas regression smoke passed, 6 tests.
- Owner fragment insertion/cycle checks and invalidation cleanup tests added.
- Release benchmark source added for independent 200-element/201-rule stages;
  release measurements not yet run. No speedup claim.
- New Skia component graph required its pinned prebuilt: restricted attempt failed
  DNS, normal approved network retry succeeded. No sandbox/browser policy changed.
- Disk recovery: 90 obsolete generated test executables (17,197,188,768 bytes)
  matched old dependency fingerprints, all checked not running, were removed;
  explicit manifest `/private/tmp/wega-obsolete-test-executables.json` preserved.
- Next: full sequential workspace/root/Clippy, native broker/security and signed
  worker, final Chrome/WPT and public captures, release benchmarks and reports.
- Production DOM/CSSOM/resource/control/VM/frame integration remains incomplete;
  production independence gate must fail until those owners migrate.
