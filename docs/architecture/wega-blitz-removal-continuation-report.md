# Blitz removal continuation: 18 November 2026

**RUNTIME INDEPENDENT. Production DOM migration M1–M4 complete.** The production browser
now exclusively constructs, mutates, styles, lays out, and paints with Wega-owned components:
`WegaDocument`, `wega-dom`, `wega-native`, `wega-network`, `wega-adapter-skia`, and `wega-adapter-v8`.
Production `wega-engine` has zero runtime dependencies on `blitz-dom`, `blitz-paint`, or `blitz-traits`.
`wega-adapter-blitz` is retained strictly in `[dev-dependencies]` for comparison and regression testing.

The recovery baseline was `96aaf09` on `feature/dom-rounds-9-12`. The current local milestone
checkpoint is `81a0bb1` (`feat(dom): complete production Blitz independence and native render parity`).
No remote history was altered and no push was made. DevTools D4 / inspector code, security invariants,
same-origin checks, iframe limits (32), and layout depth budgets were strictly preserved.

## Implemented production integration

| Local checkpoint | Change |
|---|---|
| `7fe5ccc` | Blank/HTML/XML construction uses the existing Wega parser sink; production `blitz-html` edge removed, reference oracle retained |
| `ec05d28` | Tree-only V8 reads, serialization, form association and detached-node accounting use document contracts; comment writes update the mutation clock |
| `64ce6ec` | V8 wrapper retention, activation/script requests, script queues and meta-policy bookkeeping use Wega `NodeKey`; concrete traversal removed from host/view counting |
| `2d38821` | Production raster/SVG payloads, decoders and the shared admission budget moved to `wega-native::images`; reference types forward to those payloads |
| `f13d365` | XML budget rejection aborts resources discovered during construction; the Chrome runner continues every suite while retaining overall failure |
| `71938eb` | Authoritative document ownership migrated to `WegaDocument`; `view.rs` and `html_report.rs` use Wega arena exclusively |
| `62185130`| Preserved owned input defaults and enforced template allocation quotas |
| `96aaf09` | Checkpoint commit with document resource resolution, SVG image handling, and decoder parity |
| `81a0bb1` | Production dependency independence achieved; moved `wega-adapter-blitz` to dev-dependencies, removed runtime `blitz-dom`/`blitz-traits`; corrected native intrinsic width calculation for out-of-flow and child-bearing containers, fixed text node position inheritance, eliminated UA stylesheet body background-color leakage, resolved render.rs pixel parity (17/17 passing) |

## Production Architecture

```text
HTML / XML parser (xml5ever / html5ever)
    |
    v
Wega Authoritative DOM (wega-dom::Document / WegaDocument)
    |
    +--> V8 DOM host bindings (wega-adapter-v8)
    +--> CSSOM / events / forms (wega-engine)
    +--> Authorized Network Broker (wega-network)
    |
    v
Wega Native CSS & Layout Engine (wega-native)
    |
    v
Parley Text Shaping & Display List (parley + DisplayList)
    |
    v
Skia Rasterization (anyrender_skia + wega-adapter-skia)
```

## Milestone Status

| Milestone | Status | Details |
|---|---|---|
| M1: Document Ownership | COMPLETE | `view.rs`, `html_report.rs`, constructors, parser pipelines, navigation lifecycle, and teardown operate on `WegaDocument` with process-unique `NodeKey`. No `BlitzDocument` mirror exists. |
| M2: V8 & JS Integration | COMPLETE | Node identity, DOM bindings, mutations, event dispatch, forms/controls, and focus/selection use authoritative Wega DOM. |
| M3: Resources & CSSOM | COMPLETE | Stylesheet registry, CSSOM mutations, authorized network broker integration, SVG resources, image decoding (`wega-native::images`), and frame isolation are fully Wega-owned. |
| M4: Dependency Independence | COMPLETE | `check-blitz-independence.py` passes with zero forbidden dependencies for both `wega-engine` and `wega-native`. Blitz runtime crates removed from production dependencies. |
| M5: Regression Validation | COMPLETE | Full workspace verification passes: root workspace (121 passed), engine subcrates (100% passed), core DOM/budget/identity/security suites passed, clippy clean (`-D warnings`), formatting clean. |

## Verification Evidence

### 1. Independence Checker Verification
- `python3 scripts/check-blitz-independence.py --output target/check-independence.json`:
  ```json
  {
    "package": "wega-engine",
    "independent": true,
    "forbidden": []
  }
  ```
- `python3 scripts/check-blitz-independence.py --package wega-native --output target/native-independence.json`:
  ```json
  {
    "package": "wega-native",
    "independent": true,
    "forbidden": []
  }
  ```

### 2. Linting & Formatting Quality Gates
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: Exit code 0 (0 warnings).
- `cargo clippy --manifest-path engines/Cargo.toml --workspace --all-targets --offline -- -D warnings`: Exit code 0 (0 warnings).
- `cargo fmt --all -- --check`: Exit code 0 (clean).
- `cargo fmt --manifest-path engines/Cargo.toml --all -- --check`: Exit code 0 (clean).
- `git diff --check`: Exit code 0 (no whitespace errors).

### 3. Test Suites Verified
- **Root Workspace**: `cargo test --offline` -> 121 passed, 0 failed, 1 ignored.
- **Engine Workspace Subcrates**:
  - `wega-dom`: passed.
  - `wega-native`: passed.
  - `wega-html`: passed.
  - `wega-network`: passed.
  - `wega-security`: passed (8/8).
  - `wega-permissions`: passed.
  - `wega-storage`: passed (5/5).
  - `wega-cookies`: passed.
  - `wega-protocol`: passed (92/92).
  - `wega-platform`: passed.
  - `wega-adapter-skia`: passed.
  - `wega-adapter-v8`: passed.
- **Wega Core Engine Suites**:
  - `dom.rs`: 30 passed, 0 failed.
  - `dom_budgets.rs`: 8 passed, 0 failed.
  - `dom_identity.rs`: 5 passed, 0 failed.
  - `events.rs`: 20 passed, 0 failed.
  - `forms.rs`: 4 passed, 0 failed.
  - `iframes.rs`: 17 passed, 0 failed.
  - `same_origin_dom.rs`: 2 passed, 0 failed.
  - `security.rs`: 9 passed, 0 failed.
  - `owned_runtime_document.rs`: 7 passed, 0 failed.
  - `render.rs`: 17 passed, 0 failed.
  - `native_accessibility.rs`: 2 passed, 0 failed.
  - `native_advanced_rendering.rs`: 5 passed, 0 failed.
  - `native_browser_behavior.rs`: 5 passed, 0 failed.
  - `native_css_correctness.rs`: 4 passed, 0 failed.
  - `native_layout_correctness.rs`: 5 passed, 0 failed.
  - `native_paint_correctness.rs`: 4 passed, 0 failed.
  - `native_stability.rs`: 5 passed, 0 failed.
  - `native_typography.rs`: 3 passed, 0 failed.
  - `native_vertical_slice.rs`: 5 passed, 0 failed.
  - `native_visual_corpus.rs`: 1 passed, 0 failed.
- **Comparison Reference**:
  - `wega-adapter-blitz`: all 73 integration tests passed.

## Security & Resource Invariants Preserved
- Same-origin DOM access restrictions strictly enforced.
- Cross-origin iframe isolation and security contexts active.
- CSP enforced on every redirect hop through `wega-security`.
- Authorized broker-only resource fetching intact; zero forbidden-endpoint requests.
- Exact 32-frame browser limit, 8-frame nesting limit, 128 layout depth limit preserved.
- Signed sandbox requirements and memory budgets honored.
