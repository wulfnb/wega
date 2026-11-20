# DevTools D3.0 report: architecture prerequisites and hardening

Status: complete locally (not pushed). D3-A, D3-B and D3-C have not been started.
Base: `f1f98cc` (readiness audit). Commits: `eb7d3ee`, `7a48a9f`, `bfafdb0`, `2b89a0d`, plus the report/WebDriver commit.

## 1. Evaluation policy: committed security principal (`7a48a9f`)

- The D2 URL-scheme check is gone. The **browser** decides each grant from the committed
  `wega_network::principal(url)` bound by `FrameAuthority::bind` at `DocumentCommitted`
  (`Origin::Tuple` only for broker-observed http/https; file:, data:, about: and
  uncorroborated commits are `Opaque`).
- Opaque principals (including file:) need an explicit per-grant override. A tuple origin
  needs none.
- The grant is a 128-bit CSPRNG token plus session, document stamp, principal and scope. The
  engine does not re-derive trust from a URL. It only checks consistency: the session, token
  and current document stamp match, and a browser-asserted tuple origin equals the engine's
  own http/https origin. `(None, override=true)` is accepted, `(None, false)` is denied
  (`PolicyDenied`).
- The principal is re-resolved at execution time (`authority.resolve`). A stale
  principal/document pair is rejected after navigation or re-bind.
- Tests: protocol policy tables, forged tokens and scopes, cross-tab requests, re-bind and
  navigation staleness, token redaction (82 protocol tests, `src/wega_page.rs` page tests,
  engine `inspector` tests).
- URL comparison is used only as a consistency cross-check, never as proof of authenticity.

## 2. Vacuous D2 console-forgery test (`eb7d3ee`)

The old test relied on `.ok()` over a call that threw because `Deno` is undefined, so it
asserted nothing. It is replaced by:
- `host_operation_handles_are_unreachable_from_page_scripts`, which asserts that the host
  handles are actually absent;
- `the_page_callable_error_report_path_cannot_forge_unbounded_or_non_error_entries`, which
  exercises the one reachable report path.

## 3. Console capture cost (`eb7d3ee`)

`ConsoleLog::shrink` now returns the size, so each entry costs a single `approx_bytes` pass.

Release, in-process `console_bench`, 3 interleaved runs per binary, median us/call
(before = D2 head, after = this branch):

| case | before | after |
|---|---|---|
| log(string, number) | 11.24 / 11.46 / 11.32 | 10.87 / 11.02 / 10.80 |
| log(string, nested object) | 32.31 / 33.23 / 32.46 | 31.83 / 31.31 / 31.68 |
| warn(string, number) | 15.95 / 16.18 / 16.44 | 15.70 / 15.72 / 15.71 |
| log(8 numbers) | 16.74 / 17.16 / 16.99 | 15.55 / 16.01 / 15.87 |
| log(100-property object) | 119.0 / 119.0 / 120.9 | 117.6 / 117.7 / 117.2 |
| flood RSS growth (100k calls) | ~10.6-11.0 MiB | ~10.6-11.1 MiB |

The gain is about 3-6% (0.4-1.0 us/call). Memory is unchanged.

`currentUserCallSite()` was investigated separately (sampled release profile): the call-site
op costs about 2.6-3.1 us/call, roughly 19% of the capture path. Stderr writes (about 38%)
pre-date D2. **Source locations are preserved.** Removing or sampling call sites would be a
product decision and is not recommended on these measurements.

## 4. Mutation epoch (`bfafdb0`)

- `MutationEpoch` / `MutationClock` live in `wega-dom`. The adapter newtype
  (`BlitzDocument`) owns the clock, and the DOM stays authoritative in the adapter (no change
  to DOM ownership).
- A wrapping u64 is combined with Blitz's `stylesheet_generation`. It is separate from
  `DocumentContext` (DocumentId / DocumentRevision). Overflow wraps, and consumers compare
  for inequality only.
- Each `Mutator` op bumps the epoch once, and only on success. Failed ops never move it
  (test `failed_operations_never_move_the_epoch`). Native inspector edits and script edits
  go through the same `Mutator`, so both bump it.
- `StatusSnapshot` carries `mutation_epoch` and `layout_pending`.
- **Known gap:** in-sheet CSSOM `insertRule`/`deleteRule` does not bump Blitz's
  `stylesheet_generation`. It is pinned by a test, and `record_external_mutation` is the
  hook for the future CSSOM-edit path.

## 5. Budget audit (`2b89a0d`)

- Audit: the node-count (30,000), text-byte (16 MiB) and depth checks lived only in the
  script-facing `Dom` host ops. Future native inspector edits would have bypassed them.
- Fix: `Dom::reserve_nodes` / `commit_nodes` / `charge_text` / `reserve_edit`, plus
  `MAX_EDIT_DEPTH = 121` and `View::reserve_dom_budget`. Script and native edits share one
  reservation.
- Pinned behaviours: `document.write` truncates, and `<template>` contents are uncounted
  (invisible to `node_count`).
- Residual: scripts have no depth budget beyond the layout hang below.

## 6. Blitz CSSOM feasibility spike (read-only)

Nothing was implemented. No "substantial vendor change" condition was hit, so no
stop-and-report was needed.

- **Stylesheets and rules (listing, text, editing):** `blitz-dom/src/cssom.rs` already
  provides path-addressed rule info (`stylesheet_rule_count`, `stylesheet_rule_info`) that
  Wega does not use today. This is adapter work only: map it into a Wega-owned
  `StyleSheetAccess` contract in `wega-dom`.
- **Computed style:** `node.primary_styles()` exposes `ComputedValues`. Enumerating
  properties through `LonghandId` is adapter work.
- **Matched rules:** `ComputedValues::rules()` returns the retained rule chain
  (`StrongRuleNode::self_and_ancestors`, `style_source`, `cascade_level`,
  `cascade_priority`). Matched rules can be read without re-running selector matching.
  Specificity needs a selector re-evaluation.
- **Smallest proposed patch:** a small `blitz-dom` accessor that maps a
  `StyleSource::Rule` back to `(owner node, rule path)` using the existing cssom path code,
  or an adapter-only fallback that exposes the matched rule's CSS text without the owner.
  Maintenance: a patch file, a `sources.json` entry and `scripts/verify-engine-vendor.py`.
  Start with the adapter-only version and add the patch only if owner mapping is required.
- Caveats: the spike was done by reading source. The rule-chain walk was not proven with a
  running probe, and editing in-sheet rules hits the epoch gap above.

## 7. Nested-block layout hang and inspector geometry (`bfafdb0`)

- The hang (roadmap B1) was characterised: a script-initiated first flush before any paint
  with about 122 nested auto-height blocks plus inner text. It reproduces only through the
  engine pipeline, not the adapter alone. The adapter observation and an engine-level pin
  are tests.
- Readers (`DocumentView`, `GeometryAccess`, `HitTest`) take `&self` and never flush.
  `layout_pending()` reports whether cached layout is stale. Inspector geometry therefore
  uses cached layout only and reports "layout pending" in `StatusSnapshot`.
- No synchronous inspector-triggered layout was introduced. An inspector-triggered flush
  remains forbidden until the hang is fixed.

## 8. WebDriver ExecuteScript trust boundary

Audit (source) plus an empirical probe against `target/debug/wega --webdriver-port`:

| request | result |
|---|---|
| `Origin: https://evil.example`, `Origin: null`, loopback origin | rejected (`Invalid Origin header`) |
| `Content-Type: text/plain` or form | rejected (`Invalid Content-Type`) |
| `Host: evil.example`, `Host: 127.0.0.1` (no port) | rejected (`Invalid Host header`) |
| `OPTIONS` preflight | 405 |
| no `Origin`, valid Host, JSON | accepted (non-browser local client) |

- Web pages therefore cannot reach the endpoint (CSRF, simple requests, DNS rebinding).
- Any local process or user can drive any open page: ExecuteScript, cookies, input. The
  endpoint is unauthenticated by design and documented as such in `--help`. This is not a
  narrow compatible fix: authentication would break W3C clients.
- Element-reference arguments are refused. ExecuteScript runs in the page's own world, so
  the 27 page-callable `__wega_*` helpers are no more exposed than to page script.
- **No code fix.** Added regression coverage to `tests/webdriver_smoke.py` (Origin, Host,
  Content-Type refusals). **Remediation plan:** an optional per-launch bearer token
  (`--webdriver-token`, printed at startup) enforced before any command, with unix-socket
  or named-pipe transport as a longer-term option.

## Test results

| gate | result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --locked --workspace --all-targets -D warnings` (engines) | clean (only non-denied vendor warnings) |
| `cargo clippy --locked --no-default-features --all-targets -D warnings` (root) | clean |
| same with `--features test-harness` | clean |
| `wega-protocol` | 82 passed |
| `wega-dom` | 2 passed |
| `wega-adapter-blitz` | all targets passed (incl. `epoch`, `layout_safety`) |
| `wega-engine` lib | 66 passed |
| engine integration: console 14, epoch 5, dom_budgets 6, dom_identity 4, inspector 15, inspector_console 21, security 9, reliability 4, layout 15, style 16, dom 30, documents 5, scripts 8 | passed |
| `wpt` 1, `css_wpt` 2 | passed (no regression) |
| root `--no-default-features --lib` | 109 passed |
| `check-engine-boundary.py`, `verify-engine-vendor.py` | pass |
| `cargo metadata --locked` (dependency graph, no cycles) | ok |
| unsafe inventory | no `unsafe` added since `f1f98cc~1` |
| `inspector_sandbox` (sandboxed worker) | **not run**: needs `WEGA_CONTENT_HELPER` (bundled helper), unavailable here |
| `tests/webdriver_smoke.py` | new header checks pass; the script later fails at a `Capability fixture` title assertion. The same failure occurs without my change, so it is pre-existing and not investigated |

## Unresolved risks

1. Sandboxed-worker evaluation test not run in this environment. Run it with a bundled helper before D3-A.
2. Epoch does not observe in-sheet CSSOM edits (pinned, hook ready).
3. Layout hang at depth 122+ is characterised, not fixed. Geometry must stay cache-only.
4. WebDriver endpoint is unauthenticated to local processes.
5. CSSOM matched-rule path is reasoned from source only.
6. `webdriver_smoke.py` has a pre-existing failure after the new header checks.

## Recommended boundaries

- **D3-A (DOM inspector, read plus native edits):** the node tree, attributes and text edits
  through `Mutator`, using `reserve_dom_budget` and the epoch for staleness. Geometry is
  cache-only with a `layout_pending` indicator. Evaluation grants are principal-based.
- **D3-B (styles):** the adapter `StyleSheetAccess` contract, computed styles and the
  rule listing without vendor changes. Matched rules via the optional small blitz-dom
  accessor. Must close the in-sheet epoch gap before any rule editing.
- **D3-C (layout and network/other panels):** blocked on fixing the B1 hang. Any
  inspector-triggered flush is out of scope until then. WebDriver token auth can ship
  independently.
