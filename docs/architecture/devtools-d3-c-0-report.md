# D3-C.0: layout safety, geometry feasibility and editing architecture — report

Local only, nothing pushed, **no D3-C feature implemented** (no picker, overlay or editing).
Baseline `82e642b`. Design: [`devtools-d3-c-0-design.md`](./devtools-d3-c-0-design.md).
Benchmark samples: [`baseline/devtools-d3-c-0-console-bench.json`](./baseline/devtools-d3-c-0-console-bench.json).

## Commits

| Commit | Content |
|---|---|
| `c3632f6` | Layout fix (`MAX_LAYOUT_DEPTH`), killable-subprocess regression tests |
| `aeb1102` | Wega-owned cached-only geometry types and reader, feasibility tests |
| design commit | `devtools-d3-c-0-design.md` |
| this commit | report, benchmark evidence, roadmap status |

## 1. Depth-122 layout hang

**Root cause: a native stack overflow that spins instead of aborting. Not a layout loop, deadlock,
Stylo problem or exponential algorithm.**

Evidence (killable child processes, external 15-20 s deadline, debug build, macOS arm64):

* lldb on a hung child: thread stopped at `EXC_BAD_ACCESS` on a stack address in Taffy
  `perform_final_layout_on_in_flow_children`. Earlier samples showed recursion
  `compute_block_layout -> compute_inner -> LayoutPassState::compute_child_layout_internal`.
* Time is flat then infinite: ~0.5 s at every completing depth.
* The cliff follows stack size and call context:

| Thread stack | script-initiated first flush | paint then flush | paint |
|---|---|---|---|
| 2 MiB (test default) | 121 ok, **122 fails** | 128 ok, 256 fails | 128 ok, 256 fails |
| 8 MiB (`wega-dom` engine thread) | 520 ok, 600 fails | 520 ok, 600 fails | 520 ok, 600 fails |
| 64 MiB | 256 ok | 256 ok | 256 ok |

* Instrumenting vendored code changed frame sizes and moved the failure, which is how the
  stack explanation was found (the adapter alone "does not reproduce" because it uses less stack
  per level).
* Not explained: why this platform spins rather than aborts; other platforms were not tested.

**Fix** (Wega-owned, no vendored change, no new dependency, 0 unsafe): `MAX_LAYOUT_DEPTH = 256`;
`BlitzDocument::flush_layout` refuses deeper trees via an iterative early-exit check;
`GeometryAccess::layout_refused()`. With the guard, depths 257, 600, 1,000 and 5,000 complete
through script flush, paint and paint-then-flush (5,000 in about 2.3 s, all of it HTML parse).
Refused documents render blank; the engine stays alive.

**Regression tests** (`engines/wega/tests/layout_depth.rs`, each scenario in a child process killed at
a deadline; and `adapters/blitz/tests/layout_safety.rs`): depths 120, 121, 122, 128, 256 and 1,000 in
three modes on the 8 MiB engine stack; boundary test (limit-3 divs lay out, one more is refused with
height 0); a characterisation test that depth 200 does **not** complete on a 2 MiB stack (the limit is
valid only on the engine stack); adapter tests for 20, limit, limit+1, 1,000, 5,000; wide documents
(20,000 siblings) are not refused. The two old pins in `reliability.rs` were replaced.

**Residual risks, not fixed:**

* HTML parsing is O(depth²) in html5ever (1,000: ~0.1 s; 5,000: ~2 s; 20,000: not finished in 15 s).
  Recommended fix: a parser nesting cap like Chrome's 512 (separate round, touches every insertion path).
* Script `cloneNode(true)` at 5,000 levels is stopped by the script watchdog (no crash).
* Release-build and Linux/Windows stack cliffs were not measured; the 256 limit relies on debug frames
  being the larger ones.
* Any new thread hosting a `View` must have at least 8 MiB.

## 2. Geometry feasibility (measured, `geometry_feasibility.rs`)

| Question | Result |
|---|---|
| Four boxes from cached layout | Yes. Fixture margin 10/20/30/40, padding 5/6/7/8, border 2, 100x50: border box (40,10,118x66), padding box (42,12,114x62), content box (50,17,100x50), margin box (0,0,178x106) |
| Device scale / zoom | CSS-pixel boxes identical for DPR 1 and 2 and for zoom 1 and 1.5; viewport size in CSS px = window / (DPR x zoom) |
| Viewport scroll | Box y 10 -> 5 after scroll 5; centre of the box hit-tests to the same element |
| Nested scroll container | Inner box y 236 -> 196 after `scrollTop` 40 |
| Clipping | **Not applied**: 200 px child of 50 px `overflow:hidden` parent reports 200 |
| Transforms | **Not applied**: `translate(30px,40px)` box reports x=0 |
| States without flushing | Pending (never flushed), NoBoxes (`display:none`), Invalid (bad key), Stale (after mutation, old boxes returned), Refused (depth 400) all verified |
| Cost | 1,000 reads on a 5,000-paragraph page: 1.4 ms (debug) |
| Deep DOM | refused, not hung |

## 3. Performance uncertainty (console, D3-A `ac6928e` vs D3-B `82e642b`)

Isolated worktrees and separate `CARGO_TARGET_DIR`s, optimized binaries, 10 alternating pairs with
order reversed each pair, macOS 27.2.1 arm64. Host: load average 4-7 during runs, and one foreign
`ANECompilerService` at about 100% CPU, so **not a quiet host**; spreads were nonetheless under 3%.
Values are p50 [min-max] over 10 runs.

| Metric | D3-A | D3-B | B/A |
|---|---|---|---|
| log(string, number) p50 us | 8.57 [8.48-8.83] | 8.87 [8.65-9.13] | 1.035 |
| warn(string, number) p50 us | 10.80 [10.54-10.94] | 10.94 [10.78-11.23] | 1.013 |
| warn p95 us | 11.17 [11.05-11.49] | 11.36 [11.14-11.72] | 1.017 |
| nested object p50 us | 29.00 | 29.21 | 1.007 |
| 100-property object p50 us | 114.5 | 115.5 | 1.009 |
| Process CPU s (user+sys) | 4.862 [4.834-4.883] | 4.908 [4.848-4.978] | 1.009 |
| Process wall s | 4.871 | 4.917 [4.853-6.706] | 1.009 |

**The D3-B report's +20% warning median and +7.2% process CPU were not reproduced**; the measured
difference is about +1% with overlapping ranges, within host noise. They were most likely noise from
a shared, loaded host. No console code was changed. A strict regression gate (<1%) still needs a
dedicated host. Resident growth for the 100,000-call flood was 10.2 MiB (single run, unchanged scale).

## 4. Validation executed

Executed on this host (macOS arm64, rustc per `rust-toolchain.toml`):

* Engine integration targets, `--test-threads=1`, all passed: layout 15, layout_depth 4 (+1 ignored
  exploration matrix), dom 30, dom_budgets 6, dom_identity 4, css 21, css_wpt 2, style 16, render 17,
  reliability 2, epoch 5, inspector 15, inspector_dom 7, inspector_styles 5, security 9, documents 5,
  scripts 8, wpt 1, console 14, inspector_console 21.
* **`console` and `inspector_console` each failed once** (2 and 1 test) in the first matrix run while
  two release builds were compiling (load average about 23); both passed fully on the immediate rerun
  in isolation. The tests were not otherwise investigated, so a load-sensitivity cause is likely but
  unproven.
* Package tests: wega-adapter-blitz 82, wega-protocol 87, wega-dom 2, wega-html 2.
* Root: `cargo test --features test-harness --lib --bin wega`: 113 and 98 passed.
* `cargo fmt --check` (root and engines), `git diff --check`.
* Strict clippy `-D warnings`: engines workspace all targets, and root all targets: clean.
* Architecture check: 13 members, 24 edges, 0 violations, 11 informational (unchanged).
* `check-engine-boundary.py` and `verify-engine-vendor.py`: pass, 13 pinned archives match (no vendor
  change this round). `cargo metadata --locked` for root and engines; no manifest or lockfile diff.
* Unsafe inventory: 0 added `unsafe` lines versus `82e642b`.

**Not executed:** the full 87-target engines matrix (only the subset above), signed worker tests,
signed App Sandbox probe, native browser/detached-window scenario, broker-mode suites, root
`--no-default-features` tests, WebIDL tests, DOM WPT beyond `wpt`, `inspector_sandbox` (needs the
signed helper), Linux and Windows, release-build stack cliff, long soak.

Cleanup note: disk was nearly full; about 24 GB of stale (older than a day) test executables in the
gitignored `target/wega-engine/debug/deps` and `engines/target/debug/deps` were deleted. They are
regenerated by `cargo test`.

## 5. Recommendations

| Feature | Decision | Conditions |
|---|---|---|
| Cached-only geometry (read-only box model, freshness states) | **GO** | Never flush; show Pending/Stale/Refused; coordinates in viewport CSS px |
| Element picker | **CONDITIONAL GO** | Browser-side interception in `wega_ui.rs`; `PickNode` via `element_at` with no JS and no flush; one request in flight; refuses on Pending/Refused; wheel remains forwarded; lifecycle tests for tab switch, navigation, process death and detached window |
| Highlight overlay | **CONDITIONAL GO** | egui overlay only. Must handle transforms (suppress or label; needs a bounded computed `transform` read) and ancestor clips (intersect), because the engine reports neither. Without that it is not accurate on real pages |
| Editing: attribute, text, inline-style, node removal | **CONDITIONAL GO, phase C.4 only** | Separate `Mutate` grant bound to session/target/process/document, separate security review, undo with epoch check, tests that evaluate and mutate grants never imply each other |
| Editing: stylesheet rules / matched-rule edits | **NO-GO** | Matched rules unsupported (D3-B) |
| Parser nesting cap | Recommended before D3-C.5 | Separate engineering round |

If the layout fix were rejected, a cached-only geometry implementation would still be provably safe
for readers (they take `&self`, never flush: verified on 121 to 2,000 levels), but the picker and
highlight would also rely on the page itself not flushing, which a script can trigger. The fix
removes that dependency.

## 6. Proposed phases

C.1 geometry protocol and Elements box-model view; C.2 picker without overlay; C.3 overlay with
clip and transform handling; C.4 Mutate capability and edits; C.5 hardening (parser cap, release and
cross-platform stack measurements, dedicated-host benchmark gate). Details in the design document.
