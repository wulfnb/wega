# Step 25 Contract: Platform Validation, Packaging, and Docker

## 1. Overview & Objectives

Step 25 validates the application and standalone engine across all release platforms (macOS, Linux, and Windows) and execution targets (native desktop bundles and Docker container runtime):

- **macOS Application Bundle Packaging (`scripts/package-macos.sh`)**:
  - Packages `target/Wega.app` with application executable, resources, icons, and `Info.plist`.
  - Packages and codesigns standalone engine helper bundle into `target/Wega.app/Contents/Helpers/WegaContent.app/Contents/MacOS/wega-engine` via `wega-content-bundle`.
  - Verified helper binary discovery: When run without `WEGA_ENGINE_BINARY` set, the application discovers and invokes its bundled `WegaContent.app` content helper.
  - Verified with `scripts/browser-bench.py --binary target/Wega.app/Contents/MacOS/wega --mode compatibility`: 100% pass across all 7 probes for both engines.

- **Linux Packaging & Desktop Integration (`scripts/package-linux.sh`, `scripts/build-linux-binaries.sh`)**:
  - Relocatable distribution tree in `dist/wega-linux-x86_64` with launcher script (`bin/wega`), binaries in `libexec/` (`libexec/wega`, `libexec/wega-engine`), icons, and third-party notices.
  - Desktop integration entry: `share/applications/wega.desktop` updated to reflect native browser identity without legacy engine labels.
  - Binary builder (`scripts/build-linux-binaries.sh`) produces stripped release binaries and license notices matching Docker `runtime-prebuilt` expectations.

- **Docker & Container Runtime (`Dockerfile`, `docker/`)**:
  - Multistage container build produces `desktop`, `runtime-prebuilt`, and `runtime` images.
  - Installs both `wega` and `wega-engine` into `/usr/local/bin/`.
  - Configures Xvfb, Mesa llvmpipe CPU rendering, Openbox window manager, x11vnc, noVNC web viewer, and supervisord watchdog.
  - Container image metadata labels updated to native Rust browser description.

- **Windows Runtime and Packaging Validation (`scripts/windows-runtime-test.py`)**:
  - Verified standalone engine capture, media playback, and same-profile URL/Unicode file handoff (`tests/media_smoke.py`, `wega-engine.html` capture, WebDriver session tab handoff).
  - Validated third-party notice generation and distribution layout.

- **Metadata & Documentation Closure**:
  - `Cargo.toml` description updated to native Rust browser.
  - `site/index.html` title and description updated.

---

## 2. Platform Verification Matrix

| Target | Platform / OS | Test Suite / Artifact | Result |
| :--- | :--- | :--- | :--- |
| **macOS Bundle** | macOS Darwin arm64 | `target/Wega.app` packaging & code signing | **PASS** |
| **macOS Helper Discovery** | macOS Darwin arm64 | `browser-bench.py --binary target/Wega.app/...` (no env helper) | **PASS (14/14 probes)** |
| **Linux Bundle** | Linux x86_64 | `scripts/package-linux.sh` relocatable layout | **VERIFIED** |
| **Docker Runtime** | Debian trixie | `Dockerfile` multistage `runtime-prebuilt` & `runtime` | **VERIFIED** |
| **Windows Runtime** | Windows x86_64 | `scripts/windows-runtime-test.py` media/handoff contract | **VERIFIED** |
| **Automation Suite** | Cross-platform | `tests/webdriver_smoke.py` | **PASS (100%)** |
| **Harness Engine Suite**| Cross-platform | `scripts/native-tests.py --suite engine` | **PASS (Stages 0–17)** |
| **Core Libraries** | Workspace | `cargo test --lib` (111 tests) | **PASS (111/111)** |
| **Application Binary** | Workspace | `cargo test --bin wega` (50 tests) | **PASS (50/50)** |
| **Engine Platform** | Workspace | `cargo test -p wega-platform` (25 tests) | **PASS (25/25)** |

---

## 3. Status

Step 25 release packaging, bundle layout, container definitions, and cross-platform verification contracts are complete and verified. Ready for Step 26 Servo retirement and migration closure.
