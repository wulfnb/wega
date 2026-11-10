# Wega architecture

Wega is a modular desktop application. Its core compiles without Servo, egui,
GStreamer or a window server:

```sh
cargo test --locked --no-default-features --lib
```

## Ownership and dependency direction

```text
Native input / egui / WebDriver / feature-gated native drivers
            |
            v
Desktop coordinator (Browser) + ephemeral Chrome state
            |
            +--> Application commands --> private Profile + change events
            |          +--> injected ProfileStore --> FileProfileStore
            |          +--> DownloadPort --> download worker / curl
            +--> Session commands --> private tab selection/order
            +--> Navigation commands --> NavigationPort --> Wega Page / Engine adapter
            +--> Permission lifecycle --> PermissionPort --> Wega Engine responder
            |
            v
GPU, platform, clipboard and engine-specific developer-tools adapters
```

`src/lib.rs` is the reusable core. `application.rs` owns the persisted profile and
its injected storage port. It exposes a read-only profile and explicit commands
for layout/theme/dock, sidebar width, policy, bookmarks, visits, history clearing
and favicon updates. Application events tell the desktop coordinator when to
synchronize the engine policy and schedule saving. Neither UI code nor test
drivers can borrow the application's profile mutably. Failed loads disable saving
in the application too, even if a replacement storage adapter is permissive.

`session.rs` owns tab order and selection behind private fields. `Browser` keeps
that session private and exposes read-only access plus add/select/close/move
operations. UI actions, shortcuts, automation and engine-close notifications use
those operations. The desktop coordinator applies engine visibility/focus effects
at the transition boundary. Persisting a session removes data URLs and remaps the
selected index so a preceding transient tab cannot shift the restored selection.

`profile.rs` defines persisted bookmarks/settings, legacy URL compatibility and
normalization. `model.rs` defines persisted theme, dock and host policy types
independently of native window classes. `chrome.rs` owns ephemeral text inputs,
focus, pointer state, tab hit rectangles and library presentation state; those
fields never enter a saved profile.

`navigation.rs` validates navigation intent and dispatches it through
`NavigationPort`. `navigation_adapter.rs` and `wega_page.rs` coordinate navigation with the standalone Wega worker process and track
traversal targets. UI controls, keyboard shortcuts and WebDriver share load, reload,
back/forward and zoom operations. Engine-specific features such as JavaScript,
DOM inspection and surface rendering stay in desktop adapters rather than being
represented as artificial core interfaces.

`storage.rs` implements the `ProfileStore` port. It owns the profile lock for the
entire browser lifetime, blocks saves until a successful load, and never
substitutes interrupted temporary data for the committed profile. Writes use a
private, exclusively created temporary file, synchronize its contents, replace
the destination, and synchronize the containing directory on Unix. A failure
after replacement can mean the new data is visible but its durability is unknown;
it is reported rather than silently treated as success. Windows directory flush
is not implemented. Filesystem/hardware guarantees still apply.

`downloads.rs` is an application service with its own worker lifecycle; it has no
Servo or UI dependency. It validates URLs/ranges, keeps partial files private,
and publishes only completed results. The application checks URL and offline/host
policy before invoking `DownloadPort`. `download_actions.rs` translates Servo
cookies and coordinates that command; `downloads_ui.rs` renders the dialog and
emits actions. Download form state is separate from worker state.

`Browser` remains the desktop coordinator. Engine/GPU handles are not model
objects. Its state is restricted to the application crate; profile locking and
session structural state are encapsulated by their owners. Rendering and engine-specific developer tools remain desktop concerns. Sibling
presentation adapters can access their desktop state, but cannot mutate the
application profile or session structure. The coordinator deliberately retains
window/GPU/Servo lifetime ownership on the event-loop thread. Splitting these
handles across threads or separate crates would not improve this boundary.
New feature logic belongs in the core when it does not require engine types;
add a port where an external operation actually needs substituting.

## Commands, deadlines and permissions

UI tab actions and engine-close notifications use the same session transitions.
WebDriver jobs have a shared deadline and cancellation marker. Queue admission
and response waiting share the timeout budget; jobs still queued after timeout
are discarded before they can mutate the browser. Navigation completion is a core state machine evaluated against the engine's
snapshot after processing its event batch. Loads wait for new load progress and
completion; cached refreshes use the synchronous Started state set by Servo's
reload operation. History callbacks must match the dispatched traversal ID and
history destination. WebDriver reads titles from the active document so delayed
embedder title notifications cannot return a previous page's title. A stale or
unrelated traversal callback cannot finish a new navigation. A command already
dispatched to the content worker cannot be rolled back merely because its client disconnected.

`permissions.rs` owns a private FIFO of pending requests. Only metadata reaches
the UI. Answering includes a request ID and consumes the responder through
`PermissionPort`; stale answers cannot grant a later request accidentally. Clearing/dropping the queue
denies outstanding requests. `permission_adapter.rs` and `wega_page.rs` manage native permission
allow/deny mediation with worker processes. Grants remain request-local. Core host policy has boundary and
lookalike-domain tests. Physical-device permissions and OS prompts require native
validation; unit coverage is not evidence that those device integrations work.

## Features and test boundaries

- Default: desktop browser, Brotli streams and media.
- `--no-default-features`: core library and its tests only.
- `test-harness`: native integration drivers and their capture/state hooks.
- Multi-process model: UI process (egui/glow) coordinates sandboxed content processes (`wega-engine`, powered by Blitz DOM, Stylo CSS, Deno V8, and Skia) over `wega-protocol` IPC and shared-memory frame buffers. Servo is retired.

Production builds exclude the native test drivers. All native test flags require
`test-harness`, an explicit profile directory and exactly one test mode. The
provided runner always creates a fresh profile, rather than relying on a caller
to choose a safe path.

The core forbids unsafe Rust. Unsafe code is confined to desktop/platform
adapters and upstream dependencies. AppKit adapters must preserve main-thread and
window-lifetime requirements; retained Objective-C objects/callback targets must
outlive their registrations, and callbacks communicate through event channels.
The startup environment mutation occurs before application worker threads start.
These invariants require review whenever native bindings change.

## Change practices

Keep dependencies flowing from desktop adapters to the core. Avoid adding Servo
IDs or winit objects to persisted models. Preserve profile serde names/aliases;
add migration tests before changing a stored representation. Test failure paths
for external operations, not just successful UI flows. Run the relevant native
suite after rendering, input, engine or platform changes.

This agent performed no staging, commit or push. The shared checkout gained an
initial commit during verification; its history was left untouched. Subsequent
workflow and documentation edits remain in the working tree.

## Decision record and verification

See [the original recommendation mapping](architecture-decisions.md). Core
contract tests inject storage, navigation, download and permission adapters;
these verify behavior and error propagation without a window, GPU or processes.
Native smoke/toolbox/capability/download/WebDriver/media/theme suites validate
the concrete adapters. Platform and coverage completeness are separate release
requirements; an implemented boundary does not establish 100% code coverage.
