# Architecture decision record

The accepted model is a modular monolith with ports and adapters. The application
is one process and one Cargo package; dependencies point from desktop adapters
toward the reusable core. Splitting into crates is deferred until a real boundary
requires separate compilation or distribution, as proposed in the initial review.

| Original recommendation | Implementation | Validation |
| --- | --- | --- |
| Extract an engine-independent core | `lib.rs`, optional desktop dependencies, unsafe forbidden in core | Default-free core tests and Clippy |
| Use commands and events for core changes | Application commands/events; session commands/events; navigation commands | Pure transition and port contract tests |
| Separate persisted state from presentation | Application owns private Profile; Chrome owns ephemeral forms/input | Compilation prevents UI profile mutation; settings and native smoke suites |
| Narrow Browser responsibilities | Browser coordinates core operations and owns desktop handle lifetimes; workers, persistence, session transitions, permissions and presentation state have dedicated owners | Core contracts plus native integration |
| Encapsulate state behind operations | Profile and session structure are private; UI, shortcuts, WebDriver and native drivers use shared operations | Core and real WebDriver/tab-drag tests |
| Introduce replaceable external boundaries | ProfileStore injected into Application; NavigationPort, DownloadPort and consuming PermissionPort have concrete adapters | Failure propagation and zero-effect rejection tests with recording adapters |
| Keep crash-safe storage and recovery separate | FileProfileStore owns lock and guarded durable replacement; Application independently blocks writes after failed load | Lock contention, failed writes, corrupt files, stale temp files, collision saturation |
| Keep native tests out of production | Explicit test-harness feature and explicit isolated profile requirement | Parser contracts and native runner |
| Separate portable auth tests from Docker | Real handler tests use temporary credentials and substituted process boundaries; isolated Compose environment tests actual gateway/WebSocket behavior | Python contracts and Docker integration workflow |
| Automate quality and produce reports | Pinned tooling, CI workflows, native runner, Rust/Python/JavaScript exports, source inventory and exact 100% line gates | Gate tests reject rounding, missing files/platforms and invalid captures |
| Establish Git baseline | Initial commit c0849c1 appeared in the shared checkout during verification | This agent did not stage, commit or push; history left untouched |

The interfaces are intentionally specific. Rendering, AppKit objects, Servo
JavaScript/developer-tools requests and OS clipboard calls remain desktop adapter
implementation details. Introducing traits for every concrete method would add
indirection without improving the application boundary. Native test drivers can
inspect adapter details for integration assertions; application changes follow
the same operations as interactive use.

Input translation is separated from delivery: `input::keyboard_input` converts
ordinary keyboard values, while the public adapter sends the resulting event to
Servo. Browser shortcuts similarly accept logical key/state values and use the
same tab/navigation operations as other entry points. Tests can exercise these
boundaries without constructing platform-private winit event fields or replacing
the real engine with a simulated browser.

Accessibility node builders must not re-enter egui Context while its write lock
is held. Context values are read before the callback, and a real native
activation/grafting contract guards this lock-order requirement.

Remaining verification obligations are tracked in [quality-report.md](quality-report.md)
and the newer [coverage progress report](coverage-progress.md): 100% line
coverage has not been achieved across Rust or all required build configurations.
Shell process contracts now have complete line counters; native packages still
need platform verification. Windows directory durability, physical permissions and
remote CI execution are not certified by macOS tests. These are open checks,
not completed architectural results.


## 24 October 2026: independent second-engine worker

A real second engine now justifies an engine boundary. Wega reuses published
Blitz primitives and owns a QuickJS-ng script host, networking/limits and lifecycle.
Servo 0.7 and Blitz beta.4 require incompatible ICU versions, so the new engine
has an independent Cargo workspace/lockfile and one private worker per tab.
The versioned pipe protocol carries bounded commands, state and pixel frames;
the existing NavigationPort has adapters for both engines. Servo-specific APIs
remain guarded desktop features instead of pretending the worker implements them.
Servo initialization is lazy; it is still process-scoped once used. Worker
termination releases a Wega tab's process resources, and the UI retains only its
newest frame. The native footer selects a tab's engine and reloads its page;
URL/title/engine persistence and actual switching are integration tested. The
existing default-free core remains independent of both rendering implementations.
See [engine strategy](engine-strategy.md) for limitations and remaining work.

## 25 October 2026: V8 via deno_core replaces QuickJS-ng in the Wega engine

A spike (`engines/v8-spike`) showed that V8 (`v8` 150.4.0 through `deno_core` 0.413.0)
and Blitz 0.3.0-beta.4 resolve and link in the engine's independent Cargo graph:
V8 bundles its own C++ ICU, so the Servo ICU conflict does not recur. V8 ran the
same script 8–15× faster than QuickJS-ng for about 12–20 MB more per tab, and Deno's
MIT Web API crates (fetch, events, timers, streams, storage, WebSocket, crypto)
replace bindings Wega would otherwise write. The engine therefore moves to V8 behind
a `ScriptHost` trait, keeping QuickJS as a feature until V8 passes the compatibility
gates. Wega keeps the single blitz-dom DOM and writes the DOM bindings itself.
Cloudflare's acquisition of Deno ends upstream development in about 12 months, so the
deno_core sources are vendored in `engines/vendor/deno/` and the V8 prebuilt libraries,
crate sources and git history are mirrored locally; an offline build was verified.
See [V8 engine plan](v8-engine-plan.md).

## 26 October 2026: V8 is the only Wega JavaScript runtime

Remove QuickJS from the engine and standalone spike, their dependency graphs,
CI baseline/contracts and feature selection. The owner chose V8-only maintenance
after the optional QuickJS Windows Angular fixture hit its native stack limit.
V8 is now mandatory, including builds with default features disabled. Keep the
framework fixtures, limits, error-isolation contracts and mutation proofs running
against V8. Old benchmark records remain historical evidence. This decision does
not change Servo's separate role or make Wega the default browser engine.

## 31 October 2026: Strategic commitment to standalone Wega engine and retirement of Servo

The owner resolved to deprecate and retire the Servo rendering engine completely,
committing 100% of engineering bandwidth to Wega's native engine (`blitz-dom`, V8 via
`deno_core`, and Skia). Maintaining Servo as a dual-engine fallback divided developer
effort between fixing bugs in upstream SpiderMonkey/Servo and advancing Wega's own
engine, while locking Wega into split Cargo workspaces due to the ICU 2.1 vs 2.3
dependency conflict.

Under this decision:
1. Servo and its dependent crates (`servo`, `servo-script-bindings`, `servo-devtools-traits`,
   `servo-base`, `malloc_size_of`) and features (`multiprocess`, `media-gstreamer`)
   will be removed.
2. The Cargo dependency graph will be unified into a single root workspace.
3. The out-of-process worker-per-tab architecture over versioned IPC is retained.
4. Internal `wega://` pages will be implemented as native egui views.
5. All web platform features (WebIDL generated bindings, selection/clipboard, form
   controls, sandboxing, IndexedDB, Canvas 2D, and platform media) will be implemented
   directly in Wega's standalone engine following the phased plan in
   [standalone-engine-migration.md](standalone-engine-migration.md).
