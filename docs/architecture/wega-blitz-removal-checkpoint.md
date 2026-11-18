# Blitz removal recovery checkpoint

## Continuation from `2d067b2` (2026-11-18)

- Verified clean `feature/dom-rounds-9-12` at the requested starting commit.
- M1 in progress: production blank/one-shot HTML and XML constructors now use
  Wega's generic mutation sink. XML preserves the existing xml5ever namespace,
  case, entity and recovery behavior, and now enforces the navigation node budget.
  XML scripts are still scheduled after construction. No mirror tree exists.
- `blitz-html` moved to comparison-test dependencies. The adapter boundary now
  explicitly forbids its production import; the parser remains independent of
  adapters/engine. Original oracle cases remain, with XML/noscript cases added.
- Validation: DOM/parser/reference-adapter suite 111 passed, 2 existing ignored;
  production brokered lifecycle/documents/identity/scripts 24 passed. Logs and
  current production graph in `baseline/blitz-removal-2026-11-18/`.
- Production graph still fails independence with `blitz-dom`, `blitz-paint`,
  `blitz-traits`, `wega-adapter-blitz`. The production document owner has not yet
  switched; M1 is not complete and M2-M5 remain outstanding. Native-only runtime
  validation must not be inferred from these results.
- Continue with tree-only V8 host operations against `DocumentView`, then owned
  resource/control/CSSOM integration. No push or inspector/D4 edits authorized.

## Historical campaign checkpoints

### Continuation tree-host checkpoint

- Tree-only V8 reads, fragment serialization, form association, component/slot
  traversal and detached-node retention now use `DocumentView`/`DocumentMutation`
  without concrete node-data matching. Shared algorithms run on `wega-dom::Document`.
- Added detached-document creation and counted comment-data mutation to the
  existing mutation contract and both owners. Comment writes no longer bypass
  the authoritative mutation clock; identity and no-op behavior are exercised.
- Verified runtime DOM/identity/forms/custom elements: 51 passed. DOM/parser/
  reference-adapter suite: 115 passed, 2 existing ignored. Deep content traversal
  exercises 10,000 levels on 128 KiB without changing accepted browser depth.
- This is a safe M1/M2 integration slice, not completion of either milestone.
  Editor, CSSOM, query, resources, frame and production owner migration remain.

### Continuation V8 identity checkpoint

- V8 node arguments, wrapper retention, activation/script requests, started-script
  sets, pending script queues and applied-meta bookkeeping now use `NodeKey`.
  Conversion to reference IDs occurs only at remaining concrete reference calls.
- Removed concrete traversal from V8 host and view node counting; generic
  pre-order iteration preserves document order. Script/meta reads use NodeView.
- Verified 87 production brokered DOM/identity/form/custom-element/event/frame/
  script regressions, then 20 lifecycle/document/script checks on the final
  lifecycle edits. No callback borrows or security boundaries were broadened.
- Initial runtime rebuild failed with linker ENOSPC. Inventoried 86 obsolete
  generated test executables with old dependency signatures, confirmed no active
  processes/open files, removed 17,241,038,848 bytes and reran successfully.
  Manifest and failure/retry logs are archived; sources and current executables
  were preserved. Full production DOM ownership still has not switched.

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

## Full validation in progress

- Last verified implementation/local checkpoint: `0aa6412` (native extraction).
- Active orchestration: `python3 /private/tmp/wega-blitz-run-validation.py`.
  Durable command/results/log directory: `/private/tmp/wega-blitz-final-validation`;
  runner progress `/private/tmp/wega-blitz-validation-runner.log`.
- Source frozen for this run. The orchestrator runs all required checks sequentially;
  `active.json` records the active command and `results.json` records completed
  commands/counts/hash/exit. Resume the runner for the same implementation rather
  than discarding changes or restarting already successful checks.
- Remaining final work: investigate actual failed checks, review fresh public
  Chrome/native captures, archive evidence and release metrics, write required
  final reports/evidence, final documentation commit. Runtime owner integration
  remains incomplete; do not delete the reference backend/vendor sources.

## Final public review correction

- Full run on `0aa6412`: engine 878/root 233/security 57, both signed sandbox
  checks, strict Clippy/fmt, 46 Chrome/WPT tests, release measurements passed.
  The production independence gate failed with the expected five dependency names.
- Fresh paired capture review exposed example.com's max-width/auto-width body
  centering. A new Chrome reduction reproduced 150px displacement before the fix.
  Final-width auto margin distribution now passes all nine Chrome reductions plus
  five existing layout geometry tests, with original tolerances.
- Previous complete run retained at `/private/tmp/wega-blitz-validation-0aa6412`.
  The latest source needs its own final validation; do not reuse the old head as
  the final implementation result. Reports are drafts pending that final run.

## Final verified checkpoint

- Frozen tested implementation: `d4d209834ad88db268787010d0670905fced1d08`.
- All recorded final commands complete. Engine 879 passed/25 existing ignored;
  root 233/1; native broker/security 57/1; mandatory acceptance 47/0, including
  25 strict Chrome comparisons and 1685 pinned DOM WPT assertions. Both signed
  sandbox checks and strict codesign verification passed; Clippy/fmt passed.
- Independent native component build/graph/tests pass. The production graph gate
  exits 1: five Blitz runtime names remain. Do not delete Blitz or call the worker
  native-only. The default production backend remains the reference backend.
- Fresh native/Chrome captures reviewed for all four targets. Rust overlap and
  rustdoc offscreen-main defects are corrected; example.com centers correctly.
  All public pages remain PARTIAL with explicit remaining visual/standards gaps.
- Comparable and independent release benchmarks passed. Isolated binary RSS was
  measured outside the outer tool sandbox after time's clockrate query failed
  inside it; original browser/signed-worker restrictions were not altered.
- Final artifacts: `wega-blitz-removal-report.md`, dependency inventory,
  compatibility/performance reports, and `baseline/wega-blitz-removal-evidence.json`.
  Logs, commands, captures, measured geometry, dependency trees and hashes are in
  `baseline/blitz-removal-2026-11-17/`. The closure commit changes documentation and
  evidence only. No further source validation is needed for that closure alone.
- Decision: PARTIAL MIGRATION. Next required implementation is production
  CSSOM/resource/control/focus/VM/frame ownership migration to the new document,
  then native-only runtime/security/compatibility gates before any Blitz removal.
- No push issued. No DevTools D4/inspector implementation work performed.
