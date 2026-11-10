# Servo backend removal checkpoint

Branch: refactor/remove-servo-backend
Base: origin/main at 0316dd2

This is an implementation checkpoint, not a validated release.

Implemented:
- Removed the embedded Servo runtime, default Cargo dependency, vendored backend crates, engine switching, and automatic fallbacks.
- Added a native Surfman/glow window context for the existing egui shell.
- Kept Stylo CSS, Blitz, V8, and Skia in the standalone engine.
- Migrated legacy saved Servo engine identities to Wega.
- Moved three pinned MPL WebIDL reference inputs and optional media packaging allowlists out of the retired backend.
- Connected worker media sessions, notifications, and permission requests to native services.
- Removed Servo-specific developer-tool adapters. The native Wega panel exposes worker state; richer standalone tooling remains a limitation.

Validation observed:
- Core library cargo check passed.
- Desktop cargo check with test-harness passed before subsequent cleanup edits.
- WebIDL regeneration passed.
- Core tests: 96 passed; 12 failed because loopback binds were sandbox-denied or disk space was exhausted.
- Desktop test build and strict lints stopped with No space left on device.
- Full native, packaging, cross-platform, and hosted CI validation remains pending.

Remaining work:
- Finish source cleanup, update current documentation and packaging, and adapt retired test-mode expectations.
- Review native permission/notification handling and standalone navigation/session behavior.
- Run formatting, strict lints, core/desktop/engine tests, and native worker smoke tests with adequate disk space.
- Verify no embedded Servo/SpiderMonkey crates remain in the resolved desktop graph.

An unrelated pre-existing macOS DMG retry edit in .github/workflows/cd.yml is excluded from this checkpoint.
