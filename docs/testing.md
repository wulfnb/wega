# Quality checks and coverage

Use the pinned Rust toolchain in `rust-toolchain.toml`. `Cargo.toml` continues to
declare Rust 1.95 as the minimum; this change was validated with Rust 1.98.0, not
with every minimum-version feature combination. Docker and CI use 1.98.0.

## Setup and ordinary checks

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements-dev.txt
PYTHON=.venv/bin/python sh scripts/check.sh
# Requires the native dependencies listed in README:
CHECK_DESKTOP=1 PYTHON=.venv/bin/python sh scripts/check.sh
```

The core check runs formatting, strict Clippy, Rust tests and portable gateway
tests. Desktop checks cover both production and test-harness configurations.
Download tests need permission to bind loopback sockets and `curl` in PATH.
Gateway unit tests run on macOS/Linux (the gateway uses POSIX file permissions), use temporary accounts and invoke the real handler without
opening sockets or starting WebSocket/VNC services.

## Native container account checks

Container account isolation uses disposable profiles and a local HTTP fixture:

```sh
python3 tests/containers_smoke.py --binary target/debug/wega
```

Build both the desktop and worker first. The test verifies the actual selected
engine, ordinary/HttpOnly cookies, persistent localStorage, per-window
sessionStorage, simultaneous accounts and existing-window handoff. The native
smoke suite also exercises the default menu and create/edit/remove controls.
See [native containers](containers.md).

## Coverage

```sh
cargo install cargo-llvm-cov --version 0.9.1 --locked
# llvm-tools-preview is included in the pinned toolchain.
PYTHON=.venv/bin/python sh scripts/coverage.sh
```

Outputs:

| Artifact | Path |
| --- | --- |
| Rust HTML | `target/coverage/rust/html/index.html` |
| Rust LCOV | `target/coverage/rust/lcov.info` |
| Rust JSON | `target/coverage/rust/coverage.json` |
| Gateway HTML | `target/coverage/python/html/index.html` |
| Gateway XML / JSON | `target/coverage/python/coverage.xml`, `coverage.json` |

Rust coverage measures the default-free core and download service. Test modules
are in separate `*_tests.rs` sources, excluded by cargo-llvm-cov. Vendored engine,
UI, macOS APIs and GStreamer are outside that percentage. Python coverage includes
the gateway's unexecuted process/CLI paths; it does not hide them to inflate the
result. Python's percentage combines statements and branches. These two figures
are separate metrics and must not be averaged into a whole-browser percentage.

`coverage-baseline.json` retains the existing component regression gates: 95%
Rust core lines and 100% gateway lines. The separate whole-project completion
gate requires **100% lines**, as requested. Exact covered and total counts are
compared; a displayed rounded 100% cannot pass with a missed line. Branches remain
informational. Passing the component regression gates does not establish the
whole-project target. Do not lower thresholds or add exclusions to conceal gaps.

For real desktop coverage (unit tests plus all native suites):

```sh
python3 scripts/desktop-coverage.py
python3 scripts/desktop-coverage.py --report-only
```

The compiler wrapper is compiled from `scripts/coverage-rustc.rs` for the host,
including Windows; it does not require a POSIX shell on Windows. Use
`--core-only --output target/coverage/builds/<platform>-core` for a separate core
measurement. Required desktop configurations still need their own native runs.

Reports are in `target/coverage/desktop/`. A local `objects/` snapshot preserves
the instrumented executable when a normal build replaces `target/debug/wega`.
It is intentionally excluded from CI artifact uploads because of its size. HTML,
JSON and LCOV remain standalone review artifacts. Darwin continuous profiles retain
counters from terminated browser processes; see the [Rust instrumentation docs](https://doc.rust-lang.org/rustc/instrument-coverage.html)
and [LLVM continuous coverage documentation](https://clang.llvm.org/docs/SourceBasedCodeCoverage.html).

The smoke suite clicks the real Refresh toolbar button and waits for a new,
fully loaded document before checking that the counter and input reset. Its HTTP
fixture disables caching and delays subsequent interaction-page responses by
1.2 seconds, exercising reloads beyond the former fixed 650 ms wait. The overall
90-second smoke deadline still fails a missed click or a reload that never finishes.
When a suite fails, coverage export retains the failed measurement status and
prints the failed suite and native log directory after the coverage table.

For optional, informational Rust core branch measurement:

```sh
rustup component add llvm-tools-preview --toolchain nightly
RUSTUP_TOOLCHAIN=nightly WEGA_COVERAGE_BRANCH=1 python3 scripts/desktop-coverage.py --core-only --output target/coverage/core-branches
```

For all seven standalone JavaScript files (Node 26, no npm dependencies):

```sh
sh scripts/javascript-coverage.sh
```

This generates `target/coverage/javascript/lcov.info` and enforces 100% lines for
loaded production JavaScript. The inventory catches files that were not loaded.

For the whole-project audit and strict completion gate:

```sh
python3 scripts/coverage-inventory.py
python3 scripts/coverage-inventory.py --require-complete
```

For production shell scripts, run the isolated Linux contract suite with actual
kcov counters (build the base application image first):

```sh
docker build -f docker/coverage.Dockerfile --build-arg WEGA_BASE=wega:local -t wega-coverage-tools:local .
mkdir -p target/coverage/shell
docker run --rm -v "$PWD:/source:ro" -v "$PWD/target/coverage/shell:/output" wega-coverage-tools:local python3 /source/tests/shell_contracts.py --source /source --output /output
```

These contracts execute entrypoint/startup and packaging scripts in a disposable
filesystem. Compiler, media bundler and long-running service processes are test
ports; this does not certify the resulting native application packages. The
inventory rejects failed suites, missing manifests and mismatched source hashes.
See [coverage progress](coverage-progress.md) for current evidence and gaps.

`coverage-policy.json` enumerates source patterns, explicit test/dependency
classifications and required builds. Missing measurements never count as covered.
The audit writes `target/coverage/inventory.md` and `inventory.json`, including
uncovered line numbers. The strict gate currently fails. Additional platform
report directories can be placed under `target/coverage/builds/`; each needs
`measurement.json`, `coverage.json` and `lcov.info` from that same source revision.

Python's raw HTML also includes test infrastructure and branch information. Use
its JSON statement counters or the inventory for the requested production **line**
metric. All production Python utilities are at 100% lines; a partial branch count
is not a line-coverage failure. Coverage percentages are scope-specific and must
not be averaged into a whole-browser percentage.

## Native engine verification

```sh
cargo build --locked --features test-harness
python3 scripts/native-tests.py
# Select suites while keeping their isolated servers/profiles:
python3 scripts/native-tests.py --suite capabilities toolbox
```

The runner uses OS-assigned loopback ports, fresh profiles, a timeout for each
process and retained logs/screenshots under `target/native-tests/`. Its
`results.json` records the latest result for each suite, including failures.
Suites cover smoke interaction, six themes, capabilities, developer tools,
downloads, WebDriver and actual muted audio/video playback. On Linux, run within
a graphical environment or Xvfb. On macOS, native suites need desktop access and
the media SDK. This does not validate physical devices, OEM behavior or every
website supported by Servo.

## Docker verification

Production images omit the harness. Docker integration scripts that use native
test flags must use a test image built with `BUILD_FEATURES=test-harness`.
Use the dedicated Compose file, which has no normal profile/download mounts:

```sh
export COMPOSE_FILE=compose.quality.yaml
export COMPOSE_PROJECT_NAME=wega-quality
export WEGA_TEST_PASSWORD="$(openssl rand -hex 24)"
docker compose build
docker compose up -d --wait
sh scripts/docker-account-test.sh
sh scripts/docker-smoke.sh
sh scripts/docker-toolbox-test.sh
sh scripts/docker-download-test.sh
docker compose down -v
```

The account suite checks actual HTTP and WebSocket behavior, including revoking
existing control connections after credential changes. The Docker workflow runs
on manual dispatch and weekly, on Linux AMD64. macOS desktop and cross-platform
core jobs run on pushes and pull requests. A scheduled Linux job is not evidence
of an ARM64 runtime pass; inspect its uploaded logs.

## Current validation

See `docs/quality-report.md` for the measured local results and boundaries. CI
configuration will execute after the repository is pushed; it has not run on a
remote GitHub runner during this local implementation.

GitHub actions are pinned to resolved v6 commit IDs. The official release notes
for [checkout](https://github.com/actions/checkout/releases/tag/v6.0.0),
[setup-python](https://github.com/actions/setup-python/releases/tag/v6.0.0) and
[upload-artifact](https://github.com/actions/upload-artifact/releases/tag/v6.0.0)
describe their runner requirements. Re-resolve and review pins when updating actions.

### Report freshness

Rust measurement manifests include a hash of first-party Rust files and Cargo
configuration. `desktop-coverage.py --report-only` refuses changed/unverifiable
sources. The whole-project inventory rejects stale manifests and empty platform
reports, even if their recorded counters say 100%. Regenerate measurements after
source changes. Native driver results and missing platforms remain separate
requirements from line coverage.


## UI/UX regressions

The native capabilities suite now verifies prompt defaults, explicitly empty
prompt values, Escape cancellation, live internal-page theme updates, new-tab
search and the 720×480 library overlay. Toolbar semantic labels and AccessKit
schema conversion have Rust tests. Keyboard users can use F6 to move between the
address bar and page.

The native engine suite also clicks New tab in both tab layouts while redraw owns
the GUI outside Browser, checks the deferred focus transfer, and retains the source
Wega worker. This reproduces the macOS click-only panic that shortcut tests missed.
See [trak.in and new-tab fixes](trak-and-new-tab-fixes.md) for before/after evidence
and the remaining live-site script limit.

`sh scripts/javascript-coverage.sh` also measures `site/site.js` at 100% lines and
runs its clipboard success/failure checks. Docker viewer tests cover viewport
modes and upload behavior. Python tests cover upload authentication, CSRF,
filename rejection, limits, private storage, collisions and publication failures.
`python3 scripts/docker-ux-test.py IMAGE` verifies the gateway and nginx path in a
disposable container; see [Docker documentation](docker.md).

See [UI/UX implementation](ui-ux-implementation.md) for the item-by-item changes,
current measurements and manual OS/assistive-technology validation boundaries.
