# Error reporting validation — 28 October 2026

Error reporting buffers locally when no endpoint is configured. A profile owns
its Ed25519 identity; the collector accepts only manually enrolled public keys.
Setup, wire format, retention and limits are in [the collector guide](../error-collector/README.md).
The standalone home-server stack is `wega-errors-compose.yml` in the home-server
checkout. Its public route defaults off. No registry push or remote deployment
was performed during the initial local validation. Production deployment and
verification followed on 28 October 2026, as recorded below.

## Local evidence

- Core library: 105 tests passed with default features disabled. Instrumented
  line coverage is **2622/2751 = 95.3108%**, passing the existing 95% gate.
- Desktop library: 107 tests passed, including actual HTTP signature verification,
  a 503 retry, stable report ID, and a fresh nonce on each attempt.
- Engine workspace: 140 tests passed before adding the shared diagnostic-bound
  test; the updated library then passed all 11 tests. The worker suite passed all
  six tests again, including HTML fragments, imported/external/inline/attribute
  stylesheet failures, console warnings/errors, and a burst of 40 errors plus
  300 unhandled promise rejections.
- Collector: five tests passed, covering signatures, tampering, unknown/revoked
  clients, stale timestamps, replay protection, idempotency, body conflicts,
  malformed/oversized requests, retention and restart persistence.
- Both static Docker images passed 15 real HTTP/container checks each under a
  read-only root filesystem, UID/GID 65532, dropped capabilities and a 128 MiB limit.
  ARM64 runtime image: **3,347,263 bytes**; amd64: **3,508,264 bytes**. These are
  local images, not published registry artifacts.
- Native, packaged and installed browser tests used isolated profiles. Each engine first
  queued HTML/CSS/JavaScript reports without a URL; after public-key enrollment
  and local endpoint configuration, every previously queued report ID appeared
  in the collector export. CSS diagnostics included stylesheet URL and line 1.
- Native smoke and engine UI suites passed. Strict Clippy passed for core,
  desktop, engine and collector code. Formatting, Actionlint, vendor integrity,
  macOS package signatures and Swarm configuration validation passed.

Artifacts are under `target/error-reporting/`: `core-coverage.json`,
`container-arm64.json`, `container-amd64.json`, `native/results.json`,
`packaged/results.json`, `installed/results.json`, and `native-ui/results.json`. Test profiles contain only
fixture data and disposable signing keys. They are not intended for publication.

Warnings emitted during shutdown may remain queued after the final HTTP attempt;
the next launch sends them. No emitted event is removed merely because a status
message was overwritten. Queue size is not automatically capped; individual
messages and transport bodies are bounded as described in the collector guide.
Installation keys authenticate approved profiles; stolen keys can impersonate
those profiles until revocation. They do not attest the executable.

The installed app is `/Applications/Wega.app`. Its pre-update rollback copy is
`target/error-reporting/Wega-before-error-reporting.app`. The browser profile is
preserved.

## Production deployment

The service `wega-errors_collector` is deployed on `new-horizons` at
`https://errors.noufal.de/v1/errors`. Its privately transferred amd64 image is
`wega-error-collector:20261028-160922c895d4`, with the verified Docker configuration
ID `sha256:160922c895d4a6f69001ebed511bbcf688c8643ed288dd058c25a86597f264a3`.
The runtime is healthy, read-only, and uses UID/GID 65532. Storage is the dedicated
server directory `/home/loganwolf/home-server/config/wega-errors`.

The default Mac profile is enrolled using its public key and configured for the
production endpoint. The installed app was launched and delivered its queued
synthetic setup report. The first verification also found 13 real CSS reports;
the local outbox was empty. An unsigned POST to the public endpoint returned 401.
The private key remains on the Mac. The existing `homeserver_wega` service retains
its prior image digest and configuration.

Reproducible deployment settings and maintenance commands are in the sibling
home-server checkout's `wega-errors.md`. Evidence is saved in
`target/error-reporting/production-deployment.json`.
