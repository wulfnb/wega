# Step 21 Contract: Permissions, Notifications, and Native Device Capabilities

## 1. Overview & Objectives

Step 21 implements end-to-end typed permissions, embedder permission prompts, notifications, device capture capabilities, wake locks, screen orientation, protocol handlers, and fullscreen management for Wega:
- **Permission & Policy Registry (`engines/wega/src/permissions/`)**:
  - `registry.rs`:
    - Origin-scoped grants and denials (`HashMap<(Origin, PermissionFeature), PermissionState>`).
    - Explicit denial for opaque origins (`data:`, `about:blank`, uncommitted contexts) without invoking embedder prompts.
    - Permissions-Policy / Feature-Policy enforcement checking feature enablement.
    - Pending prompt request queues with deduplication and prompt cancellation on frame/document navigation.
    - Acknowledged embedder permission revocation returning all stopped media tracks and released wake locks.
  - `device.rs`:
    - Audio and Video `MediaStreamTrack` lifecycle tracking (`Live`, `Ended`, mute states).
    - Track cloning creating child tracks linked to the parent source.
    - Explicit `stop()` and origin-wide teardown on permission revocation (`readyState = "ended"`, fires `'ended'`).
    - Screen and System `WakeLock` sentinel management (`released = true`, fires `'release'`).
- **Typed Protocol & Inter-Process Architecture (`engines/protocol/src/permissions.rs`)**:
  - Protocol version incremented to `21` with `Capability::PermissionsRevocation`.
  - Feature enumeration: `Geolocation`, `Notifications`, `Camera`, `Microphone`, `ScreenWakeLock`, `ClipboardRead`, `ClipboardWrite`, `Midi`, `Push`.
  - Permission states: `Granted`, `Denied`, `Prompt`.
  - Commands: `Command::AnswerPermission { request_id, state }`, `Command::RevokePermission { origin, feature }`, `Command::ExitFullscreen`.
  - Content events: `ContentEvent::PermissionPrompt(PermissionPromptRequest)`, `ContentEvent::NotificationCreated(NotificationOptions)`, `ContentEvent::NotificationClosed { id }`, `ContentEvent::FullscreenEntered { origin }`, `ContentEvent::FullscreenExited`, `ContentEvent::ProtocolHandlerRegistered(ProtocolHandlerRegistration)`, `ContentEvent::ExternalAppOpened { url }`.
- **Web API & DOM Semantics (`engines/wega/src/dom/semantics.js`)**:
  - `navigator.permissions.query({ name })` returning live `PermissionStatus` objects that dispatch `'change'` events when status updates or permissions are revoked.
  - `Notification`:
    - Static `Notification.permission` property reflecting origin permission.
    - `Notification.requestPermission([callback])` prompting embedder or returning resolved promise.
    - `new Notification(title, options)` with tag, icon, badge, body, and data attributes, registering and dispatching host notifications.
    - `notification.close()` signaling embedder and firing `'close'` event.
  - Media Devices & Capture:
    - `navigator.mediaDevices.getUserMedia(constraints)` returning `MediaStream`.
    - `MediaStream` and `MediaStreamTrack` classes with `kind`, `id`, `label`, `enabled`, `readyState`, `stop()`, `clone()`, and `'ended'` event listeners.
  - Screen Wake Lock API:
    - `navigator.wakeLock.request('screen')` returning `WakeLockSentinel`.
    - `WakeLockSentinel` with `type`, `released`, `release()`, and `'release'` event handler.
  - Fullscreen API:
    - Element `requestFullscreen()` requiring active user activation; rejected with `NotAllowedError` if unactivated.
    - `document.exitFullscreen()` and `document.fullscreenElement` tracking active fullscreen element.
    - Dispatches `'fullscreenchange'` events on entry and exit.
  - Screen Orientation API:
    - `screen.orientation` singleton (`ScreenOrientation`) with `type` and `angle`.
    - `lock(orientation)` and `unlock()` methods.
  - Protocol Registration & External Apps:
    - `navigator.registerProtocolHandler(scheme, url, title)` validating safe schemes (`mailto`, `web+*`, etc.).
    - External application launch requests.

---

## 2. Verification & Test Suite

All 7 tests in `engines/wega/tests/permissions.rs` pass:
1. `protocol_handler_and_screen_orientation`: Validates safe/unsafe scheme registration, external app triggers, and `screen.orientation` properties/lock.
2. `notification_lifecycle_request_and_prompt`: Tests `Notification.permission`, `requestPermission` embedder prompt answering, `new Notification()` dispatch, and `close()`.
3. `mediastream_device_capture_and_acknowledged_revocation`: Tests `getUserMedia()` device capture, `MediaStreamTrack` clone/stop lifecycle, and acknowledged embedder revocation (`Command::RevokePermission`) ending all active origin tracks and firing `'ended'`.
4. `wake_lock_lifecycle_and_revocation`: Tests `navigator.wakeLock.request('screen')`, sentinel release, and acknowledged revocation releasing locks and firing `'release'`.
5. `fullscreen_requires_activation_and_lifecycle`: Tests user activation requirement for `requestFullscreen()`, rejection on unactivated contexts, and `exitFullscreen()`.
6. `navigation_cancels_pending_permission_prompts`: Ensures document navigation cancels all pending embedder permission prompts.
7. `permissions_query_and_opaque_origins`: Tests `navigator.permissions.query()` for multiple features, status change dispatch, and immediate denial for opaque origins (`about:blank`, `data:`).

All 39 unit tests in `wega-protocol`, 18 in `wega-network`, and 29 in `wega-engine` pass.
`cargo check --bin wega` compiles cleanly with 0 errors.
