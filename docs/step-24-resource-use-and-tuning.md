# Step 24 Contract: Measure and Tune Resource Use Without Losing Behavior

## 1. Overview & Objectives

Step 24 benchmarks, analyzes, and tunes application and worker process-tree resource consumption without sacrificing web API behavior, correctness, or browser lifecycle stability:

- **Isolated Process-Tree Resource Benchmarking (`scripts/browser-bench.py`)**:
  - Measures combined RSS resident working set and active/idle CPU of the browser plus all descendant content worker processes.
  - Covers clean startup, 1-tab, 5-tab, and 10-tab workloads with restored session profiles and W3C WebDriver automation.
  - Evaluates bounded web capability probes (`article`, `dom`, `timer`, `events`, `forms`, `storage`, `fetch`) ensuring standard compatibility is verified alongside resource tracking.

- **Supervisor Worker Quota & Memory Scaling (`engines/platform/src/supervision.rs`)**:
  - Scaled default supervisor worker capacity from 8 to 32 concurrent processes to support standard multi-tab browsing without artificial capacity rejections.
  - Scaled aggregate resident memory budget to 8 GiB and aggregate frame reservation budget to 2 GiB to comfortably host 10+ tabs under debug and release configurations.

- **Non-Loaded Tab State Safety & Navigation Resilience (`src/browser.rs`)**:
  - Fixed `can_go_back` and `can_go_forward` on `Browser` to safely query Wega page state (`self.tab().wega.as_ref().map_or(false, ...)`) instead of falling through to Servo's `.live()`/`.view()` methods which panicked when restored tabs were still pending initial load.
  - Defensively safeguarded `dispatch_navigation` to gracefully handle cases where a view is absent rather than unwrapping.

- **Native Engine Test Harness Synchronization (`src/harness/engines.rs`)**:
  - Updated new-tab render contract to assert Wega as default engine (`assert!(self.is_wega())`).
  - Aligned UI ComboBox labels to match `"Engine: Wega"` and `"Servo (legacy)"` options.
  - Updated session save assertions to expect Wega default across all tabs.
  - Verified that all 17 stages of `scripts/native-tests.py --suite engine` pass cleanly.

---

## 2. Benchmark Measurements & Results

### Compatibility Probes (`scripts/browser-bench.py --mode compatibility`)

| Engine | Probe | Result |
| :--- | :--- | :--- |
| wega | article | **PASS** |
| wega | dom | **PASS** |
| wega | timer | **PASS** |
| wega | events | **PASS** |
| wega | forms | **PASS** |
| wega | storage | **PASS** |
| wega | fetch | **PASS** |

### Process-Tree Resource Utilization (`scripts/browser-bench.py --tabs 1 5 10`)

| Engine | Tabs | Runs | Median Idle RSS (MiB) | Max Sampled RSS (MiB) | Median Idle CPU (%) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| wega | 1 | 1 | 307.9 | 310.3 | 131.2 |
| wega | 5 | 1 | 620.9 | 623.2 | 130.8 |
| wega | 10 | 1 | 1006.3 | 1008.7 | 131.3 |

*Note: Measurements reflect debug Mach-O arm64 binaries with full symbol tables, address sanitization guards, and test harnesses. Shared memory pages are counted per process tree as standard RSS.*

---

## 3. Verification & Test Suite

1. **Browser Benchmark Suite (`scripts/browser-bench.py`)**:
   - `python3 scripts/browser-bench.py --binary target/debug/wega --engine wega --mode all --tabs 1 5 10 --repeats 1 --idle-seconds 5`
   - Complete: True. 7/7 compatibility probes pass. 1, 5, and 10 tab workloads measured with 0 errors.

2. **Benchmark Unit Tests (`tests/test_browser_bench.py`)**:
   - `python3 tests/test_browser_bench.py`: 3/3 tests pass (process tree sample recursion, CPU parsing, required probe assertions).

3. **Native Engine Integration Test (`scripts/native-tests.py --suite engine`)**:
   - `WEGA_UNSAFE_DEVELOPMENT_WORKER=1 WEGA_TEST_ALLOW_LOOPBACK=1 python3 scripts/native-tests.py --suite engine`
   - PASSED: Stage 0 through Stage 17 complete with exit code 0.

4. **WebDriver Automation Suite (`scripts/native-tests.py --suite webdriver`)**:
   - PASSED 100%.

5. **Wega Unit and Core Tests**:
   - `cargo test --bin wega`: 50 passed, 0 failed.
   - `cargo test --lib`: 111 passed, 0 failed.
   - `cargo test --manifest-path engines/Cargo.toml -p wega-platform`: 25 passed, 0 failed.

6. **Compiler Diagnostics**:
   - `cargo check --bin wega`: 0 errors, 0 warnings.
