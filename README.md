# Wega Browser

Wega is an experimental native Rust browser with movable top or sidebar tabs and a standalone content worker per loaded web tab.

The embedded Servo browser backend has been removed. Wega uses Blitz DOM, Stylo CSS, V8 through Deno, and Skia rendering. Stylo and a few pinned WebIDL reference files retain their upstream Servo origins and licenses; they do not include the Servo browser runtime or SpiderMonkey.

Local standalone validation is recorded in [the removal checkpoint](docs/servo-removal-checkpoint.md), together with the remaining platform and release checks.

## Build and run

Use the pinned toolchain in rust-toolchain.toml.

    cargo build --locked
    python3 scripts/build-wega-engine.py --debug
    cargo run --locked -- --profile-dir /tmp/wega-profile https://example.com

The wega-engine worker must be installed beside the desktop executable, inside the packaged application helper location, or selected with WEGA_ENGINE_BINARY. A missing worker reports an error; there is no alternate engine fallback.

The shell and worker have separate Cargo workspaces and lockfiles. Native build dependencies for Linux are listed in docker/builder-packages.txt.

    scripts/package-macos.sh --debug
    docker compose up --build

## Native browser features

Tabs can be moved between the top strip and sidebar. Profiles preserve open addresses, titles, history, bookmarks, themes, and container identities. Containers keep website storage separate. Built-in new-tab, history, bookmarks, settings, and about pages render in the native UI.

The standalone worker handles page input, JavaScript, navigation, clipboard, find, file selection, accessibility events, and document dialogs through the bounded engine protocol. Downloads use the host service and the active worker's cookie jar.

Existing profiles saved with the retired backend migrate their engine identity to Wega. The old engine choice UI and --engine servo command are gone. --engine wega and the legacy --wega-only alias remain accepted for launch-script compatibility.

## Current limitations

This is an alpha preview, not a production browser. Compatibility is limited by Wega's own web APIs and rendering implementation. A page that cannot run stays in Wega and reports its failure or unsupported capability.

Real audio/video decoding is not equivalent to the removed backend. The standalone media module currently parses media and models playback state; its generated video frames do not establish codec decoding. The old codec integration test remains explicitly selectable with --suite media and is excluded from standalone acceptance until that implementation is complete.

The native developer panel currently shows worker process and document state. The former Servo-specific inspector, debugger, network event bridge, and richer toolbox panels have been removed. WebDriver retains the standalone implementations for navigation, JavaScript evaluation, page source, screenshots, and resource reports; unsupported commands return an explicit error.

The old theme, capability, and toolbox native test modes are retired and return an error. --engine-test and --smoke-test exercise the standalone worker. --download-test verifies a real page-triggered multipart download and HttpOnly cookie reuse. These smaller scenarios do not replace the old suites' coverage.

## Validation

    cargo fmt --check
    cargo clippy --locked --no-default-features --all-targets -- -D warnings
    cargo check --locked --all-targets --features test-harness
    cargo test --locked --no-default-features --lib
    cargo test --locked --bin wega
    cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- check
    cargo test --locked --manifest-path engines/Cargo.toml --all-targets -- --test-threads=1

Native tests require an isolated profile and actual platform display. Test fixtures on loopback require WEGA_TEST_ALLOW_LOOPBACK=1 in a debug or test-harness build. Source checks do not establish packaged, cross-platform, or hosted CI success.

## Source and licenses

Wega is licensed under MPL-2.0. See [LICENSE](LICENSE), [NOTICE](NOTICE), and the generated distribution notices for third-party licenses. Modified worker sources and patches are recorded in engines/vendor/sources.json.

Servo is a trademark of the Linux Foundation. Wega is independent and is not affiliated with or endorsed by the Linux Foundation or Mozilla.

[Historical embedded-backend documentation](docs/history/embedded-servo-readme.md) describes the retired implementation and is retained for provenance.
