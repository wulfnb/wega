# Reliability, compatibility and release baseline

Checkpoint: 25 October 2026. Source changes remain local until committed and
pushed. Published-release evidence below refers to `ab9b2b9`, not these changes.

## Unsaved page state

Updated 27 October 2026: manual close, unload, engine switch and quit now ask
for confirmation only when a page reports edits, or its state check fails or
times out. Clean loaded tabs close directly. Wega applies the same check to
navigation, reload and history traversal; Servo retains its navigation responder.
Checks are asynchronous, have request IDs and a two-second conservative timeout,
and follow stable tab identities. Cancel/Escape keeps the page. A quit checks all
loaded documents before deciding whether a prompt is needed.

Both engines track input/change events for text fields, textareas, checkboxes,
radio buttons, selects and editable content. Reverting a value or resetting a form
clears that edit; cancelled resets retain it. Tracking resets on document
replacement. Servo reattaches the tracker at head/load completion, because its
initial user script can belong to the previous document. Wega uses a dedicated
state-check IPC response so automation evaluations cannot consume the result.

This is best-effort edit detection, not an acknowledged engine close handshake.
Arbitrary application state, saved-state acknowledgements and cross-origin frame
editors are not fully observable. A live tab's DOM does not survive a crash,
forced termination or OS shutdown. Automatic idle unloading remains restricted to
browser-owned internal documents; website/local tabs remain loaded, and restored
tabs still load lazily.

Native regressions preserve textarea/native-input values after cancelled close,
quit, navigation, switch and unload, follow a reordered tab's identity, and close
ten loaded clean Servo tabs without confirmations. Engine tests cover native IME,
reverts, checkboxes, editable content, form reset and navigation reset.

## Diagnostic logs

Logs are enabled at startup, including Finder launches. **Menu → Open browser
logs** opens the profile's `logs/` folder. The active file is `browser.jsonl`;
three numbered backups retain earlier output, with a five-MiB limit per file.
Each JSON line includes a UTC timestamp, process ID, source/worker ID and message.
The browser records console messages, compatibility reports, notices, worker
stderr and (on Unix) native browser stderr. The worker reports JavaScript errors
before its short UI-status queue is capped. It does not record IPC evaluation
commands or keystrokes. Website console messages and errors can contain private
page data; logs belong to the private profile.

CPU/RAM is visible by default, including existing profiles after a one-time
migration. Later opt-outs in Settings persist. The browser counter excludes
separate content workers; Wega displays the active worker's RAM/JS heap separately.
Both update every two seconds. Worker node/heap updates are also sampled, avoiding
continuous IPC and UI wakeups as V8's heap fluctuates.

## Repeatable measurements

Use a production release for performance comparisons. Development builds and
different OS/GPU configurations must be reported separately.

```sh
python3 scripts/browser-bench.py --binary /absolute/path/to/wega \
  --output target/baseline-new-run --label 'optimized release COMMIT' \
  --repeats 3 --tabs 1 5 10 --idle-seconds 10

python3 scripts/browser-bench.py --binary /absolute/path/to/wega \
  --output target/live-new-run --mode compatibility --live
```

The output directory must be new. No personal profile is opened. The runner uses
disposable profiles and a loopback HTTP fixture; resource runs explicitly select
every restored tab and wait for its expected title and load completion. This
avoids measuring ten placeholders as ten loaded pages. There is a two-second
settling period before idle samples. Engine order alternates between repeats.

`results.json` records platform, binary/worker/fixture hashes, working-tree
context, raw process samples, startup-to-all-tabs-ready latency, peak sampled
RSS, median idle RSS and idle CPU. `summary.md` groups repeated runs. Changed
binaries/workers, missing samples, failed readiness or required probes fail the
run. The runner never rebuilds a binary itself.

RSS includes the browser and every observed descendant, including per-tab Wega
workers. Shared resident pages can be counted more than once; RSS is not unique
physical memory. CPU 100% represents one fully occupied core. Sampling is roughly
250 ms plus collection cost, and can miss brief peaks or very short-lived children.
Startup means document/title readiness, not a measured first-paint event. The
fixture is a deterministic text article, not a representative sample of all sites.
OS caches, display scaling and background host activity are not controlled.

Local compatibility probes cover article rendering, DOM mutation, timers, click
events, FormData, local storage and fetch. Servo must pass all seven. Wega must
pass its three currently supported probes; unsupported exploratory cases remain
visible and are not represented as successes. `--live` adds public reference-site
captures; repeat `--live-url URL` to choose specific sites. Capturing a screenshot
and title does not certify login, uploads, media or interactive functionality.
The click probe tests programmatic `HTMLElement.click()`; Wega's native mouse
click/listener integration is exercised separately and already works.

## Local verification

The final affected native suites were rerun after the last runtime changes.
Earlier failed intermediate runs remain in their individual profile logs; the
suite result manifest contains the final passing runs.

| Check | Result |
| --- | --- |
| Default-free instrumented core | 82 tests pass; 1695/1778 lines, 95.3318%, passing the existing 95% gate |
| Rust with test harness | 84 library + 34 desktop + 2 allocation tests pass |
| Python tooling | 65 tests pass; gateway 443/443 lines, passing the existing 100% gate |
| Native integration | All nine suites pass; smoke, capabilities, WebDriver and engine suites rerun after final runtime changes |
| Rust formatting and strict production/harness Clippy | Pass; existing vendored Servo and native linker warnings remain |
| Workflow lint | actionlint passes |
| Updated macOS debug package | Bundling and strict signature verification pass; Servo passes seven packaged probes and Wega passes its three required probes, with four unsupported exploratory cases retained |
| macOS Launch Services | Cold URL delivery, running-instance URL batches and Unicode/space HTML-file handoff pass against the updated production binary |
| Linux benchmark runner | Local tooling tests, all required engine probes and four resource runs pass in the cached ARM64 image; this is not a rebuild of today's Linux application |

The whole-project 100% line target is still incomplete. Benchmark tooling is a
test driver, classified with the existing native runners; production coverage
thresholds have not been reduced. Native OS assistive-technology/device tasks
and fresh Windows/Linux runtime checks for these edits are not certified here.

## Release validation

The existing immutable release `test-preview-ab9b2b9385ef` passed
[GitHub's release run](https://github.com/wulfnb/wega/actions/runs/40115706369).
Verified jobs include macOS packaging, Linux/Docker integration and packaging,
Windows desktop launch, packaged media and URL/file handoff, and Windows installer
install/uninstall. These results supersede older notes saying that this commit's
remote Windows/release checks had not run. They do not verify today's local edits.

The downloaded macOS DMG was checked against its published SHA256 checksum and
mounted read-only. Both executables, license notices, `/Applications` link and
readme are present. Deep strict signature verification passes. It is ad-hoc
signed, with no TeamIdentifier; Gatekeeper rejects it. This Mac has no valid
Developer ID signing identity, so notarized distribution remains blocked by
signing credentials and notarization setup.

CI now runs both-engine compatibility and process-tree baselines on macOS,
Windows and the Linux container, and uploads their reports. All three release
jobs additionally exercise both engines from the packaged app/directory or
extracted Linux archive. macOS/Windows remove worker and media path overrides.
Those new workflow steps still need a pushed
commit and successful remote run. No release is published by the local runner.
Docker publication now follows the Linux archive compatibility checks and
artifact upload, so those checks can fail before moving the public image tags.

## Evidence and next engine work

Local raw artifacts are under `target/milestone-native-final/`,
`target/release-compatibility-ab9b2b9/`, `target/release-baseline-ab9b2b9/`,
`target/browser-bench-linux/` and `target/release-resources-ab9b2b9/`.
Updated-package evidence is in `target/milestone-package.log`,
`target/milestone-packaged-compatibility/` and `target/milestone-macos-launch/`.
The earlier `target/browser-bench-2026-10-25/` development run overlapped binary
rebuilds and is exploratory only; do not use it as the final performance baseline.

The published binary renders Example.com and Wikipedia in both engines. Inspected
Wikipedia captures show a substantial Wega layout difference: its contents list
occupies much of the first viewport, pushing the article down. Servo shows the
article in that viewport. This is rendering evidence, not interaction certification.

### Optimized macOS ARM64 resource results

Three repetitions per engine/tab count, ten idle seconds after a two-second
settling period. All tabs were loaded; the Wega ten-tab samples contained the
browser plus ten workers. Binary and worker hashes remained unchanged. Each
group below is the median of the three per-run idle medians; CPU is likewise
the median per-run value. Background OS activity/caches were uncontrolled and
RSS varied across runs, so these are local baselines, not certified budgets.

| Engine | Tabs | Idle tree RSS (MiB) | Idle tree CPU (%) |
| --- | --- | --- | --- |
| Servo | 1 | 152.5 | 0.9 |
| Servo | 5 | 176.2 | 0.9 |
| Servo | 10 | 197.7 | 0.4 |
| Wega | 1 | 177.5 | 0.4 |
| Wega | 5 | 319.8 | 0.5 |
| Wega | 10 | 355.9 | 0.6 |

Maximum sampled tree RSS across repetitions was 245.3 MiB for ten Servo tabs
and 582.8 MiB for ten Wega tabs. Both engines were mostly idle on this fixture;
Wega does not demonstrate a memory saving here. The smaller JS heap is not a
proxy for the complete renderer's footprint.

Prioritize process/renderer memory scaling and layout differences before
expanding the experimental engine's default-site scope. Then tackle event/form
behavior, fetch and persistent storage/sign-in, with explicit fixtures and
per-site evidence. Keep Servo the everyday default.
