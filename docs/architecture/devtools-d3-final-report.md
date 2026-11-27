# D3 final: implementation report

Local only; **nothing pushed**; D4 not started. Baseline `589dc12` (D3-C.0). Design:
[`devtools-d3-final-design.md`](./devtools-d3-final-design.md). What is and is not supported:
[`devtools-d3-acceptance.md`](./devtools-d3-acceptance.md). Machine-readable results:
[`baseline/devtools-d3-final-evidence.json`](./baseline/devtools-d3-final-evidence.json).

## Local commits (after `589dc12`)

| Commit | Stage |
|---|---|
| `84351ae` | Design, committed before feature work |
| `db1d9b7` | Layout guard validation (engine-stack constant, bypass routes, recovery, unguarded-cliff probe) |
| `2d2e03b` | Inspector 5 / protocol 26: geometry, picking, separate mutation grant, edits; adapter paint mapping |
| `9617884` | Browser: picker interception, overlay, Box Model, editing UI, session and lifecycle logic |
| `611f598` | Native end-to-end acceptance with visual evidence |
| `71144ae` | Latency benchmarks, deep-document real-worker test, release-aware characterization |
| `2f1aff0` | Sandbox coverage of the D3 commands, load-tolerant tests, signed-worker evidence |
| `4abbb9b` | Strict clippy fixes, observer limitation pin, acceptance document |
| this commit | Final report, evidence JSON, roadmap |

## 1. Layout safety (validated, not assumed)

* One constant, `wega_engine::ENGINE_THREAD_STACK` (8 MiB), now sizes the engine thread and every probe.
* Depths 120, 121, 122, 128, 200, 256, 257, 520, 600, 1,000 in killable children on that stack, in
  **debug and release**, through script flush, paint, paint-then-flush and a misc route (computed style,
  `scrollIntoView`, geometry reads, resize, hidpi and dark repaint, pointer); every child completes, the same
  view then renders an ordinary page (77 px fixture), and a real `wega-engine` worker given a 1,000-deep
  document reports `Refused` geometry and stays responsive.
* Only the adapter's `flush_layout` reaches Blitz `resolve` in production code (vendor callers are unit tests).
* **Unguarded cliff, 8 MiB stack:** debug 520 to 600 levels; **release between 3,200 and 4,800**. Margin over
  the limit of 256: about 2x in debug and more than 12x in release. The limit was **not** raised or changed.
  On a 2 MiB stack the debug script-flush cliff is 121 to 122 (the original "depth-122 hang").
* Platforms: macOS arm64 only. Linux and Windows are unmeasured; the guard is a conservative number on a
  stack size that is explicitly requested on every platform, not a proven cross-platform bound.
* The parser's O(depth²) cost remains a tracked engine defect (see the acceptance document).

## 2 to 6 and 8 to 9. Features

Implemented as designed: cached geometry with six explicit states; Inspect Element picker with browser-side
interception (page sees no hover, press, release, click or key; scroll still forwarded); hover and selected
overlay that draws only when the engine's boxes are the painted geometry (transforms, positioned-under-clip and
unknown are suppressed with a stated reason; ancestor clips are intersected); Box Model panel; separate editing
grant; attribute add, change, remove; text and comment node edits; node removal; inline CSS add, change, remove
and `!important`; selection and parent refresh through the normal bounded epoch path; no tree rebuild after an
edit. Details and limits: acceptance document.

## 7. Authorization (security findings)

* Evaluation and mutation grants are separate tokens, records and lifecycles on both sides; tested both ways at
  protocol, real-process, sandboxed-worker and native level. Neither opens the other.
* Revoked on navigation (document replacement), process replacement or crash, detach, session revocation,
  explicit revoke, tab close and DevTools close; queued edits are dropped when authority is lost.
* The generic UI request path **refuses `Mutate` and `GrantMutation`**; they have dedicated entry points that
  attach the browser's own record. Native test confirms a forged token and a forged grant request are refused.
* Edits are bound to the mutation epoch; code-running edits (`on*`, `srcdoc`, `javascript:` URLs, `<script>`
  text) additionally need the evaluation grant (verified applied when live, refused when not, in the two native
  passes). Protected nodes (`html`, `head`, `body`, document) cannot be removed.
* Edits go through the same `Dom::call` host operations page script uses, so the node and text budgets,
  invalidation, custom-element queue and mutation epoch are shared; one documented difference: no
  `MutationObserver` records (pinned by a test).
* Findings worth recording: a page that mutates continuously can starve edits (retry once, then report); no
  new listener, remote-debugging transport or unauthenticated mutation endpoint exists.

## 10. Matched CSS rules

Not attempted; the Unsupported state is retained and labelled "Matched (unsupported)". No heuristic specificity
or winner data is produced. This did not block completion.

## 13. Performance (optimized builds, isolated artifacts, alternating, no concurrent compiles)

### Existing paths: `589dc12` versus final, 8 alternating pairs, host load 3.4 to 5.2

| Metric | A p50 [min-max] | B p50 [min-max] | B/A |
|---|---|---|---|
| console log(string, number) p95 us | 10.73 [10.53-14.93] | 10.68 [10.49-11.18] | 0.995 |
| console warn p50 us | 10.84 [10.65-11.16] | 10.97 [10.80-11.24] | 1.012 |
| console warn p95 us | 11.18 [11.11-11.79] | 11.43 [10.97-11.71] | 1.022 |
| console nested object p50 us | 29.30 | 29.20 | 0.996 |
| console 100-property object p50 us | 115.4 | 114.8 | 0.995 |
| console flood resident growth MiB | 10.85 | 10.75 | 0.991 |
| render 800x600 rasterise ms | 0.24 | 0.24 | 1.000 |
| render 1600x1200 record / rasterise ms | 0.09 / 0.60 | 0.09 / 0.60 | 1.000 |
| native text mutation us | 0.0253 | 0.0251 | 0.992 |
| console process CPU s (median) | 4.917 | 4.905 | 0.998 |

No regression: the D3-C.0 finding (the earlier console increase was not reproducible) holds on the final code.
Closed-DevTools rendering is unchanged within measurement spread.

### New paths (optimized, 3 runs; in process, no IPC)

| Operation | p50 | p95 |
|---|---|---|
| GetGeometry read (small / 5,000 paragraphs / 20,000 rules) | 0.17 / 0.13 / 0.13 us | 0.21 / 0.17 / 0.17 us |
| PickNode: hit test + report (small / 20,000 rules) | 0.38 / 0.29 us | 0.42 / 0.33 us |
| PickNode on 5,000 paragraphs | 76 us | 78 us |
| Attribute edit (`apply_edit`) | 0.46 to 0.54 us | 0.54 to 0.58 us |
| Inline CSS edit (`apply_edit`) | 5.5 us (2.4 us with 20,000 rules) | 5.6 us |
| Inline CSS edit + next layout flush (small / 20,000 rules / 5,000 paragraphs) | 35 / 29 us / 3.0 ms | 44 / 39 us / 3.2 ms |
| Overlay plan + screen mapping per element | 0.58 us | 0.79 us |

### Through the real worker pipe (debug engine: the release engine refuses the harness's development launch)

GetGeometry round trip p50 142 us / p95 156 us; **PickNode round trip (the picker's hover latency) p50 182 us /
p95 195 us**; status snapshot reference 132 us. Hover requests are additionally limited to one per 40 ms and one
outstanding. A release-build pipe figure was not measured.

### Retained memory

Not measured as RSS. Bounded by construction: the Elements cache is unchanged (512 nodes, 512 KiB); geometry
state holds at most two reports (well under 2 KiB each); the edit queue holds at most 4 operations (each under
8 KiB) plus the editor text buffers. No benchmark claims zero overhead: closed-DevTools cost is the measured
table above.

## 14. Validation executed

Executed on macOS 27.2 arm64, rustc 1.98.0:

* **Engines workspace, every lib, bin and integration target, locked, serial, `--test-threads=1`: 92 target
  runs, 749 passed, 0 failed, 23 ignored** (87 normal targets 720 passed, plus the 5 broker-mode reruns 29
  passed, `WEGA_TEST_BROKER=1`: documents, forms, history, scripts, security). `inspector_edit` has since gained
  one test (11 now). Includes: protocol 92, adapter-blitz layout_safety 6 and geometry_feasibility 9, layout
  15, layout_depth 6, dom 30, dom_budgets 6, css 21, style 16, render 17, inspector 15, inspector_dom 7,
  inspector_styles 5, inspector_console 21, console 14, security 9, reliability 2, **DOM WPT 1, CSS-WPT 2**.
* Root: `--features test-harness` lib 113 and binary 119 (+1 ignored benchmark); `--no-default-features` lib 109.
* Architecture and WebIDL tool tests 8 and 6; WebIDL `check` current, `verify` OK.
* Strict clippy `-D warnings`: engines workspace all targets, root default, `test-harness` and `no-default-features`,
  engines/tools, WebIDL: clean. `cargo fmt --check` root and engines, `git diff --check`.
* Architecture check: 13 members, 24 edges, 0 violations, 11 informational, no cycles. Boundary check: pass.
  Vendor verification: 13 pinned archives match (**no vendor change this round**). Locked metadata: pass,
  no manifest or lockfile diff. Unsafe added: **0** lines.
* **Signed, App-Sandboxed worker** (bundled with `wega-content-bundle`, hardened sandbox entitlement present):
  `inspector_sandbox` (now covering geometry, pick, mutation grant, forged and evaluation tokens, stale epoch,
  revocation) 1 passed; `inherited_broker` 1 passed; `native_sandbox` ambient-authority probe 1 passed.
* **Native end-to-end (real window, real worker, injected pointer events through the real UI loop)**, with the
  signed sandboxed worker and again with the development worker: both passes (detached OS window and docked)
  passed, followed by detach/reattach and the stalled-worker test (NotResponding after 2,500 ms with 424 UI polls,
  recovered 29 ms after SIGCONT). Logs and images under `baseline/`.

**Not executed or not passing:**

* **WebDriver smoke fails** at `tests/webdriver_smoke.py:76` (`AssertionError: New tab`): the same assertion
  documented as pre-existing in the D3-A report; not re-run on the baseline commit, so "unchanged" is inferred
  from identical failure text, not proven.
* Linux, Windows, Docker-based suites: no host. Release-build real-worker pipe harness: cannot run by design.
* A production developer-ID signature (ad-hoc signing was used); long-duration soak; dedicated-host performance
  gate; AccessKit/keyboard-navigation tests of the new controls beyond existing UI tests; per-frame overlay
  cost inside the real egui renderer (only the pure plan and mapping were timed).

### The D3-C.0 `console` / `inspector_console` failures: exact mechanism

Reproduced 3 of 3 under compiler load (load average 21 to 29): `evaluate_json` returned
**"Script deadline exceeded"**. The page script watchdog (about 5 s) fires because two debug-build tests perform
a single very large JavaScript run (10,000 console calls; 8 heavy logs; later a 20,000-call flood) that exceeds
it when the CPU is saturated. A third test failed its own 10 s "layout never settled" wait at load about 21.
They are CPU starvation against wall-clock deadlines, not console correctness or performance regressions (idle:
3/3 clean, and the optimized A/B shows no change). The tests now split each run into bounded batches (same totals
and assertions) and the settle wait is 45 s.

## Reproduction

```sh
cargo test --locked --manifest-path engines/Cargo.toml -p wega-engine --test layout_depth --test inspector_edit -- --test-threads=1
cargo test --locked --manifest-path engines/Cargo.toml -p wega-adapter-blitz --test geometry_feasibility --test layout_safety
cargo test --locked --features test-harness --lib --bin wega
cargo test --release --locked --manifest-path engines/Cargo.toml -p wega-engine --test layout_depth     # then: --ignored print_unguarded_cliff
cargo test --release --locked --manifest-path engines/Cargo.toml -p wega-engine --test inspector_edit_bench -- --ignored --nocapture --test-threads=1
cargo build --locked --features test-harness --bin wega && cargo build --locked --manifest-path engines/Cargo.toml -p wega-engine --bin wega-engine -p wega-platform --bin wega-content-bundle
engines/target/debug/wega-content-bundle engines/target/debug/wega-engine /tmp/WegaD3F.app    # after ad-hoc codesign of the engine
WEGA_D3_EVIDENCE_DIR=$PWD/docs/architecture/baseline/d3-evidence \
  WEGA_CONTENT_HELPER=/tmp/WegaD3F.app/Contents/Helpers/WegaContent.app/Contents/MacOS/wega-engine sh scripts/inspector-native-test.sh
WEGA_CONTENT_HELPER=... cargo test --locked --manifest-path engines/Cargo.toml -p wega-engine --test inspector_sandbox --test inherited_broker -- --ignored --nocapture
```

## Recommendation for D4 (Network inspection)

**CONDITIONAL GO.** D3's inspector, session, grant and bounded-IPC foundations are sound, tested at every level
including inside the App Sandbox, and show no measured regression. Conditions before or alongside D4: (1) decide
whether the pre-existing WebDriver failure blocks the release gate and fix or waive it explicitly; (2) schedule
the parser nesting cap, since D4's large-document tooling will meet deep pages; (3) keep the D4 design's
non-goals on edit-and-resend (it must use the page's authority); (4) measure Linux and Windows before any
cross-platform claim about layout depth or the sandbox.
