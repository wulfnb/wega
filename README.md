# Wega Browser (Alpha Test Release)

*Wega* is the German name of Vega, the bright star in the constellation Lyra.
It was the pole star around 12,000 BC and will be again around 13,700 AD.

A native Rust desktop browser engineered in Germany that embeds the open-source **Servo 0.7.0** web engine. Websites are fetched,
parsed, styled, painted, and executed by Servo. The interface uses egui 0.37 and
winit. Servo's WebRender and the browser chrome share a Surfman/OpenGL context;
each tab renders into its own offscreen framebuffer, composited into the window.

The browser chrome uses Wega's own emerald theme. It is an independent design and
contains no Firefox code or artwork. Tabs can be
placed above the address bar or in a resizable sidebar and dragged to reorder.
Changing layouts keeps the existing webviews and their page state alive.

## Download

[Website](https://wulfnb.github.io/wega/) · [All releases](https://github.com/wulfnb/wega/releases) · [SHA256 checksums](https://github.com/wulfnb/wega/releases/download/test-preview/SHA256SUMS.txt)

| Platform | Download Alpha Preview |
| --- | --- |
| macOS · Apple Silicon (ARM64) | [Download DMG](https://github.com/wulfnb/wega/releases/download/test-preview/Wega-macOS-arm64.dmg) · [Download ZIP](https://github.com/wulfnb/wega/releases/download/test-preview/Wega-macOS-arm64.zip) |
| Windows · x64 | [Download ZIP](https://github.com/wulfnb/wega/releases/download/test-preview/Wega-Windows-x64.zip) |
| Linux · x86_64 | [Download tar.gz](https://github.com/wulfnb/wega/releases/download/test-preview/Wega-Linux-x86_64.tar.gz) |

The currently published Windows preview does not include video or audio playback.
The next Windows build enables bundled media; packaged playback is checked in Windows CI before release.

These links download the current `test-preview` prerelease. Linux setup requirements
are included in the archive's README.

On macOS, open the DMG and drag **Wega.app** into **Applications**, then launch
the installed app from Applications. The alpha is ad-hoc signed and **not
notarized by Apple**, so macOS may require you to approve that specific app in
**System Settings → Privacy & Security → Open Anyway** after trying to open it.
You do not need a paid Apple developer membership to run it. See the
[macOS installation instructions](docs/install-macos.txt). The ZIP download is
also available if the DMG layout is inconvenient.

> ### ⚠️ Alpha Test Notice & Legal Disclaimer / Haftungsausschluss
>
> **This is an experimental Alpha preview (`v0.1.0-alpha.1`) intended strictly for testing, developer evaluation, and feedback.**
>
> 1. **Experimental Engine**: Wega embeds the Servo web engine. Websites are rendered and executed using an engine that is under continuous active open-source development and does not yet implement all web security standards or process isolation mechanisms found in production browsers.
> 2. **User Responsibility**: Wega is **not** intended for security-sensitive activities such as online banking, financial transactions, or critical personal data. Any use of this experimental software is at your own risk (*Nutzung auf eigene Gefahr*).
> 3. **Limitation of Liability (German Law)**: This software is provided free of charge (*unentgeltlich*). Pursuant to § 521 of the German Civil Code (BGB), liability of the developers and contributors is limited to intent (*Vorsatz*) and gross negligence (*grobe Fahrlässigkeit*).
> 4. **Trademark Notice**: Servo is a trademark of the Linux Foundation. Wega is an independent project and is not affiliated with or endorsed by the Linux Foundation or Mozilla.

## Why Wega? (Motivation)

Modern web browsing has largely converged into a monoculture of memory-hungry, corporate-controlled browsers. Wega was born out of three immediate personal needs and a broader architectural vision:

1. **Turning the Servo Engine into a Real Everyday Browser**:
   Servo is one of the most exciting achievements in modern systems programming—a ground-up parallel web engine written in Rust. Yet for years, it has existed primarily as an embedded research library or tech demo. Wega takes Servo and builds an actual, usable desktop browser around it: complete with movable tabs, bookmarks, history, developer tools, download managers, and window management.
2. **The Ideal Lightweight Homelab Browser**:
   In homelab and self-hosted environments, accessing internal dashboards, web services, and the internet directly from the server typically requires running heavy remote desktop environments (like XFCE + Chrome) that consume gigabytes of RAM. Wega was designed to run seamlessly in a lightweight Docker container with built-in authenticated HTTPS, noVNC streaming, and minimal server overhead.
3. **Escaping RAM Bloat & System Freezes**:
   Mainstream browsers frequently spawn dozens of background helper processes and consume 4–8 GB of RAM for only a few tabs, causing system stuttering, thermal throttling, and freezes on resource-constrained systems. Wega leverages Rust's zero-cost abstractions, egui's immediate-mode GUI, and Servo's parallel WebRender pipeline to maintain a lean, predictable memory footprint.

### Why This Project Matters for the Web

* **Breaking the Chromium Monoculture**: With almost every modern browser (Chrome, Edge, Brave, Opera, Vivaldi, Arc) now relying on Google's Blink engine, web standards are increasingly dictated by a single advertising company. Fostering a viable Servo-based browser is crucial for long-term open web diversity.
* **Radical Privacy with Zero Telemetry**: Modern commercial browsers constantly phone home with user profiling, diagnostic metrics, and search telemetry. Wega has zero telemetry, zero analytics tracking, and no hidden network calls.
* **More Memory Safety, Honestly Scoped**: Many historical browser vulnerabilities stem from C/C++ memory corruption (use-after-free, buffer overruns). Wega's own code is Rust: the platform-independent library crate is compiled with `#![forbid(unsafe_code)]`, and the desktop binary keeps its `unsafe` to a few small, reviewed blocks (OpenGL context, AppKit/macOS APIs, native media, process resource queries and environment setup before threads start). Memory safety is **not** end-to-end, though: the Servo engine embeds SpiderMonkey (C++ with a JIT compiler) and GStreamer's C media stack, and by default everything runs in a single process without a sandbox. A bug in those components can still compromise the browser.

## Run

Install Rust 1.95 or newer with rustup and the native development tools for your OS. On macOS,
Xcode Command Line Tools and `brew install pkgconf gstreamer libnice-gstreamer` are required. The first compilation builds a substantial
web engine and downloads native engine components; allow time and disk space.

```sh
cargo run
# Open a particular website:
cargo run -- https://www.google.com/
# Use a separate browser profile:
cargo run -- --profile-dir /tmp/wega-profile https://example.com/
```

On macOS, `scripts/package-macos.sh --debug` creates `target/Wega.app`
from the debug build. Run the script without `--debug` for an optimized app bundle.

`cargo run --release` builds an optimized browser. Dependencies are locked in
`Cargo.lock`; use `--locked` for reproducible builds.

The desktop app is developed on macOS. Linux ARM64 has also passed the native
interaction and developer-tools integration tests in Docker. The Dockerfile
supports AMD64 builds, but that architecture has not been runtime verified here.
See the [Servo embedding guide](https://book.servo.org/embedding/overview.html).

## Multi-account containers

**Containers** is available by default beside the engine badge. Open Personal,
Work, Banking or Shopping to keep separate sign-ins, or add your own names and
colors in **Manage containers…**. **Open copy of this page in…** retains the
original tab and opens its URL in the selected account space.

Containers currently use **separate windows and isolated engine profiles** for
both Servo and Wega. Every tab in a window shares that container. This is a native
feature; Firefox's add-on is not installed. See [isolation, limitations and tests](docs/containers.md).

## Docker

Run the [published Docker image](https://github.com/wulfnb/wega/pkgs/container/wega) through the HTTPS viewer with a persistent profile:

```sh
docker run -d \
  --name wega-browser \
  -p 127.0.0.1:7443:7443 \
  -e BROWSER_USER=servo \
  -e BROWSER_PASSWORD='choose-a-strong-password' \
  -v wega-profile:/config \
  ghcr.io/wulfnb/wega:latest
```

Open **https://localhost:7443** and sign in as `servo` with your chosen password.

The viewer is a full remote browser session protected by a password. **Never
expose port 7443 directly to the internet.** For access from other devices, keep
it on a private network such as [Tailscale](https://tailscale.com/) or WireGuard,
or put it behind a reverse proxy you already trust for authentication and TLS.
The example binds to loopback (`127.0.0.1`), so the viewer is reachable only
from the Docker host itself.

To build and run from this repository with Docker Compose:

```sh
cp .env.docker.example .env
# Set your username, unique password and download folder in .env first.
docker compose up -d --build
```

Open **https://localhost:7443**. This standalone Debian image runs our own browser
UI; it does not include Firefox, LinuxServer or a full desktop environment.
Use **Account** in the viewer to change your username/password. After encrypting
bootstrap values, `python3 scripts/web-credentials.py show` displays the initial
login locally; changed passwords cannot be recovered. Downloads are
saved to the host folder set by `WEGA_DOWNLOAD_DIR` (default `./downloads`).
See [Docker setup and configuration](docs/docker.md) for build requirements,
authentication, resolution, logs, and the lightweight viewer's limitations.

## Browser controls

- **↓ / Downloads** opens the real download manager. Paste an HTTP(S) file URL,
  right-click a link/image to download it, or click a page's `<a download>` link.
  Choose 1–8 connections (default four), pause/resume or cancel. Multipart uses
  validated byte ranges and stable server validators; unsupported servers fall
  back to a single connection. Desktop downloads require `curl` in `PATH`.
- The small status-bar readout shows live CPU usage and RAM, updated every two
  seconds. Hover for the measurement scope and browser uptime. Desktop readings
  cover this browser process; Docker readings cover the whole container when
  cgroup v2 counters are available. CPU 100% means one fully used core.
- Drag a tab title onto another tab to change its position. Right-click a tab for
  close, move earlier/later, and layout options. Middle-click closes a tab.
- Use the layout button at the left of the address bar, or the menu, to switch
  between top tabs and sidebar tabs. Drag the sidebar edge to resize it.
- Back, forward, reload, page zoom, and the address bar operate on the live page.
  Domains use HTTPS by default; localhost addresses use HTTP. Other text becomes
  an encoded Google search query. Absolute file paths and `file://` URLs also work.
- Bookmarks and history share a searchable floating card with clickable rows. Remove
  controls appear on hover; clearing history requires confirmation. History groups
  visits by local date and shows visit times. Existing profiles remain readable;
  visits without timestamps appear under “Earlier visits”.
- The star saves/removes a bookmark. Bookmarks, recently visited pages, tab order,
  startup URLs, selected tab, sidebar width, and layout persist in the profile.
- JavaScript popups open as tabs. JavaScript dialogs, select controls, color inputs,
  file inputs (using explicit file paths), HTTP authentication, and permission
  requests have browser UI. Permission grants apply to that request only.
- F12 opens a resizable developer toolbox with ten panels, described below.

| Shortcut | Action |
| --- | --- |
| Cmd/Ctrl+L | Focus and select the address |
| Cmd/Ctrl+T | New tab |
| Cmd/Ctrl+W | Close tab |
| Cmd/Ctrl+Shift+T | Reopen the last closed URL |
| Cmd/Ctrl+Shift+L | Switch top/sidebar layout |
| Ctrl+Tab / Ctrl+Shift+Tab | Next/previous tab |
| Cmd/Ctrl+1…8 / 9 | Select tab / last tab |
| Cmd/Ctrl+R / F5 | Reload |
| Alt+Left / Alt+Right | Back/forward |
| Cmd/Ctrl+D | Toggle bookmark |
| Cmd/Ctrl+H | History |
| Cmd/Ctrl+Shift+B | Bookmarks |
| Cmd/Ctrl+plus / minus / 0 | Zoom in/out/reset |
| F12 | Toggle developer toolbox |
| Cmd/Ctrl+Shift+E | Network |
| Cmd/Ctrl+Shift+I / Cmd+Option+I | Inspector |
| Cmd/Ctrl+Shift+K | Console |

On macOS use Cmd for the shortcuts marked Cmd/Ctrl. Closing the final tab creates
an empty tab. Close the native window to exit. Reopening a closed tab restores its
URL, not its previous JavaScript heap or back/forward stack.

## Appearance

Open **… → Appearance** to choose macOS Glass (Auto), Glass Light, Glass Dark,
Midnight, Paper, or Wega · Emerald Glass. Changes apply immediately to tabs, the address bar,
library cards, menus and developer tools, and are saved per profile. Website
styles and running JavaScript are preserved.

On macOS, glass uses a real AppKit `NSVisualEffectView` behind a transparent
Surfman surface. Auto inherits system appearance; fixed Light/Dark choices also
set the native window appearance. The native material follows window activation
and macOS accessibility settings. Website pixels remain opaque. Other platforms
use opaque versions of the glass palettes. App-owned PNG exports contain the
chrome's alpha channel; the macOS compositor's desktop blur is not part of that
framebuffer export. The default Wega · Emerald Glass theme matches the selected green glass globe icon.
Older profiles keep their stored theme until a new choice is
made.

Run the native switching/compositing check with an isolated profile:

```sh
cargo run --locked --features test-harness -- --theme-test --profile-dir /tmp/wega-theme-test
```

The test exports six theme frames and checks native backdrop state, chrome and
corner alpha, opaque page pixels, persisted choices and preserved page state.

## Developer toolbox

The toolbox consumes Servo's real developer event stream through a local adapter.
It does not open a remote debugging port. Data and selection are isolated by tab;
page probes run only when requested. Use **Refresh** after navigation or a page edit.
The panels share the browser theme and dock beneath the inset page. Developer
output uses a monospace font, Network cells share fixed column edges with a
flexible waterfall, and the debugger shows unwrapped code, gutter breakpoints
and an amber paused-line highlight. Brief notices expire automatically.

> **Automation endpoints are unauthenticated.** `--webdriver-port PORT` and
> `--remote-devtools PORT` open local TCP listeners without any password or
> token. Any process or user on the same machine (including other containers
> sharing the network namespace) can then drive the browser, read pages and
> cookies, and run scripts in your profile. Enable them only for testing, with a
> separate `--profile-dir`, and never forward these ports.

| Panel | Working controls and data |
| --- | --- |
| Inspector | Expandable/searchable DOM tree (500 elements), breadcrumbs, native page element picking, matching author CSS rules, inline CSS/classes, searchable computed styles, box model, flex/grid properties and children, attribute/HTML editing, attribute edit history and undo |
| Console | Native severity/source/time/category records, JS/CSS/page errors, stack/object previews, filtering, repeat grouping, timestamps, persistence, history, multiline input, await expressions, native frame/global evaluation, paused-scope identifier suggestions, copy/export |
| Debugger | Domain-grouped sources, file/text search, go to line, executable gutter breakpoints, individual/all breakpoint toggles and removal, whole-source ignore listing, pause/step shortcuts, selectable native call stack, lexical scopes and watch expressions |
| Network | Live requests, multi-select types and query filters, sortable/resizable columns, waterfall, headers/cookies/payload/response search, TLS info, clear/pause, persist logs, cache toggle, HAR import/save/copy, cURL copying, edit/resend, domain blocking |
| Style Editor | Engine stylesheet enumeration and source text, editable per-page CSS overrides and reset |
| Performance | Actual navigation/resource timing entries, marks and measures |
| Memory | Actual Servo allocation reports across browser processes |
| Storage | Local/session storage inspection, set/delete items, page-visible cookies |
| Accessibility | Actual Servo accessibility node updates and page semantic checks |
| Application | Manifest URL/content, service-worker registrations when supported, CacheStorage names |

The toolbox’s **…** menu switches between bottom, right, left and a separate
native OS window. Docked panels resize and the page viewport adjusts to the
remaining space. The separate window shares the active tab’s live tools; closing
it closes the toolbox while keeping the browser open. Dock position persists in
the profile. The same menu enables a resizable split console; Escape toggles it
when keyboard focus is in the toolbox.

Inspector Rules shows matching author CSSOM rules and source URLs. Cross-origin
stylesheets may be inaccessible; browser default rules, exact cascade precedence,
overridden declaration marking, forced pseudo-classes, source line mapping,
shadow/iframe inspection and flex/grid overlays are not implemented. Computed
values come from the engine’s `getComputedStyle`. Layout dimensions are viewport
CSS pixels. Undo covers the last 100 Inspector attribute/class/inline-style edits
on the current document; outer HTML replacement is not undoable. The picker
intercepts a page click and cleans up its temporary outline and listeners on
selection, Escape, toolbox close or a nine-second timeout.

Console records come from Servo's native developer stream, with severity, timestamp,
source position, argument previews and trace stacks when supplied by the engine.
CSS errors are subscribed in the main page; frame/worker messages are shown when
Servo reports their pipeline. Filters combine severity and category switches with
space-separated text terms; `-term` excludes a term. Repeated adjacent records are
grouped only when content, arguments and source positions match. Right-click a
message to copy it; the disclosure control opens argument and stack previews.
Options contains timestamps, navigation persistence, follow-output, multiline
editing, native evaluation and filtered text export. Cmd/Ctrl+K clears displayed
records. Up/Down navigates the last 100 commands in single-line mode; Ctrl/Cmd+Enter
runs multiline input.

The default Console evaluator runs in the main page and awaits promises. An
expression beginning with `await` is supported; use an async IIFE for statement
blocks with await. This mode requires page `eval` permissions. **Native evaluator**
uses Servo's debugger evaluator in the main global, with synchronous results;
promises are shown as objects. While paused, Console always evaluates in the
selected native frame, independently of the page's script execution loop. Getter
properties are displayed without automatic invocation in ordinary descriptor-based
previews; proxies or explicit evaluated code may execute page code. Native previews
are snapshots rather than live expandable remote objects. Scope identifier
suggestions use captured paused environments, not general property completion.

Debugger **F8** pauses/resumes, **F10** steps over, **F11** steps in, and **Shift+F11**
steps out. Frame selection changes scope inspection, watches and Console evaluation;
stepping still uses the actual suspended frame. Watches refresh on pause/frame
selection, or explicitly while running. Values are discarded when their debugger
context changes. Up to 20 watch expressions are retained per tab across navigation;
source IDs, breakpoint locations and ignore state belong to a specific document.
Whole-source ignoring delegates to Servo's blackbox API and is exercised against a
real executable breakpoint. Native requests are serialized; a slow request reports
its state rather than dispatching overlapping commands that could crash the engine.

Console retains 1,000 records across tabs, with 16 KiB display text and 64 KiB
structured records. Native object/environment caches retain at most 1,000/500
entries with 64/256 KiB previews respectively. Source capture is capped at 250
sources / 1 MiB per source; search stops at 500 matches / 16 MiB scanned. Scope
rendering limits parent traversal and property counts. Event overflow is reported.
Capture data and command history stay in memory unless explicitly exported.

This is not complete Firefox or Chrome DevTools parity. Conditional breakpoints,
logpoints, pause-on-exception/debugger-statement settings, XHR/event/DOM mutation
breakpoints, source maps, source-mapped pretty printing, eager previews, live remote
object expansion, worker debugging, CPU sampling, heap snapshots, IndexedDB editing,
and HttpOnly cookie inspection remain unimplemented. Debugging targets the main page context;
worker-specific debugging is not exposed. Page-driven tools require JavaScript to
be running, and show errors or timeouts when the page cannot answer.

Network recording retains at most 500 requests across tabs and 64 KiB per body
preview. The table marks truncated previews. Total time and the waterfall reflect
observed engine events; DNS/TLS/server-wait/receive phases are not separately
reported. Some engine network failures may remain pending because Servo omits
response metadata. Transferred sizes use Content-Length when available and show unknown otherwise;
response data size is separate. These are not full wire-byte measurements. A warning appears
if events are dropped under load. Cache disabling affects all tabs while the
toolbox remains open. Right-click requests for copying, cURL, edit/resend, opening
in a new tab or domain blocking. The Options menu contains HAR import/save/copy,
log persistence, column visibility and host/offline settings. Edit/resend uses the
active page's fetch API, so CORS and engine-controlled headers apply. URL filters
accept space-separated `domain:`, `method:`, `status:`, `type:`, `is:cached`,
`is:imported`, and `-` exclusions; selected resource types are combined with OR.
HAR import validates the whole file before adding rows, accepts up to 16 MiB/500
entries, preserves binary previews as base64, and marks imported rows with [HAR].
Imported durations stay fixed. HAR exports include headers and bounded previews.
Live initiator stacks, WebSocket frames, bandwidth throttling and detailed phase
timings remain unavailable; imported HAR initiator metadata can be displayed.
Capture data stays in memory until explicitly exported and is not saved with the
browser profile. Closing the toolbox resumes paused JavaScript.

## Profile storage

The default profile uses the OS application data directory:
`~/Library/Application Support/io.github.wulfnb.Wega` on macOS,
`~/.local/share/wega` on Linux and `%LOCALAPPDATA%\wulfnb\Wega\data` on Windows.
`--profile-dir` overrides this path.

`browser.json` stores browser settings, bookmarks, recent URLs, and session URLs;
`engine/` holds Servo's cookies and website storage. The browser writes its JSON
atomically and does not overwrite an unreadable existing profile. Treat the
profile as private browsing data. Startup restoration reloads URLs; it does not
serialize DOM state or navigation stacks. The browser locks each profile to prevent concurrent writes; use separate profile directories for multiple instances.

## Verification

See the [reliability, compatibility and release baseline](docs/reliability-baseline.md)
for unsaved-state protection, repeatable two-engine process-tree benchmarks and
the distinction between published-release validation and local changes.

See [architecture](docs/architecture.md), [decision record](docs/architecture-decisions.md), and [quality checks, coverage and isolated test runners](docs/testing.md).
The core can be tested without Servo using `cargo test --locked --no-default-features --lib`.
The required target is 100% first-party line coverage; it is not yet achieved. See the [current coverage status and gaps](docs/quality-report.md#current-coverage-target-and-result).
Native test flags require a build with `--features test-harness`; production builds exclude their drivers.

```sh
cargo fmt --check
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
# Opens a native window and checks the real Servo engine:
cargo run --locked --features test-harness -- --smoke-test --profile-dir /tmp/wega-smoke
# Manual interaction fixture, including links, scrolling, popups and dialogs:
cargo run --locked -- "file://$PWD/tests/fixtures/interaction.html"
```

For the manual fixture, use its absolute file path, or the `file://` URL. You can
also serve `tests/fixtures/` with any local HTTP server to check networking.
The smoke test exits nonzero on failure and writes `smoke-page.png` in the isolated
profile directory, along with rendered browser-frame images for visual checks. It
requires a graphical desktop and GPU context. Actual pointer gestures exercise
drag-and-drop in both tab layouts. To test HTTP as well, serve the fixtures on a
loopback port and pass the fixture URL after the smoke-test options.

For the developer toolbox integration test, run the fixture server in one terminal
and the browser test in another:

```sh
python3 tests/network_server.py 8767
cargo run --locked --features test-harness -- --toolbox-test --profile-dir /tmp/wega-toolbox-test http://127.0.0.1:8767/toolbox.html
```

It checks real HTTP POST bodies/headers, 404 responses and redirects, HAR output,
DOM/CSS inspection, matched rules/computed values, attribute edits/undo, actual
page pointer picking, bottom/left/right docking, split console, detached window
close/reopen/redock, structured console levels/source positions/object/stack previews,
CSS and uncaught errors, native nested scopes, parent-frame evaluation, watches,
stepping, breakpoint enable/disable, whole-source ignoring, await expressions,
accessor-safe previews, source searching and navigation, debugger pause/resume,
performance entries, memory reports, storage, accessibility and the app manifest.
It exports app-owned frame images and panel JSON into the isolated profile.

## Current limits

This is a working browser implementation, not a claim of Firefox feature parity.
Website compatibility follows Servo's implementation. Firefox extensions, account
sync, password management, printing, DRM and WebGPU remain
outstanding. Multiprocess is an optional experimental build. GStreamer media
playback is enabled by default. HTTPS certificate verification remains enabled. The UI does not
claim that an HTTPS page is trustworthy just because its URL uses HTTPS.

Servo 0.7 does not support `background-clip: text`. A small user stylesheet makes
Tailwind `.bg-clip-text` and inline clipped text readable with a solid gradient-stop
color (or inherited text color). Ordinary gradient backgrounds are preserved. The
fallback disables itself when the engine supports text clipping. It is a readability
fallback, not an implementation of gradient glyph masking.

For rendering diagnostics, export a real page and browser frame with an isolated
profile:

```sh
cargo run --locked -- --profile-dir /tmp/wega-capture --capture /tmp/edudia.png https://edudia.in/
```

This exits after loading and writes the page PNG, a `.window.png` browser frame,
and a JSON report of clipped text colors.

The old simulated site layouts, fabricated search results, loading timings, DOM
tree, and engine statistics have been removed.

## Source map

- `src/main.rs`: native application lifecycle and startup options.
- `src/browser.rs`: Servo webviews, shared GPU rendering, input routing, sessions.
- `src/engine.rs`: engine event delegates and system clipboard bridge.
- `src/ui.rs`: browser chrome, draggable tab layouts, collections, site dialogs.
- `src/input.rs`: native keyboard/modifier/cursor translation.
- `src/profile.rs`: saved profile, URL resolution, tab ordering and unit tests.
- `src/theme.rs`, `src/native_theme.rs`: palettes, persisted themes and AppKit vibrancy.
- `src/harness/`: native test scenarios (smoke, theme, downloads, capabilities, toolbox), built only with `--features test-harness`.
- `src/network.rs`: bounded network recorder, native event bridge and HAR export.
- `src/toolbox.rs`, `src/probes.rs`: asynchronous engine commands and page inspection.
- `src/devtools_ui.rs`: ten native developer panels.
- `tests/network_server.py`: fixture server for the live developer-tool scenario.
- `vendor/servo`: the small builder hook for an in-process developer-event consumer.
- `vendor/epaint`: a documented glyph-rendering compatibility patch; see `vendor/README.md`.

Built against the [published Servo API](https://docs.rs/crate/servo/0.7.0) and
[official embedding example](https://github.com/servo/servo/blob/aac43a3f31a259f04a574f5ec4e959c943ad7cc7/components/servo/examples/winit_minimal.rs).

## Expanded engine integration

See [the capability status matrix](docs/servo-capabilities.md) for the implemented Servo hooks, native browser controls, optional server modes, integration-test command and remaining backend work. Use **… → Browser settings** for host rules, offline mode, user scripts, browsing-data controls and WebRender diagnostics. The menu also provides reader view, page dark styling, site information and viewport screenshots. **Cmd/Ctrl+F** opens find; **Cmd/Ctrl+Shift+R** reloads without cache.

Audio/video decoding is enabled in the default build. macOS builds need `brew install pkgconf gstreamer libnice-gstreamer`; the package script bundles the media plugins and native libraries. A muted WebM/WAV playback test is available as `python3 tests/media_smoke.py`. For a build without GStreamer, use `--no-default-features --features brotli-streams`.

Network request details now open in a resizable right pane, with independent scrolling. The Style Editor keeps its stylesheet list on the left and CSS editor on the right.

Storage isolates errors for local storage, session storage, and cookies. Opaque origins (including data pages) show an explanation and disable unavailable edits rather than failing the entire panel.

The Storage **Cookies** tab reads matching cookies asynchronously from Servo,
including HttpOnly cookies. Its filterable table shows domain, path, expiry,
SameSite, HttpOnly and Secure metadata. Cookie values stay in native developer
tools and are never injected into the page. Cookie editing is not yet available.

Performance supports start/stop recordings (60-second limit), animation-frame intervals, resource/mark/measure/paint timings, a selectable waterfall, time-range zoom, summaries, and copying timing JSON. Captures are bounded to 2,000 observer entries and 3,600 animation frames. These are page Performance API timings, not Gecko-style sampled CPU profiles or flame graphs. Navigating away discards the page recording.

Product branding is **Wega Browser**, with the **Emerald Globe** app icon and the
default **Wega · Emerald Glass** palette. Servo remains the rendering engine.
The executable and the Compose service are named `wega`, Compose settings use
`WEGA_*` variables, and internal pages use `wega://`. WebDriver reports
`browserName: wega`. The macOS application bundle is `target/Wega.app`.

## Licensing

Wega is licensed under the [Mozilla Public License 2.0](LICENSE). See
[NOTICE](NOTICE) for attribution, the Servo trademark notice and the warranty and
liability terms. `vendor/` contains modified copies of the MPL-2.0 Servo 0.7.0
crate and the MIT/Apache-2.0 epaint 0.37.0 crate; [vendor/README.md](vendor/README.md)
describes each change.

Every download (macOS app, Linux and Windows archives, Docker image) includes
`LICENSE`, `NOTICE` (recording the exact source commit it was built from) and
`THIRD-PARTY-NOTICES.html`, generated with
[cargo-about](https://github.com/EmbarkStudios/cargo-about) by
`scripts/third-party-notices.sh`. The macOS app additionally lists the bundled
LGPL GStreamer/GLib libraries, their Homebrew versions and where to obtain their
source in `Contents/Resources/MEDIA-LIBRARIES.txt`.

Video and audio decoding on macOS uses Apple's system codecs (VideoToolbox and
AudioToolbox through GStreamer's `applemedia` plugin). The app does not bundle
FFmpeg, gst-libav, x264, x265 or other GPL or patent-encumbered codec libraries,
and the packaging script refuses to produce a bundle that contains them. See
[docs/licensing.md](docs/licensing.md) for details.


The experimental second engine is built separately because its ICU dependencies
cannot be linked with the pinned Servo release. Run
`python3 scripts/build-wega-engine.py --debug`, then use the bottom-right engine
selector or start with `--engine wega`. It uses [Blitz](https://github.com/DioxusLabs/blitz)
and V8 through Deno; Servo remains the default for everyday browsing. See the
[engine strategy and current limits](docs/engine-strategy.md#which-engine-should-we-use-now).
