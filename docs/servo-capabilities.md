# Servo 0.7 integration status

The default build remains WebRender/GL. The presence of a delegate method or Cargo feature does not guarantee a complete platform backend. This file records the actual integration, including gaps.

| Requested feature | Implementation and remaining work |
|---|---|
| Unload confirmation | Servo navigation uses its `request_unload` responder. Browser-owned confirmation now protects loaded website/local tabs before close, manual unload, engine switch and quit; Wega also confirms navigation. Website/local tabs are excluded from automatic unloading. Cancellation retains live state. A proper acknowledged engine close/unload handshake remains required to replace this conservative policy. See [reliability baseline](reliability-baseline.md). |
| Favicons | Engine raster images appear in tabs, bookmarks and history. Up to 128 URL favicons persist at bounded thumbnail size. |
| Web notifications | WebView and worker delegates submit through macOS UserNotifications after OS authorization. Site permission remains separate. Requires `.app` identity. Delivery, OS UI, notification actions and click callbacks have not been integration tested. |
| Navigation rules/external apps | Configurable host allow/block rules, domain-boundary matching, confirmation before opening external URI schemes through macOS open, Windows ShellExecuteW or Linux xdg-open. Windows/Linux dispatch still needs runtime validation. Host restrictions are not a secure kiosk mode. |
| Request interception | Real HTTP/HTTPS interception blocks configured hosts and offline requests, including subresources. This is a host blocker, not an Adblock filter-list implementation. Arbitrary outgoing header mutation is not exposed by this hook. |
| Internal pages | Registered unprivileged `wega://newtab`, `history`, `bookmarks`, `settings` protocol. History/bookmark HTML is escaped and served with a restrictive CSP. Native settings are in the browser menu. |
| Protocol registration | Approval consumes Servo's registration responder; approved schemes navigate through Servo. Registrations last for this browser process. |
| Screen geometry | Monitor size and actual outer window bounds; macOS available area excludes system UI via NSScreen.visibleFrame. |
| Popup move/resize | Opt-in window changes; resize bounded to monitor size. Popups still use browser tabs rather than independent popup windows. |
| Media sessions | Receives events and provides native settings controls calling Servo actions. macOS Now Playing metadata and play/pause/toggle/stop/track commands are wired through MediaPlayer and winit. Actual muted media playback is verified; physical media keys and OS control-center UI still require manual verification. |
| Remote Firefox tools | Explicit `--remote-devtools PORT` uses Servo's real loopback server and connection approval. This mode replaces the native Network/Debugger event adapter. Firefox compatibility follows Servo's protocol subset. |
| Errors | ServoDelegate errors and background console diagnostics reach browser status/stderr. |
| Traversal completion | Back/forward operations retain traversal IDs, show pending state and clear it on Servo completion. |
| Clear browsing data | Engine APIs delete cookies, local/session storage and HTTP cache, plus browser history. Session cookies can also be deleted separately. This does not claim to clear IndexedDB, service-worker registrations or CacheStorage. |
| Site info | Engine storage-site inventory, per-site deletion and cookies including HttpOnly. Cookie values remain inside native UI. |
| Storage cookie inspector | Asynchronous engine-backed cookie table for the active HTTP(S) URL, including HttpOnly, Secure, SameSite, domain, path and expiry, with native filtering. Navigation resets results; opaque origins show an explanation. Cookie editing and IndexedDB inspection remain outstanding. |
| User scripts | Persisted source via UserContentManager.add_script; explicit Apply. Reader overlay preserves original document; a page-dark-style toggle is also available. These are page DOM features, not Firefox Reader View parity. |
| Screenshots | Asynchronous WebView screenshot API exports viewport PNG to the profile's screenshots directory. Thumbnail/new-tab preview gallery remains outstanding. |
| Pinch zoom | Native trackpad pinch routed separately from page zoom. The Docker viewer also has an explicit local pan/pinch mode that suspends remote input and resizing; real Chrome/noVNC gestures are verified, physical mobile devices remain untested. |
| Permission persistence/revocation | Page-scoped grants and top-level-origin Block/Ask only. The pinned callback lacks requesting-frame identity and the engine cannot acknowledge active-access revocation. See [required engine changes](permission-engine-requirements.md). |
| Custom requests | Reload without cache uses UrlRequest custom headers. Servo 0.7's public UrlRequest has URL and headers only; custom method/body cannot be implemented through this API. |
| Clear tab history | Settings calls WebView.clear_session_history. |
| WebDriver | Opt-in loopback W3C transport from Mozilla's webdriver crate. Sessions, URL/title/source, navigation, script evaluation with JSON arguments, handles/selection and screenshots. Navigation uses WebView operations and actual load/traversal completion callbacks; page source uses Servo.execute_webdriver_command with a supported script command. Other endpoints explicitly return UnsupportedOperation; not a complete Selenium implementation. |
| Memory allocation reports | Patched pinned Servo DOM vtables to distinguish Box and Rc allocations: Rc value pointers include a preceding reference-count header and cannot be passed directly to allocator-size callbacks. Ownership/alignment regressions pass. The real toolbox Memory probe now passes in a fresh Linux ARM64 debug/test-harness build, together with all Linux runtime/shell checks. macOS native measurement also passes; Ubuntu x86_64 release CI must rerun after push. |
| WebRender tools | Settings offers profiler, texture cache, render targets and capture via native Servo calls. |
| GStreamer media | Enabled by default. Real WebM video and WAV audio decoding/playback are integration tested. macOS packaging includes the explicit plugin list and dependent libraries. GStreamer SDK, pkgconf and libnice-gstreamer were installed on the development Mac. macOS uses frame uploads; accelerated-media initialization is not invoked because the exposed API has no CGL context representation. Windows CI/release packaging now enables a pinned GStreamer SDK and exact x64 dependency closure, including native Media Foundation/D3D11 plugins; actual Windows playback must pass CI after push. Per-element fallback messaging is native-fixture tested. Codec/site compatibility still follows Servo and the bundled plugins. |
| Multiprocess | Optional Cargo `multiprocess` feature plus `--multiprocess` and early content-process dispatch. Experimental; this is not a claim of sandboxing or one OS process for every site. |
| WebGPU | Outstanding: dependency/build activation and adapter/device-loss validation. |
| Gamepads/haptics | Outstanding: input backend and haptic delegate. |
| Bluetooth | Device chooser hook is wired with explicit selection/cancel; native transport/build/backend and device testing remain outstanding. |
| WebXR | Outstanding: runtime discovery, session/render targets and packaging. |
| Brotli streams | Enabled in the default Cargo build. |
| Vello | Outstanding experimental canvas backend profile. In Servo 0.7 this flag reaches servo-canvas/vello and the dom_canvas_backend preference; it does not replace the page compositor. Production stays on WebRender. |
| Download manager | Implemented: page download links, link/image context menus, URL entry, four parallel connections by default (configurable 1–8), validated range responses with single-connection fallback, pause/resume/cancel/retry, and atomic publication of completed files. Site cookies are reused. The web viewer exposes authenticated downloads from the configured host directory. Active jobs do not resume after browser restart. |
| Website password manager | Outstanding. The web viewer's encrypted account settings authenticate access to Wega; they do not save website passwords or provide autofill. |
| Find in page | Native find controls use DOM ranges, next/previous, wrap and selected-match feedback. Search caps at 50,000 text nodes/10,000 matches; does not join text across separate nodes. |
| Session restore/settings | URL/session restore and native persisted settings exist. Web viewer account names and password-verifier records are encrypted at rest; password verification uses salted scrypt. Full page-state recovery, website password storage, printing and DRM are not provided. |

## Running

```sh
cargo run --locked -- --profile-dir /tmp/wega-personal
cargo run --locked -- --profile-dir /tmp/wega-remote --remote-devtools 7000
cargo run --locked -- --profile-dir /tmp/wega-automation --webdriver-port 7001
cargo run --locked --features multiprocess -- --profile-dir /tmp/wega-processes --multiprocess
```

Remote tools and WebDriver are off by default. The server binds only loopback. Use a separate profile for automation. The WebDriver bridge reports the `wega` browser name, normal page load strategy and a fixed 30-second command deadline. Element arguments and unsupported commands are rejected. Existing cookies, storage and tabs in the selected profile remain accessible to automation.

## Native integration test

Run the HTTP fixture, then use a fresh isolated profile:

```sh
python3 tests/network_server.py 8767
# In another terminal:
cargo run --locked --features test-harness -- --capabilities-test --profile-dir /tmp/wega-capability-test
```

The test exercises scripts, screen metrics, engine cookies/storage deletion, DOM-range find, reader mode, offline interception, internal history, screenshot saving and pinch zoom. It does not grant OS notifications, connect physical devices or claim media backend coverage.

## Media build and verification

The default build requires Rust 1.95+ and a GStreamer 1.20+ development SDK. On macOS:

```sh
brew install pkgconf gstreamer libnice-gstreamer
scripts/package-macos.sh --debug
python3 tests/media_smoke.py 'target/Wega.app/Contents/MacOS/wega'
```

`--no-default-features --features brotli-streams` builds without media; the packaging equivalent is `scripts/package-macos.sh --debug --no-media`. The media packager copies libraries before rewriting their install names and ad-hoc signing the copies. Installed SDK libraries remain untouched. Each profile keeps its GStreamer registry under `engine/`, and startup loads Servo's explicit plugin list rather than scanning unrelated SDK plugins. Physical-device APIs and DRM are outside this playback test.

### Website icon rendering (23 October UI review)

Wikipedia interface icons in `docs/ui-ux-2026-10-23/wikipedia.jpg` use Vector
SVG masks over black backgrounds. The live diagnostic confirmed that both CSS
mask support checks return false. `src/mask_icons.js` supplies a scoped MediaWiki
compatibility fallback using the original same-origin SVG URLs and selectors;
it skips engines with mask support. An actual native screenshot of the same mask
pattern verifies transparent corners and a visible glyph. A fresh live Wikipedia
capture remains required because verification navigation timed out. This does
not implement general CSS masks in Servo.
