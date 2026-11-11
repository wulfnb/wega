# Servo backend removal validation

Branch: `refactor/remove-servo-backend`
Base: `origin/main` at `0316dd2`
Local validation: 11 November 2026, macOS arm64

## Implementation

- Removed the embedded Servo runtime, its default Cargo dependency, vendored backend crates, engine switching, and automatic fallback paths.
- Added a native Surfman/glow window context for the existing egui shell. Web tabs use only the standalone Wega worker; built-in pages use the native UI. A missing worker produces an explicit error.
- Retained Stylo CSS, Blitz DOM, V8, and Skia in the independent engine workspace. Servo-derived CSS support crates remain. Neither pinned dependency graph contains the Servo browser runtime or SpiderMonkey.
- Migrated saved Servo engine identities to Wega while preserving tabs and preferences. Retired command-line engine choices are rejected.
- Preserved three pinned MPL WebIDL reference inputs and optional GStreamer packaging allowlists outside the removed backend, with provenance and licenses.
- Connected document-bound worker permission prompts, notifications, and media sessions to native services. Permission grants expire when the document changes.
- Preserved native tab, container, settings, download, and WebDriver flows. Site-data clearing now preserves other containers. Attachment navigation hands download metadata to the host without buffering the entire file in the renderer.
- Fixed a developer-panel lock recursion, transparent-page canvas contrast, and screenshot availability after UI frame upload. Screenshot storage shares the retained frame buffer and clears it on navigation.
- Updated packaging, CI source checks, attribution, and current documentation. Added a dependency boundary gate to reject reintroduction of the retired backend. Historical documentation is explicitly marked.

## Local results

| Check | Result |
| --- | --- |
| Core library tests | 109 passed |
| Desktop tests with native harness enabled | 27 passed |
| Independent engine workspace, all targets, serial | 253 passed, 1 ignored, 0 failed across 30 suites |
| Python integration and packaging tool tests | 107 passed |
| Core and desktop strict Clippy | Passed |
| Rust formatting and whitespace checks | Passed |
| JavaScript line coverage gate | Passed at 100% |
| WebIDL generation and canonical output check | Passed |
| Workflow lint and shell lint | Passed |
| Embedded-backend dependency boundary | Passed for both lockfiles |
| Real macOS WebDriver, capture, standalone engine and native UI | Passed |
| Real macOS multipart download | Passed; four byte ranges, HttpOnly cookie reuse, exact 6 MiB + 173 byte output |

The initial checkpoint's disk-space and sandbox failures were resolved before these results. Tests requiring local sockets ran with that access. The ignored engine test requires the separately packaged native sandbox helper; other worker process and sandbox regression suites ran. These results do not establish release or cross-platform readiness.

Reproducible runtime artifacts are in `target/servo-removal-native/`: the final `engine`, `webdriver`, and `capture` results are in `final/results.json`; the download result is in `tests/results.json`. The final engine regression log and summary are in `target/servo-removal-validation/`. These generated artifacts are not committed.

## Remaining limitations

- The removed Servo-specific inspector, debugger, network bridge, and toolbox are not ported. Wega's native developer panel currently exposes worker and document state. Unsupported WebDriver commands return explicit errors.
- Wega's current media module models playback and generates video frames; this does not establish real codec decoding. The old codec suite is explicitly selectable but excluded from standalone acceptance. Full audio/video compatibility remains unfinished.
- Retired theme, capability, and toolbox harness modes report errors. The replacement engine/UI scenarios cover a smaller set of behavior.
- Windows and Linux runtime validation, full distribution builds, Docker execution, and hosted CI remain unverified for this branch.

An unrelated macOS DMG retry edit in `.github/workflows/cd.yml` remains uncommitted and is excluded from the removal commits.
