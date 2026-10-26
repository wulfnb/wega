# P4–P6 iteration

Branch: `feature/wega-p4-performance`, based on merged main `5d7c1fb`.
Scope: complete the P4–P6 implementation together before the next CI handoff.
Servo remains the general default; Wega rollout is limited to validated sites.

- [ ] P4: embedded startup snapshot of compiled binding/bootstrap factories;
  restore per-document Web API, origin, permissions and storage state at runtime.
- [ ] P4: native worker RSS, bounded V8/style pools, lite-worker policy, freeze GC
  and allocator/cache release with state-preservation regressions.
- [ ] P4: shared browser network broker, bounded HTTP cache and profile cookie jar.
- [ ] P4: owned display lists, asynchronous raster worker, bounded shared-memory frames.
- [ ] P4: repeatable compatibility/startup/active/frozen resource measurements.
- [ ] P5: Skia raster backend, CSS feature/pixel regressions and pinned CSS WPT subset;
  documented/upstream-ready patches for demonstrated Blitz gaps.
- [ ] P6: persisted per-site engine preference, address-bar engine badge,
  safe one-way Servo fallback and a narrow validated default-on allowlist.
- [ ] Combined engine/browser/tooling checks, packaging/CI dependency audit,
  recorded measurements and explicit remote-platform validation boundaries.

The pre-change development resource report is `/tmp/wega-p4-before/results.json`.
It uses debug builds and is not an optimized-release performance certification.
Historical JS heap figures are never treated as process RSS. Frozen live-page
state must survive; a memory target does not authorize destructive tab discard.
