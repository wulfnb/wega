# Standalone Wega Engine Migration Plan

Status: **Proposed Architectural Pivot**, 31 October 2026.  
Supersedes the dual-engine strategy in [engine-strategy.md](engine-strategy.md) by scheduling the full retirement of the Servo rendering backend and committing all development to Wega's native engine (`blitz-dom` + V8 via `deno_core` + Skia).

---

## 1. Executive Summary & Strategic Intent

### Why Retire Servo?
Until now, Wega operated as a hybrid browser: Servo served as the general-purpose compatibility engine, while Wega's custom engine was treated as an experimental, low-memory worker for simple static and article sites.

In practice, maintaining Servo as a fallback creates several critical frictions:
1. **Diverted Engineering Focus:** Substantial effort has been spent debugging, patching, and working around upstream Servo limitations (e.g., CSS mask quirks, background clip text workarounds, DOM allocator vtable patches, SpiderMonkey memory leaks, and complex GStreamer bindings) rather than advancing Wega’s own engine.
2. **Cargo Dependency Incompatibility:** Servo's pinned `mozjs_sys` requires ICU 2.1, while Blitz 0.3-beta.4 requires ICU 2.3. This forced Wega into two separate Cargo workspaces, two separate `Cargo.lock` files, duplicated compilation, and intricate external build scripts.
3. **Heavy Build and Binary Overhead:** Compiling SpiderMonkey, WebRender, and GStreamer C-libraries inflates build times from minutes to over 25 minutes, complicates cross-platform release packaging (macOS dylib rewriting, Windows ANGLE/WGL hacks), and balloons distribution packages.

### The Decision
**Commit 100% of development to Wega's own native engine and systematically remove the Servo dependency.**

All future bug fixes and features will be implemented directly in Wega's engine or upstreamed to Blitz/Deno. Rather than dividing time between two engines, this roadmap outlines how every functionality currently provided by Servo will be built into Wega's standalone engine.

---

## 2. Target Architecture

```text
┌────────────────────────────────────────────────────────────────────────┐
│                        Wega Native Browser Process                     │
│                                                                        │
│   ┌────────────────────────────────────────────────────────────────┐   │
│   │               egui UI Chrome & Window Manager                  │   │
│   │    - Tabs, movable panes, address bar, native bookmarks/find   │   │
│   │    - Native internal views: wega://newtab, settings, history   │   │
│   └────────────────────────────────────────────────────────────────┘   │
│                                │                                       │
│   ┌────────────────────────────┴───────────────────────────────────┐   │
│   │           Network, Cookie & Permission Broker (P4)             │   │
│   │    - Shared HTTP/2 & HTTP/3 connection pool, HSTS cache        │   │
│   │    - Centralized cookie jar, site isolation policies, cache    │   │
│   └────────────────────────────┬───────────────────────────────────┘   │
└────────────────────────────────┼───────────────────────────────────────┘
                                 │
           Versioned Pipe IPC Protocol (Shared Memory Frames)
                                 │
┌────────────────────────────────┴───────────────────────────────────────┐
│                       Out-of-Process Tab Worker                        │
│                     (One OS-Sandboxed Process per Tab)                 │
│                                                                        │
│   ┌────────────────────────┐  ┌────────────────────────────────────┐   │
│   │       blitz-dom        │  │       V8 Runtime (deno_core)       │   │
│   │   - HTML5 Parser       │  │   - JS Engine (JIT enabled)        │   │
│   │   - Stylo CSS Matcher  │◀─┼─▶ - WebIDL Generated DOM Bindings  │   │
│   │   - Taffy Layout       │  │   - Deno Standard Web APIs         │   │
│   │   - Parley Text Layout │  │   - Event Loop & Watchdog Timer    │   │
│   └───────────┬────────────┘  └────────────────────────────────────┘   │
│               │                                                        │
│   ┌───────────▼────────────────────────────────────────────────────┐   │
│   │              Skia / Vello Hardware Rasterizer                  │   │
│   │    - 2D Canvas rendering & accelerated vector display list     │   │
│   │    - Shared-memory frame emission (RGBA buffer / GPU texture)  │   │
│   └────────────────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────────────────┘
```

### Key Architectural Principles
1. **Unified Workspace:** With Servo removed, the ICU version conflict is eliminated. The root `Cargo.toml` and `engines/wega/Cargo.toml` will merge into a single clean Cargo workspace with one `Cargo.lock`.
2. **Worker-per-Tab Process Isolation:** The out-of-process worker architecture remains. Running each tab in a separate worker process guarantees that page crashes or V8 memory pressure never bring down the browser UI.
3. **Native Chrome for Internal Pages:** Internal `wega://` pages (new tab, history, bookmarks, settings) will be rendered as native `egui` interfaces rather than running through a web engine, ensuring instant loading, zero memory overhead, and immune to web vulnerabilities.
4. **Codebase Cleanup:** Approximately ~7,300 lines of Servo desktop adapter glue and delegate handlers in `src/` will be safely deleted.

---

## 3. Functionality Parity Matrix

The following matrix lists every major browser capability, its status under Servo, and the engineering plan to achieve full parity in Wega's standalone engine:

| Capability | In Servo (Retiring) | In Wega Today | Engineering Plan for Standalone Wega Engine |
| :--- | :--- | :--- | :--- |
| **Workspace & Builds** | Split Cargo graphs, 25m+ builds | Separate `engines/` workspace | Merge into single Cargo workspace; drop SpiderMonkey, GStreamer, and C++ toolchains. |
| **DOM & WebIDL Bindings** | 574 WebIDL interfaces generated | Handwritten `bindings.js` (~85 KB) | Build an automated WebIDL-to-Rust/JS code generator for standard HTML, DOM, and Event interfaces. |
| **HTML Forms & Controls** | Full HTML5 form controls | Basic text/form submit | Implement native popup controls for `<select>`, `<input type="file">` via desktop dialogs, and full `FormData`. |
| **Text Selection & Clipboard** | Page-wide range selection | Input fields only | Add layout-node hit-testing in Blitz for text selection; integrate with `arboard` OS clipboard. |
| **Iframes & Subdocuments** | Full nested frame support | Stubs / Fallback triggers | Extend Blitz sub-documents (`iframe.rs`) to instantiate nested isolates or routed workers communicating via `postMessage`. |
| **Web Components** | Shadow DOM & Custom Elements | Unsupported | Upstream Shadow DOM encapsulation and scoped style trees in Blitz/Stylo; wire custom element lifecycle callbacks. |
| **Client Storage** | LocalStorage, partial Session | LocalStorage via Deno | Implement IndexedDB and Cache API using embedded SQLite (`rusqlite`) in the profile directory. |
| **Media (`<video>`, `<audio>`)** | GStreamer pipeline | Unsupported | Build a dedicated platform media worker (AVFoundation on macOS, Media Foundation on Windows, ffmpeg on Linux) streaming video frames to shared textures. |
| **Graphics (Canvas 2D & WebGL)** | WebRender Canvas 2D/WebGL | Unsupported | Expose `CanvasRenderingContext2D` using Wega's existing `skia-safe` backend; implement WebGL on `wgpu`. |
| **Web Workers** | SpiderMonkey background workers | Single main isolate | Support `new Worker()` by spawning additional V8 isolates within the tab process. |
| **Security & Sandboxing** | In-process or basic multiprocess | No OS sandbox | Apply OS sandboxes to the tab worker before V8 initialization (Seatbelt on macOS, Landlock on Linux, AppContainer on Windows). |
| **Developer Tools** | Firefox Remote DevTools | Error console logs only | Connect V8's Inspector protocol to a local WebSocket for Chrome DevTools / VS Code debugging; build DOM inspection bridge. |
| **Automation** | WebDriver loopback transport | Partial CLI harness | Map W3C WebDriver endpoints directly to worker protocol commands. |

---

## 4. Phased Implementation Roadmap

### Phase 1: Decoupling & Workspace Unification (Immediate P0)
* **Goal:** Eliminate the Servo dependency, unify the build, and run Wega entirely on its own engine.
* **Work Items:**
  1. Remove `servo`, `servo-script-bindings`, `servo-devtools-traits`, `servo-base`, and `malloc_size_of` from root `Cargo.toml`.
  2. Remove `multiprocess`, `brotli-streams`, and `media` features tied to Servo.
  3. Merge `engines/wega` into the main workspace under `crates/wega-engine` or as a top-level workspace member with a single `Cargo.lock`.
  4. Port `wega://newtab`, `wega://history`, `wega://bookmarks`, and `wega://settings` to pure `egui` components in `src/ui.rs`.
  5. Delete Servo-specific files: `src/servo_host.rs`, `src/engine.rs` delegates, `src/native_media.rs`, and unused compatibility scripts.
  6. Set `EngineKind::Wega` as the sole, non-optional engine across all tabs and session models.

### Phase 2: Web Platform Core & WebIDL Generation (P1)
* **Goal:** Expand real-world site rendering and script compatibility without hand-coding hundreds of APIs.
* **Work Items:**
  1. **WebIDL Binding Generator:** Create a code generator that parses standard WebIDL files and emits Rust `deno_core` ops and JavaScript class prototypes.
  2. **Page Text Selection & Clipboard:** Extend Blitz layout hit-testing to support selection ranges across arbitrary DOM nodes, and wire `copy`/`paste` to `arboard`.
  3. **Native Form Controls:** Implement native `<select>` drop-down menus, checkboxes/radio buttons, and file picker dialogs via `rfd`.
  4. **DOM Event Conformance:** Ensure complete capturing/bubbling event dispatch, `CustomEvent`, and synthetic event dispatch.

### Phase 3: Isolation, Storage & Sub-Documents (P2)
* **Goal:** Support complex, authenticated multi-page applications and secure untrusted scripts.
* **Work Items:**
  1. **Worker OS Sandboxing:**
     * macOS: Apply `sandbox_init` with a tight profile restricting filesystem access to the profile cache and allowing only pipe IPC and TCP sockets.
     * Linux: Apply Landlock filesystem sandboxing and seccomp syscall filtering.
     * Windows: Run workers within an AppContainer with restricted token privileges.
  2. **Storage Subsystem:** Implement the W3C IndexedDB standard on top of SQLite, isolated per origin under the profile directory.
  3. **Iframes & `postMessage`:** Wire Blitz's subdocument trees so that embedded frames execute scripts and communicate across origin boundaries using structured cloning over IPC.

### Phase 4: Hardware Graphics & Native Media (P3)
* **Goal:** Support web graphics, games, and streaming audio/video without GStreamer baggage.
* **Work Items:**
  1. **Canvas 2D:** Implement HTML `<canvas>` by exposing Skia drawing contexts (`skia-safe`) directly to V8, writing into Blitz image nodes.
  2. **Hardware Video/Audio:** Implement a lightweight media backend leveraging OS hardware decoders (Apple VideoToolbox / AVFoundation, Windows Media Foundation, Linux VAAPI/ffmpeg) emitting decoded video frames as GPU textures.
  3. **Web Workers:** Enable `Worker` constructors by initializing secondary V8 isolates on background worker threads with message channel communication.

### Phase 5: DevTools & Ecosystem Integration (P4)
* **Goal:** Professional developer tooling and automated testability.
* **Work Items:**
  1. **V8 Inspector Integration:** Expose V8's built-in inspector protocol over a local WebSocket, allowing developers to connect standard Chrome DevTools or VS Code to debug JavaScript.
  2. **DOM / Style Inspector Bridge:** Map Stylo computed styles and Blitz DOM nodes into the Chrome DevTools Protocol (CDP) DOM domain.
  3. **W3C WebDriver Compatibility:** Wire the WebDriver HTTP server commands directly to `wega-engine` IPC commands for automated headless testing.

---

## 5. Verification & Quality Gates

Each phase of the migration must satisfy strict verification criteria:
* **Compilation Gate:** Clean build without warnings on standard Rust toolchain across macOS, Linux, and Windows.
* **Offline Framework Fixtures:** Angular, Preact TodoMVC, React, and Vue fixtures must continue to pass without regression.
* **WPT Test Suite Growth:** Systematically expand the automated Web Platform Tests (WPT) DOM subset executed against `wega-engine` as new WebIDL interfaces land.
* **Memory and RSS Baselines:** Standalone worker RSS must remain within documented budgets (< 120 MB for typical news/article pages, < 250 MB for complex interactive apps).

---

## 6. Conclusion

Retiring Servo transforms Wega into a truly independent, natively integrated Rust browser. By consolidating resources onto Blitz, V8 (`deno_core`), and Skia, Wega sheds massive external C++ technical debt, unifies its developer toolchain, and gains complete sovereignty over its rendering and JavaScript execution pipeline.
