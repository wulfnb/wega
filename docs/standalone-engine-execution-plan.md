# Standalone Wega engine: architecture and execution plan

Date: 31 October 2026. Branch: `feature/standalone-wega-engine`.
Planning baseline: `212304b`; source inspected before this documentation-only change.
Status: **Implementation started. No migration step is runtime-verified yet.**

Execution instruction (31 October 2026): commit and push incremental work on this
branch; run tests at the end only. Source inventory, code review and implementation
can proceed meanwhile. Record implementation separately from runtime verification;
deferred tests never satisfy a completion, parity or Servo-retirement gate.
See the [feature ledger](standalone-feature-ledger.md) for source observations and
pending evidence. Step 01 source auditing is in progress; baseline test execution is deferred.

Implementation language: all new Wega-owned engine, browser and tooling code is
Rust. The owner explicitly permits retaining V8 and Skia as dependencies. New DOM
semantics belong in Rust; generate required binding glue instead of adding new
handwritten JavaScript feature implementations. Existing Python tooling and
handwritten JavaScript bindings must be inventoried for later migration; their
presence is not permission to add more non-Rust implementations.

This is the execution order for [the migration proposal](standalone-engine-migration.md).
The destination is a browser powered entirely by Wega's engine: Blitz, V8 through
`deno_core`, and Skia. The requirement is to retain **every capability currently
provided by the enabled Servo integration**, including browser integrations and
web APIs, rather than delivering an article-only browser.

The older proposal establishes direction; this document governs work order,
architecture constraints and completion evidence where they differ. Servo removal
is a delivery milestone after replacement coverage, not proof of feature parity.
All new engine work goes into Wega. The existing Servo build supplies a frozen
comparison baseline while replacements are developed; this does not create a new
dual-engine product strategy.

## 1. What the current checkout actually contains

These are source observations, not freshly executed runtime or platform results:

| Observation | Source | Consequence |
| --- | --- | --- |
| Desktop still enables Servo, WebGL, WebCrypto and default GStreamer media. | Root `Cargo.toml` | Removing dependencies now removes real behavior. |
| Wega already uses V8/Deno, Blitz and Skia in a separate workspace. | `engines/Cargo.toml`, `engines/wega/Cargo.toml` | Extend the existing engine; do not start another prototype. |
| A shared HTTP/cache/cookie broker already exists. | `engines/network/src/lib.rs`, `src/wega_page.rs` | Harden and complete it rather than inventing a second network stack. |
| Private IPC is version 5, with bounded packets, navigation serials and shared-frame acknowledgements. | `src/engine_protocol.rs`, `engines/platform/src/lib.rs` | Evolve this protocol into a reusable contract. |
| Forms already have submission encodings, submit events and `FormData(form)`; Deno supplies several Web APIs and storage globals. | `engines/wega/src/dom/forms.js`, `dom/web.js`, `view.rs`, `web.rs` | Test the remaining semantics and controls; these are not all missing from scratch. |
| General selection, visible iframe rendering, OS sandboxing and several graphics/media APIs remain gaps. | `docs/wega-compatibility-roadmap.md`, engine sources | Include real implementations and negative security tests. |
| Default and internal-page selection still chooses Servo; unsupported visible frames can trigger fallback. | `src/engine_choice.rs`, compatibility roadmap | A successful page capture can currently conceal a fallback. |
| Native tooling, permissions, cookies, accessibility and media adapters contain Servo types. | `src/engine.rs`, `toolbox.rs`, `permission_adapter.rs`, `cookie_inspector.rs`, `native_media.rs` | Port the integration boundary as well as the rendering engine. |
| Containers already use separate browser processes and profile directories. | `docs/containers.md`, `src/containers.rs` | Preserve account separation and paths during migration. |
| DOM/CSS WPT subsets and framework fixtures already exist. | `engines/wega/tests/` | Expand the real-engine suites and retain upstream provenance. |

The original proposal's build-time, binary-size and RAM numbers are hypotheses
or targets until measured against a recorded workload. V8 and Skia still bring
native/C++ dependencies. Removing Servo does not remove every C++ toolchain.
Native internal views consume memory, and process isolation does not guarantee
that browser-wide memory exhaustion or privileged-service failures are harmless.
Stylo remains an intentional dependency even though it shares Servo ancestry.

## 2. Architecture to build toward

```text
Browser process: trusted coordinator
  Application/Profile + Session + Chrome (existing ownership retained)
  Navigation controller + browser-owned frame tree + process supervisor
  Permission decisions + user activation + native dialogs/clipboard/notifications
  Native internal views + downloads + container/profile selection
  DevTools/WebDriver adapters (explicit opt-in)
             |
             +-- Network service: HTTP/TLS, redirects, cookies, cache, HSTS
             +-- Storage service: origin/partition data, quotas, deletion
             +-- Platform services: files, fonts, devices and OS integrations
             |
     Versioned, authenticated, bounded IPC + revocable capabilities
             |
Sandboxed content processes: site/agent-cluster aware
  Document host: parser + DOM + lifecycle + origin/policy context
  Script host: V8 realms + Deno Web API adapters + task/microtask scheduling
  WebIDL-generated binding layer -> handwritten semantic DOM operations
  Blitz adapter: style/layout + hit testing + text geometry + accessibility
  Rendering: owned display list -> raster/compositor -> bounded surfaces
  Worker hosts: separate agent/event loop, structured clone and cancellation
             |
  Dedicated media service; GPU service when justified by the backend design
```

Services are ownership boundaries first. Keep the existing browser-owned broker
initially; extract a service process where fault containment or a platform backend
requires it. Do not split every module into a crate or introduce synchronous IPC
cycles merely to match the diagram.

### Ownership and dependency rules

1. **Browser state stays engine-independent.** Keep application, session and
   profile ownership in the existing core. Persist no V8, Blitz, Servo, GPU or
   native-window objects. Preserve profile locking, atomic saves and serde migrations.
2. **Document identity is explicit.** Distinguish stable tab, browsing-context/frame,
   document generation, navigation, request, process and service-session IDs.
   Browser-issued document identity scopes network, storage, permissions and tools.
   Navigation invalidates old capabilities and stale asynchronous replies.
3. **One process per tab is only the starting topology.** Cross-site frames need
   separate processes; same-origin documents that synchronously script each other
   need compatible agent/process placement, including related popups. V8 contexts
   or isolates alone are not OS security boundaries. Decide placement from site,
   origin and agent-cluster rules, not the tab index. See [Chromium's process model
   design](https://www.chromium.org/developers/design-documents/site-isolation/).
4. **Privileged services own authority.** A compromised renderer must not choose
   an arbitrary origin, profile path, cookie partition or requesting frame in a
   service request. Bind these to browser-issued capabilities and verify requests
   on receipt. Protect cross-origin response bodies as well as credentials.
5. **No renderer access to the whole profile or arbitrary network.** Broker
   network, persistent storage, downloads, file handles and native operations.
   Browser-managed fonts/resources and V8 JIT needs require narrow platform grants.
6. **DOM semantics are separate from bindings.** Generated bindings implement
   WebIDL conversion, exposure and wrapper rules; handwritten DOM algorithms
   implement behavior. Wega owns wrapper identity, tracing, stale-node handling
   and reentrancy. Layout data never becomes raw JavaScript pointers.
7. **One coherent scheduling model.** Document tasks, microtask checkpoints,
   lifecycle events, timers, rendering opportunities and worker messages follow
   the [HTML event-loop model](https://html.spec.whatwg.org/multipage/webappapis.html#event-loops).
   A Deno extension is a reusable component, not automatic browser conformance.
8. **Rendering remains replaceable behind an owned representation.** Retain the
   current raster/display-list path as the reference. Separate paint production,
   rasterization, composition and presentation. Account for surfaces and decoded
   images; acknowledge reuse and discard obsolete document frames.
9. **Sandbox setup is platform-specific and mandatory for release.** macOS needs
   a supported launch/sandbox/entitlement design compatible with V8 JIT; Linux needs
   filesystem, syscall, process and network restrictions; Windows needs a restricted
   process model and job containment. A named API or Docker isolation is insufficient.
10. **Unsupported means observable.** Expose only implemented APIs according to
    their standard exposure conditions. Do not install no-op success stubs to make
    a bundle start. Preserve explicit unsupported errors and diagnostic reporting.

### Proposed code boundaries

| Boundary | Starting point | Intended responsibility |
| --- | --- | --- |
| Browser core | `src/lib.rs`, `application.rs`, `session.rs`, `navigation.rs`, `permissions.rs` | Policy, persistence and user commands; no engine dependency. |
| Engine contract | Extract `src/engine_protocol.rs` into a small `wega-protocol` workspace member | Wire types, identity, capabilities, limits and version negotiation; no UI/runtime dependency. |
| Desktop host | `src/wega_page.rs`, `browser.rs`, platform adapters | Process supervision, command routing, presentation, native services. |
| Content engine | `engines/wega/src/` | Split orchestration into document, script, DOM, bindings, layout and render modules behind internal interfaces. |
| Network | Existing `engines/network/` | One policy-enforced transport and authoritative cookie/cache owner per profile/container scope. |
| Storage | New module, extracted to `wega-storage` if shared by services/tests | Storage keys, transactions, quota, deletion and crash recovery; no renderer-selected paths. |
| Platform/render transport | Existing `engines/platform/` | Frame handles, memory accounting and OS-specific primitives; confine unsafe code here or in reviewed platform modules. |
| Binding generation | New `tools/webidl/` and versioned IDL inputs | Deterministic generation, provenance, supported attributes and semantic hooks. |
| Media/GPU | Introduced only by steps 19–20 | Backend adapters, bounded buffers, device loss and independent failure handling. |

Keep existing directory names initially. Merge Cargo workspaces after the
standalone application works; move directories only when the move has a concrete
ownership or build benefit. `error-collector/` remains an independent service.

## 3. Parity contract: nothing disappears from the ledger

Step 01 must expand this seed into an auditable feature ledger. Each row needs a
source reference, exact behavior, Servo baseline status, Wega status, fixture or
manual procedure, platform evidence, step/subtask, and remaining limitations.
Use `unknown`, `missing`, `partial`, `implemented`, and `verified`; record evidence
per platform. A symbol, Cargo feature or passing unrelated test is not verification.

**Preserve** means behavior present in the current browser, including its documented
limits. **Audit/extend** means establish the enabled Servo behavior first and cover
it; any additional Web-platform scope remains tracked separately. An outstanding
Servo feature must not be relabeled as an existing delivered capability.

| ID | Capability family | Contract | Delivery steps |
| --- | --- | --- | --- |
| F01 | Tabs, movable top/sidebar tabs, panes, focus, container windows, settings, bookmarks/history and session restore | Preserve browser UX and profile/account isolation. | 02, 09, 10, 23 |
| F02 | Load/stop/reload/bypass-cache, redirects, back/forward, fragment and History API, clear tab history | Preserve completion/error semantics and cancellation. | 06, 09 |
| F03 | Page title, URL, favicon, loading state, cursor, screen/window geometry, popup/tab creation and bounded move/resize | Preserve; opener relationships require real frame/agent modeling. | 09, 14 |
| F04 | Unload/close/quit safety, dirty forms, freeze/thaw and worker crash recovery | Preserve conservative confirmation until an acknowledged replacement exists. | 02, 05, 09 |
| F05 | HTML parsing, DOM, events, script/module loading, URL/encoding, timers, observers and WebCrypto | Audit enabled engine APIs and preserve current behavior. | 06–08 |
| F06 | HTTP/TLS, cookies, HTTP auth, redirects, compression including Brotli, Fetch/XHR/WebSocket, offline/host policies and cache | Preserve browser rules and authentication; expand known gaps explicitly. | 04, 06, 09 |
| F07 | Origin security, CSP, mixed content, referrer policy, secure contexts and iframe policies | Audit enabled protections; close known Wega bypasses. | 04–08, 14 |
| F08 | Text/IME/pointer/wheel, zoom/pinch, form validation, native controls, file uploads and multipart bodies | Preserve input and form interactions, including non-Latin text. | 11 |
| F09 | Page selection, clipboard, Range, find, contentEditable, rich editing, drag/drop | Audit exact Servo support; replace implemented subsets with real behavior. | 12 |
| F10 | CSS/layout/text/fonts/SVG/images/animations, scrolling, fullscreen and responsive viewport | Audit actual rendering and interaction; retain reproducible reference pages. | 13, 19 |
| F11 | Nested frames, window/opener, messaging, sandboxed/srcdoc/data/blob documents and cross-site navigation | Audit exact support; preserve origin and synchronous-access semantics. | 14 |
| F12 | Shadow DOM, custom elements and slot/style/event behavior | Audit enabled Servo surface, then implement verified subsets. | 15 |
| F13 | Cookies/site inventory, HttpOnly native inspector, local/session storage, data clearing and per-site deletion | Preserve browser data management; keep HttpOnly out of page APIs. | 04, 16, 22 |
| F14 | IndexedDB, CacheStorage, worker/service-worker storage and offline applications | Audit/extend; do not claim existing clear-data UI already covers these. | 16, 17 |
| F15 | Dedicated/Shared/Service Workers, structured clone, transferable objects and message channels | Audit each enabled worker class; implement with correct ownership and lifecycle. | 17 |
| F16 | Canvas 2D, image readback, WebGL, context loss and origin tainting | Audit exposed Servo backends; replace the supported surface. | 18, 19 |
| F17 | Audio/video, controls, codecs, seeking, autoplay, media sessions, OS commands and media errors | Preserve verified WebM/WAV playback and document platform codec coverage. | 20 |
| F18 | MSE, Web Audio, capture, WebRTC and other enabled media APIs | Inventory engine exposure, actual backend and current limitations independently. | 20, 21 |
| F19 | Permission prompts, block/ask policy, fullscreen, notifications, device chooser and other native requests | Preserve attribution and current supported behavior; separately track revocation improvements. | 02, 21 |
| F20 | User scripts, reader overlay, page dark style, viewport screenshots and downloads with site credentials | Preserve existing application features and download job semantics. | 09, 12, 19, 22 |
| F21 | Native console/DOM/styles/network/HAR/debugger/storage/memory/performance tools | Preserve each usable tool on Wega; replace engine-specific data adapters. | 03, 22, 24 |
| F22 | Loopback remote tools and W3C WebDriver/CLI/native test drivers | Preserve currently supported automation; record Firefox-to-V8 protocol compatibility explicitly. | 03, 22 |
| F23 | Web content accessibility tree, actions, focus and native screen-reader integration | Preserve both chrome and page accessibility. | 11, 13, 14 |
| F24 | Custom protocol registration, external apps, URL/file/image/text viewers, auth prompts | Preserve policy and user confirmation; no native privilege from web content. | 09, 10, 21 |
| F25 | Docker/noVNC input, clipboard, resize/pinch, uploads/downloads, container launches and account authentication | Preserve deployment/viewer workflows and profile separation. | 10, 11, 23, 25 |
| F26 | Engine diagnostics, resource budgets, logging, signed error-report pipeline and packaging/notices | Preserve operational behavior; measure the replacement backend. | 03, 05, 24–26 |
| F27 | Unlisted enabled Servo interfaces and preferences | Mandatory source/API inventory before declaring the ledger complete. | 01, routed to the relevant step |

WebGPU, gamepads/haptics, Bluetooth transport, WebXR, experimental Vello, website
password storage/autofill, printing, full page-state recovery and DRM are documented
as outstanding or incomplete in the current integration. Keep them in a separate
extension backlog after auditing any enabled pieces. Do not promise DRM by merely
changing media backends. Memory-profiler/WebRender controls need Wega equivalents
for their diagnostic purpose, rather than obsolete backend-specific switches.

## 4. Numbered implementation order

Work one step at a time. A step is a work package, not an assumed single commit.
Split large packages into ordered `NN.a`, `NN.b`, etc. before coding; each subtask
must produce working behavior and its own evidence. No dates or person-week
estimates are assigned until the inventory and first vertical slice are measured.

### 01 — Record the feature inventory and baseline

**Depends on:** nothing. **Next implementation task.**

Create `docs/standalone-feature-ledger.md` and a machine-readable companion under
`docs/validation/standalone/`. Expand F01–F27 from the enabled dependency graph,
features/preferences, Servo delegates, native UI actions, WebDriver endpoints,
WebIDL exposure and existing suites. Inspect the pinned Servo sources, not only
this repository's capability summary. Capture frozen Servo and direct Wega
behavior with fresh profiles; record commit, dirty diff, binary/worker hashes,
platform, flags, fixture hashes and whether fallback occurred. Audit an isolated
copy of an existing profile for migration risks without writing to the original.

**Done when:** every discovered capability is mapped to a replacement step and
evidence or an explicit unknown/blocker; baseline artifacts distinguish direct
Wega results from Servo fallback. Unknowns remain blockers to a parity claim.

### 02 — Establish the engine contract and document lifecycle

**Depends on:** 01.

Extract protocol types from application source-path inclusions. Add typed
identities, correlated requests/results, capability negotiation, structured errors,
deadlines, cancellation and bounded queues. Define browser/document/worker state
transitions for creation, load commit, active, frozen, closing, crashed and closed.
Carry trusted frame/origin/partition context and user activation through native
requests. Keep application ports independent of both Servo and V8.

**Done when:** stale navigation replies, reused IDs, malformed/oversized IPC,
crashes, timeouts and cancelled permissions cannot mutate a newer document; the
existing browser still works with the extracted contract. No live-page migration
between engines is implied.

### 03 — Make the real worker observable and testable

**Depends on:** 02.

Add request-correlated evaluation/capture/navigation commands for the harness,
document/frame IDs in logs, assertion/error collection and subprocess cleanup.
Run offline jQuery, React, Preact, Vue and Angular fixtures against Wega with
fallback disabled. Extend the pinned DOM/CSS WPT runners and add a path to upstream
WPT serving, asynchronous tests, reftests and testdriver-backed interactions.
Keep diagnostics available from this step onward, not only after full DevTools.

**Done when:** CI can distinguish engine error, test timeout, worker crash, harness
failure and genuine assertion failure; a wrong-engine capture fails validation.
Record per-suite counts and exclusions rather than a whole-Web pass percentage.

### 04 — Make network, cookies and storage access authoritative

**Depends on:** 02, 03.

Extend the existing broker, preserving cookies, SameSite, auth, redirects,
decompression, cancellation and offline/host policies. Bind origin, initiator,
destination, container and partition to trusted document sessions; remove direct
production-worker bypasses across navigation, subresources, fetch, modules and
WebSockets. Replace file-extension CSP guesses with explicit resource destinations;
check module dependencies and every redirect. Add HSTS/certificate-error handling,
cache partitioning and HTTP caching semantics in separate verified subtasks.
Select service transport so the sandbox can prohibit arbitrary network access.

**Done when:** spoofed origins, redirect chains, unknown file extensions, private
network targets and cross-container credentials are covered by adversarial tests;
policy is enforced at the service boundary. Test loopback access via explicit test
policy. HTTP/2/3 availability is measured and audited rather than assumed.

### 05 — Sandbox and supervise content processes

**Depends on:** 04.

Write the platform threat model and resource-access allowlist. Implement narrow
service handles, brokered fonts/files/storage, launch setup, descriptor/handle
sanitization, child-process restrictions and OS containment. Verify V8 JIT and
signed macOS entitlements together; test Linux kernel support/failures and Windows
AppContainer/restricted-token/job choices. Enforce renderer, decoded-image, frame,
queue and aggregate process budgets with recoverable termination and crash UI.

**Done when:** macOS, Linux and Windows runtime probes deny arbitrary profile/file
reads, writes, process launch and direct network connections while normal fixtures
work. Unsupported sandbox setup fails closed for release browsing; any development
escape hatch is explicit. Killing a worker releases its resources and grants.

### 06 — Consolidate the document host and event loop

**Depends on:** 03–05.

Separate parser/document lifecycle from `view.rs` orchestration and script/runtime
plumbing. Preserve incremental parsing, blocking/defer/async scripts, modules,
document.write, load/error events, document replacement and encoding. Unify task
sources, microtasks, animation frames, observers and visibility/freeze scheduling.
Audit Deno URL/Fetch/streams/crypto/timer adaptations, secure-context exposure,
origin policy and browser cancellation behavior.

**Done when:** parser/script ordering and teardown fixtures pass, navigation cannot
run old-document callbacks, an ordinary script exception does not disable unrelated
page functionality, and deadline termination has an explicit recovery outcome.

### 07 — Build a small, real WebIDL generation pipeline

**Depends on:** 06.

Pin IDL inputs and licenses. Generate conversions, prototypes, inheritance,
brand checks, dictionaries, unions, overloads, exceptions and exposure conditions
incrementally under the [Web IDL standard](https://webidl.spec.whatwg.org/).
First migrate a real small interface family; implement semantic hooks in Rust.
Define wrapper identity per realm, GC tracing/rooting, weak wrappers, node lifetime,
cross-realm handling and reentrant native calls before scaling the generator.

**Done when:** deterministic regeneration and IDL behavior tests pass for that
family; unsupported IDL constructs fail generation explicitly. No interface is
declared supported solely because its class/prototype was generated.

### 08 — Expand DOM, events and browser Web APIs by audited families

**Depends on:** 07.

Move Node/Element/Document, collections, attributes, selectors, mutation, events,
observers, location/navigation and HTML element families through the generated
layer in small subtasks. Preserve existing Range/fragment/live-collection work.
Complete event paths, capture/bubble/default actions and exception behavior.
Use the ledger to route all enabled Servo APIs, including binary/stream/crypto and
less common interfaces, to a semantic implementation or an explicit blocker.

**Done when:** each migrated family passes its selected upstream IDL/behavior tests
and real framework interactions; wrapper GC, detached nodes, DOM budgets and
reentrant mutation remain correct. No missing-feature success shims are counted.

### 09 — Preserve navigation and browser integrations

**Depends on:** 08.

Implement acknowledged load/stop/reload/bypass-cache/traversal/history clearing,
error pages, dirty-state/unload confirmation, auth prompts, title/favicon/status,
dialogs, downloads, custom requests and popup intent. Keep privileged external
scheme dispatch in the browser. Retain user scripts, reader/dark-style hooks and
download credentials without exposing HttpOnly data to scripts. Frame/opener
semantics are completed in 14; do not call the popup work finished before then.

**Done when:** existing navigation, safety, capability and download behaviors pass
against Wega; cancellation preserves drafts, failures stay visible, and completion
cannot be satisfied by unrelated load events. Profile/session formats remain readable.

### 10 — Render internal pages natively and preserve chrome/viewer UX

**Depends on:** 09.

Route `wega://newtab`, history, bookmarks and settings to native views. Preserve
address/history/session semantics, search/actions, themes, keyboard focus,
accessibility, movable tabs and panes. Keep untrusted page navigation from gaining
internal privileges. Preserve container windows and Docker/noVNC workflows.
Provide a developer Wega-only startup path with explicit missing-feature errors;
this is useful integration evidence, not a parity release.

**Done when:** all internal pages work without initializing Servo, chrome has no
privileged web-document dependency, and existing profile/container/viewer flows
retain their behavior. Old internal URLs still restore meaningfully.

### 11 — Complete input, native controls and accessibility transport

**Depends on:** 08–10.

Complete pointer/touch/wheel/keyboard focus, IME composition, text editing, form
validation/reset/disabled state, select/listbox, date/color controls as exposed,
checkbox/radio behavior, file selection and real File/Blob multipart uploads.
Browser-issued file capabilities must not reveal unrestricted paths. Route the
Blitz accessibility tree and actions into the native window with stable document
IDs, coordinate transforms and focus synchronization.

**Done when:** mouse, keyboard, IME and accessibility actions manipulate the same
real controls; canceling a file dialog leaks no grant. Screen-reader runtime checks
cover page and chrome on all release platforms; viewer upload/clipboard still works.

### 12 — Implement selection, editing, clipboard, find and drag/drop

**Depends on:** 11.

Use text-layout hit testing for multi-node selections; implement Range mutation
and geometry, Selection, copy/cut/paste, drag/drop and exposed rich-editing APIs.
Keep native clipboard/file actions browser-mediated with activation/permission
rules. Connect native find to actual ranges and selected-match geometry. Retest
reader/user-script behavior after DOM replacement.

**Done when:** forward/backward selections, bidirectional text, non-Latin input,
cross-paragraph copy, editable content, scrolled/zoomed find and clipboard denial
work on the real layout; unsupported editing commands remain explicitly tracked.

### 13 — Close rendering and scrolling gaps with reference tests

**Depends on:** 11, 12.

Audit supported Servo CSS/SVG/text/image features against Blitz and pinned Stylo.
Implement gaps by family: intrinsic sizing, flex/grid/table, positioning, overflow,
fonts/shaping, responsive images, SVG, masks, filters, compositing, transforms,
animations, queries and remaining audited layout behavior. Keep node geometry,
selection and accessibility coordinates consistent. Patch upstream adapters with
documented provenance; do not substitute page-specific CSS hacks for conformance.

**Done when:** pinned reftests and interaction checks pass for each claimed family,
including scaling, scrolling and font fallback. Maintain tolerances and platform
font notes; a screenshot alone does not establish input or API correctness.

### 14 — Add browsing contexts, iframes and site isolation

**Depends on:** 05, 08–13.

Build the browser-owned frame tree and process placement rules. Start with real
same-origin frames, then cross-origin/site workers, WindowProxy access rules,
about:blank/srcdoc/inherited/opaque origins, frame navigation, sandbox and policy
inheritance, `postMessage`, structured clone, opener/noopener and frame history.
Preserve synchronous same-origin access where required. Route composition,
hit testing, input, focus, accessibility and tool requests across process boundaries.

**Done when:** cross-site data remains inaccessible even to a renderer sending
forged IPC; same-origin frame/popup interactions work; targetOrigin and transfer
rules hold; frame crashes/navigation cannot corrupt another document. Test actual
frame rendering instead of retaining visible-iframe fallback.

### 15 — Implement custom elements and Shadow DOM

**Depends on:** 08, 13, 14.

Add custom element registration/upgrade/reactions, shadow roots, slot assignment,
scoped style and selector behavior, event retargeting/composed paths, focus and
accessibility integration. Implement in separately gated increments with Blitz/
Stylo changes where necessary.

**Done when:** representative component libraries and relevant upstream tests work;
light/shadow tree mutations, disconnected upgrades and closed-root exposure follow
the supported semantics. Known upstream blockers remain visible in the ledger.

### 16 — Complete storage and browser data management

**Depends on:** 04, 08, 14.

Create authoritative storage keys with container/profile, top-level partition and
origin context. Verify localStorage events, sessionStorage browsing-context scope,
cookie inventory/HttpOnly inspector and per-site/global deletion. Add IndexedDB
transactions/version changes/structured data and CacheStorage Request/Response
semantics as separate packages, using SQLite where appropriate. Handle quota,
eviction, concurrent tabs, locking and crash recovery. Keep old Servo data intact;
implement only validated import formats and explicitly report unavoidable re-login.

**Done when:** origins and containers cannot access each other's data, interrupted
writes recover, clear-data removes the advertised stores, and opaque origins/stale
grants are rejected. Test migrations on copies; never silently discard legacy data.

### 17 — Add workers and offline execution

**Depends on:** 06–08, 14, 16.

Implement structured clone/transfer primitives before DedicatedWorker. Give each
agent its own V8 isolate/event loop, module loader, limits and termination path.
Audit and implement SharedWorker and ServiceWorker separately, including matching
scope, update/install/activate, fetch interception, cache integration and removal.
All worker authority uses the same browser-issued network/storage context.

**Done when:** transferable ownership, errors, termination, navigation cleanup and
quota limits work; tested offline reload/update behavior matches the declared
service-worker surface. A second isolate alone is not worker API parity.

### 18 — Implement Canvas 2D

**Depends on:** 07, 08, 13, 16.

Adapt the existing Skia backend for canvas state, paths, text, transforms, images,
compositing, pixel data, serialization and resizing. Implement origin-clean/tainted
canvas rules and memory accounting. Add OffscreenCanvas where required by the
audited baseline, connected to worker transfer behavior from 17.

**Done when:** pixel/reftests and drawing apps exercise the real surface, cross-origin
readback is blocked, resize resets state correctly, and destroyed canvases release
memory. Separate supported API coverage from GPU acceleration.

### 19 — Implement WebGL and resilient composition

**Depends on:** 13, 14, 17, 18.

Choose a WebGL implementation through a focused backend feasibility subtask;
`wgpu` alone is not a WebGL implementation or a GLSL/API compatibility layer.
Specify shader translation, validation, origin checks, context loss/restoration,
resource quotas and platform driver isolation. Extend composition for frame/canvas/
video surfaces, damage and screenshot readback. Keep the software/reference paint
path available. WebGPU is a separately audited extension unless baseline proves otherwise.

**Done when:** audited WebGL behavior and graphics fixtures pass on real macOS,
Linux and Windows backends; device/context loss recovers; screenshot, zoom and
fullscreen remain correct. Backend profiler/capture tools have Wega equivalents.

### 20 — Implement media playback and audio

**Depends on:** 04, 05, 14, 17, 19.

First define a common media-service contract and capability/codec reporting;
validate backend spikes for macOS, Windows and Linux before choosing libraries.
Build real demux/decode/audio output, bounded buffers, A/V synchronization, controls,
seeking, autoplay, volume/mute, fullscreen and failure reporting. Preserve media
sessions, native metadata/commands and verified WebM/WAV behavior. Audit MSE, Web
Audio and related enabled APIs; split those into explicit subtasks. OS decoders or
FFmpeg/VAAPI names alone do not supply a complete web media stack.

**Done when:** actual playback/seek/pause/teardown and media-session tests pass per
platform, including software fallback and packaged codec closure. Report hardware
acceleration separately; physical OS media controls require their own evidence.

### 21 — Complete permissions and native/device capabilities

**Depends on:** 02, 05, 14, 20.

Port notifications, permission/auth dialogs, protocol registration, external apps,
fullscreen and all audited native delegate requests away from Servo types.
Implement enabled capture/WebRTC/device functionality found by 01, preserving its
documented backend limits. Carry trusted requesting-frame identity and activation;
deny stale/opaque requests. Add acknowledged revocation and active-resource shutdown
before offering persistent allow/stop-access controls. Follow the existing
[permission requirements](permission-engine-requirements.md).

**Done when:** cross-origin frames/workers cannot impersonate the top-level origin,
navigation cancels old prompts, device denial/stop/clone cleanup is verified, and
notification/device/OS integrations have explicit platform runtime results.

### 22 — Port every existing tool and automation endpoint

**Depends on:** 03, 09, 12, 14, 16, 19, 21.

Bridge V8 Inspector plus DOM/style/layout/network/storage/accessibility/resource
data into existing native panels. Preserve evaluation, breakpoints/stepping, source
inspection, HAR, cookie inspection and supported WebDriver commands with real load
completion/cancellation. Remote debugging stays opt-in, local and authenticated.
V8 Inspector does not implement Firefox's wire protocol or CDP DOM/CSS/Network by
itself: retain the current remote-tools subset through an adapter if required for
parity. Any incompatible CLI/protocol replacement is a recorded breaking change
requiring an explicit scope decision before retirement, not a silent success.

**Done when:** all baseline tools/endpoints operate on Wega documents, frames and
workers; unsupported commands return errors; closing/navigating a target invalidates
handles. Loopback browser content cannot invoke privileged debugging operations.

### 23 — Make the standalone application and unified workspace the default

**Depends on:** 01–22 and no unresolved required parity rows.

Make Wega the sole runtime engine in the application, migrate saved engine choices
without dropping tabs/preferences, and remove automatic fallback and selectors.
Merge root and engine workspace membership, patches, profiles and lockfiles after
resolving the full dependency graph. Preserve core-only/test-harness builds, vendor
checks, generated bindings, offline fixtures and helper binary packaging. A temporary
build-only historical Servo comparison path may remain until 26, outside the default
workspace if dependency incompatibility requires it.

Migrate remaining Wega-owned Python application/tooling code and handwritten
JavaScript feature logic to Rust before declaring the language migration complete.
Include the Docker gateway and build/package/test tooling in that inventory;
website JavaScript, generated binding glue and permitted upstream dependencies
are distinct from Wega-owned semantic implementations.

**Done when:** a clean standalone build restores existing profile copies, all
baseline product flows run with no Servo initialization/fallback, and the unified
workspace produces browser and worker through documented locked commands.

### 24 — Measure and tune resource use without losing behavior

**Depends on:** 23; collect incremental measurements from 03 onward.

Compare clean build time, artifact sizes, startup, page load, input latency, idle/
active CPU and memory with the frozen baseline. Measure browser plus all descendant
processes at 1/5/10 tabs, including frames/workers/media; report shared-memory
double counting and GPU accounting limits. Distinguish JS heap, content-process RSS,
whole-tree RSS, decoded resources and surfaces. Set numerical regression budgets
from step 01 measurements before accepting optimizations; track article/app goals
as targets until verified. Fix busy loops and bound caches before chasing microbenchmarks.

**Done when:** reproducible workloads retain required functionality and meet the
recorded budgets; freeze/thaw, repeated navigation, close/reopen and soak tests show
no unbounded growth. Low RSS from a crashed or incomplete page is a failed result.

### 25 — Validate release platforms, packaging and Docker

**Depends on:** 23, 24.

Run the standalone core, worker, native, framework, WPT/reftest, security, tooling,
media and migration gates on macOS, Linux and Windows. Verify signed application
layout, helper/service discovery, sandbox entitlements, Linux desktop bundles,
Windows DLL/codec closure and required notices/licenses. Exercise Docker/noVNC
render/input/clipboard/resize/uploads/downloads/container flows with software
rendering and any supported GPU path. Validate installed packages, not just build trees.

**Done when:** exact-source CI and packaged runtime results exist for every release
target; platform-specific manual checks are attached or remain explicit release
blockers. Do not treat local macOS success as cross-platform certification.

### 26 — Retire Servo and close the migration

**Depends on:** 01–25, complete ledger and release evidence.

Remove Servo dependencies, historical build path, delegates/GL glue, obsolete flags,
vendor patches, tests and packaging assets only after their replacements are
verified. Port reusable native media/notification/tooling code before deleting
files; whole files are not automatically disposable. Keep Stylo/Blitz and their
required notices. Update architecture, engine strategy, compatibility docs, README,
Docker/operator instructions, CLI migration notes and third-party notices.

**Done when:** no build/runtime/package path depends on Servo/SpiderMonkey/WebRender
or GStreamer through the retired backend, no fallback exists, and every required
ledger row is verified on its declared targets. Re-run final gates after cleanup;
publish the exact supported surface and any agreed breaking changes. New platform
features may still grow beyond this finite parity baseline.

## 5. How each step is accepted

- Record behavior, inputs, source revision/dirty patch, platform and output
  artifacts. Update the ledger and this checklist after each completed subtask.
- Run relevant existing core/worker tests and real native integration checks for
  the changed boundary. Preserve all nine native suites: smoke, theme, capabilities,
  toolbox, download, webdriver, media, capture and engine. Keep container/viewer
  suites in the release matrix as well.
- Expand pinned upstream WPT tests for new claims; track pass/fail/timeout/skip and
  expected failures explicitly. Add the automation needed for tests requiring
  [WPT testdriver](https://web-platform-tests.org/running-tests/from-web.html).
- Cover failure paths: denied permission, bad IPC, expired document, aborted load,
  quota exhaustion, process exit and corrupted storage. Never use the real profile
  for destructive deletion, recovery or migration fixtures.
- Use offline fixtures for deterministic gates and real sites as additional
  compatibility evidence. Site loading, rendering, interaction, sign-in and media
  are separate claims. No fallback is allowed to satisfy a Wega gate.
- Apply format/lint/vendor/license checks appropriate to each change. Evidence
  belongs to the final tested source, not an earlier commit or a different worker.
- Do not remove a failing test or lower a threshold just to complete a step.
  Record upstream/API limitations and split follow-up work without marking parity done.

Current commands before workspace unification (verify flags when each step starts):

```sh
cargo test --locked --no-default-features --lib
cargo test --locked --manifest-path engines/Cargo.toml --target-dir target/wega-engine
python3 scripts/build-wega-engine.py --debug
cargo build --locked --features test-harness
python3 scripts/native-tests.py --suite all
python3 tests/containers_smoke.py --binary target/debug/wega
```

These commands are a starting set, not evidence that they passed during planning.
Step 03 must add direct-Wega assertions before reusing native tests as parity gates.

## 6. Progress checklist and first work package

All items are pending:

- [ ] 01 Inventory and baseline
- [ ] 02 Engine contract and lifecycle
- [ ] 03 Worker harness and observability
- [ ] 04 Authoritative network/storage access
- [ ] 05 Sandbox and supervision
- [ ] 06 Document host and scheduling
- [ ] 07 WebIDL generation and ownership
- [ ] 08 DOM/events/Web API families
- [ ] 09 Navigation and browser integrations
- [ ] 10 Native internal pages and chrome/viewer
- [ ] 11 Input, controls and accessibility
- [ ] 12 Selection/editing/clipboard/find
- [ ] 13 CSS/layout/rendering conformance
- [ ] 14 Frames and site isolation
- [ ] 15 Web components
- [ ] 16 Storage and data management
- [ ] 17 Workers and offline execution
- [ ] 18 Canvas 2D
- [ ] 19 WebGL and composition
- [ ] 20 Media and audio
- [ ] 21 Permissions/native/device APIs
- [ ] 22 Tools and automation
- [ ] 23 Standalone default and unified workspace
- [ ] 24 Resource validation and tuning
- [ ] 25 Cross-platform release validation
- [ ] 26 Servo retirement

**Start with 01.a:** create the source-backed feature ledger, identify every
Servo-dependent product action and enabled API family, and map the existing tests.
**Then 01.b:** run and record baseline behavior in fresh isolated profiles.
**Then 01.c:** audit profile/data migration and choose the first measurable engine
contract slice for 02. This planning change does not remove Servo or alter runtime behavior.
