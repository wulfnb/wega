# Wega UI/UX proposals — 23 October 2026

This is a second design pass after the [22 October review](ui-ux-review.md),
whose 16 findings are implemented. It focuses on what that review did not cover
in depth: the **Docker web viewer** people use every day, the **Windows build**,
and how the desktop chrome looks and reads today. The findings below preserve
the proposal baseline; the implementation status tracks subsequent changes.

P1 blocks or breaks a common task. P2 is real friction or something that looks
unfinished. P3 is polish.

## Implementation status — 23 October 2026

These follow-ups implement the first priority group and several related chrome
improvements. It does **not** mark the whole proposal complete. See
[implementation and verification](ui-ux-implementation.md#23-october-follow-up).
The original follow-ups were committed as `7d6b1bc` on
`feature/windows-gateway-and-design-docs`. The CI corrections were committed as `6256600` before the next follow-up.
The new allocation-report, shortcut-ranking and macOS launch changes are tracked
in the implementation log; no push has been made by the agent.

| Item | Current status |
| --- | --- |
| A1 | Implemented: visible connection card, 15-second deadline, immediate retry, automatic 1/2/5/10-second backoff, session-expiry redirect, classic bootstrap for module failure. Unit checks cover silent sockets and missing modules; a real container stop/restart through the production proxy remains to test. |
| A2 | Implemented floating controls, full-height remote viewport, View/Account menus, idle hiding and Alt+Shift+V focus shortcut. Real touch and fullscreen behavior still need device validation; the two-step keyboard acceptance criterion is not yet certified. |
| A3 | Implemented request-scoped upload-to-picker IPC, authenticated chooser polling, direct file selection/cancellation and drop-on-screen handoff. Native Servo selection, gateway guards, browser/auth UID access and viewer regressions checked separately; full live Docker engine/device validation remains. |
| A6 | Virtual keyboard and coarse-pointer targets remain. An explicit Pan/zoom viewer mode now consumes local one-finger pan and two-finger pinch without sending input to the remote page; reset restores fit without changing framebuffer resolution. Real Chrome/noVNC gestures and mode restoration pass. Physical iOS/Android keyboard, IME and gesture checks remain. |
| A5 | Auto now reads audited noVNC render-queue age/actions/frames and completed paints, with congestion/recovery hysteresis. Idle time never raises quality. Sharp/Fast and the remembered device preference remain. Real queued image decoding, painted pixels and quality recovery pass in Chrome. |
| A4 | Added user-gesture paste handling and a remote-copy offer with explicit Copy here; permission/API failures open selectable text. Regression checks pass; shortcut delivery through a live noVNC canvas still needs validation. |
| A7 | Implemented mint sign-in action, password visibility control, Caps Lock warning and plain-language 401/429 errors. |
| B1 | Implemented effective appearance on view creation and live updates to every loaded view. Native theme suite checks website `matchMedia` for all six themes. Auto still needs a real OS appearance-toggle check. |
| B2 | Implemented telemetry hidden by default; persistent Advanced setting enables CPU/RAM. Engine strings removed from the ordinary footer. About Wega is now available from the browser menu. |
| B3 | Partial: address-bar connection button, HTTP warning, anchored site panel, connection explanation and confirmed current-site clearing. Exact-origin Block/Ask permission controls and full-host address emphasis are implemented. Remembered grants and revocation of existing device access are not provided; the callback does not identify requesting-frame origin. |
| B4 | Implemented the proposed modal-sheet alternative: centered, constrained width, dimmed backdrop, Close/Escape/backdrop dismissal and focus restoration. It remains a native settings surface. |
| B5 | Implemented readable storage categories, site-first rows, destructive Remove last, and “Cookies on this site”. Site data now opens below the address bar from the connection control or existing menu. |
| B6 | Partial: simple theme names and selected tab-layout controls. Three-color swatches are now included; explicit Glass Light/Dark choices are hidden when no native backdrop is available (Auto retains its opaque fallback). |
| B7 | Compact brand and contextual close glyphs implemented, including keyboard focus. New tab already uses a full-width sidebar row and 36-point top-strip target. The duplicate sidebar bookmark action is removed; History remains accessible. |
| B8 | New-tab shortcut grid combines saved bookmarks and most-visited history (up to eight), saved favicons/fallbacks and five recent links. Duplicate navigation removed; search requests HTML autofocus. New-tab commands now share the search page and transfer keyboard focus to it; native key-input verification passes on macOS. Persisted visit counts now rank history shortcuts, with recency breaking ties; clearing history removes the counts. Legacy profiles keep their existing recency order until new visits are observed. |
| B9 | Library fallback icons implemented. Network details move below the table in narrow docks; default columns fit available width, user-sized columns retain scrolling, scrollbars remain visible, and filter syntax is in a tooltip. |
| C1 | ANGLE, packaged DLLs, embedded icon and Windows launch CI are already present in this branch. No branch merge was performed; real-PC validation remains required. |
| C2 | Windows CI/release builds enable media and bundle the explicit plugin/dependency closure from a pinned, checksum-verified GStreamer SDK, with license texts and a manifest. Required Microsoft CRT files come from the Visual Studio REDIST directory with separate notices. Per-element network/codec fallback messages and safe download links pass native fixture checks. Actual Windows playback and SDK-free packaging must pass the new Windows runtime check after push; the currently published preview still lacks media. |
| C3 | Installer, associations and macOS launch-event verification remain. Windows now uses a profile-specific named mutex and message-only window to forward HTTP/HTTPS/file URLs to the existing process. Real Win32 adapter cross-compilation passes; a Windows runtime test checks web/Unicode HTML delivery, tab creation and unchanged primary process. Actual Windows execution, default-browser selection and release signing remain required. |
| C4 | Implemented confirmed URI dispatch with Windows ShellExecuteW and Linux xdg-open; macOS open is retained. Windows/Linux runtime checks remain required. |
| C5 | Windows 11 Mica is implemented through DWM, gated by API support, high contrast and the transparency preference; unsupported configurations use opaque themes. Adapter cross-compilation passes. Actual compositor, wallpaper contrast and OS preference changes still need Windows checks. |
| D | Confirmed absent CSS mask support and Vector mask-based black backgrounds. A scoped MediaWiki fallback reuses the same-origin stylesheet’s original SVGs; native screenshot checks verify transparent corners and a painted glyph. This is a compatibility fallback. A fresh live Wikipedia verification and general engine CSS-mask support remain open. |

### 31 October 2026 pass

A third pass added a shared component layer (`src/components.rs`: underline tabs,
filter chips, primary and destructive buttons, inline messages, empty states,
dialog buttons) and the palette roles `on_accent`, `danger_bg` and `success`,
with a WCAG AA contrast test for all six themes. It applies them to the main
menu, right-click menu, find bar, permission and website dialogs, settings,
site data, downloads, containers, internal pages, the developer toolbox and the
load-error page (`src/net_error.js`). Icons are regenerated per platform grid by
`scripts/generate-icons.py`.

Correction to the table above: A7's mint sign-in button was missing from
`viewer.css` until this pass; it is now styled.

Still open from that pass: anchoring Downloads and Containers as panels instead
of windows, a persisted download-folder setting, viewer toasts and a counting
reconnect timer, caching `/novnc/` assets, the website's OS-aware download
button, and the Impressum address and e-mail placeholders, which need the
owner's details.

Current implementation and verification limits are recorded in the [24 October checkpoint](ui-ux-implementation.md#remaining-proposal-implementation--24-october-2026). Remembered grants and active revocation require the [engine changes](permission-engine-requirements.md).

## Evidence

- Native suites (smoke, theme, capabilities, toolbox) run on macOS against
  `main` (7da8a7b) with the test harness, all passing; 32 captures.
- Captures of `wega://newtab`, an English Wikipedia article and the live Docker
  sign-in page, taken with `wega --capture`.
- The Docker viewer (`docker/viewer/`) reviewed in source and in daily use
  behind Cloudflare and Traefik on a home server.
- The Windows release built and started on a Windows PC by the project owner;
  packaging and platform code read in source.

Selected screenshots are in [`ui-ux-2026-10-23/`](ui-ux-2026-10-23/).
Not covered: screen readers, real touch devices, Linux desktops.

## Design direction

Wega has a clear identity (emerald glass, the globe icon) and capable developer
tools. What holds it back is that it still **looks and talks like a test bench**:
engine names and CPU counters in everyday chrome, floating debug-style windows,
internal type names in labels, and a web viewer that stacks a second toolbar on
top of the browser's own. The proposals below follow three rules:

1. **One surface per task.** No floating panels over the page for settings or
   site data; no two toolbars for one browser.
2. **Everyday first, developer on request.** Telemetry and engine details move
   behind an explicit developer setting or into the toolbox.
3. **Every state is visible.** Loading, failure and success each have a message
   and a next step; nothing waits silently.

## A. Docker web viewer

### A1. P1 — The viewer can wait on "Connecting…" forever

**Observed:** if a script module fails to load or the WebSocket never opens, the
header keeps saying *Connecting…* with an empty dark screen and no Reconnect
button. The button appears only after a `disconnect` event, which never fires
if the connection was never made. Hit in production on 23 October, when a
module request returned 500. [Status handling](../docker/viewer/viewer.js#L10).

**Proposal:**

- Show a centered state card on the screen area instead of an empty canvas:
  spinner and *Connecting to your browser…*.
- After 15 s without a connection, switch the card to *Can't reach the browser*
  with **Try again** and a short reason where known (session expired → go to
  sign-in; server not responding; module failed to load → reload page).
- After a drop, reconnect automatically with backoff (1, 2, 5, 10 s) and say so:
  *Reconnecting in 5 s… Reconnect now*.
- Catch module-load failure with a tiny non-module bootstrap script so even a
  broken bundle shows the error card.

**Acceptance:** blocking `/novnc/core/rfb.js` or `/websockify` shows the error
card within 15 s; stopping and restarting the container recovers without a
click.

### A2. P1 — Two toolbars for one browser

**Observed:** the viewer header (brand, status, Account, Sign out, Downloads,
Upload, Clipboard, View, Full screen) sits above Wega's own tab and address
bars, so the screen shows two stacked toolbars with seven equal-weight buttons
in the first. *Sign out* sits between everyday actions with no distinction.
[Header](../docker/viewer/index.html#L3).

**Proposal:**

- Replace the header with a **single floating pill** at the top center (about
  32 px tall): Wega icon + connection dot, then *Clipboard*, *Upload*,
  *Downloads*, a *View* menu (Fit, Actual size, Resize, Full screen) and an
  account menu (Account settings, Sign out).
- The pill hides after 2 s of no pointer movement near the top and in full
  screen; it reappears on hover or with a keyboard shortcut.
- The remote desktop then gets the full window height, and the only toolbar the
  user reads is Wega's own.

**Acceptance:** at 1280×800 the remote screen gains the 38 px header height;
every former header action is reachable by keyboard in two steps or fewer.

### A3. P2 — Uploading a file takes five steps

**Observed:** Upload → choose file → wait → copy the remote path → open the
site's file chooser in Wega → paste the path. [Upload dialog](../docker/viewer/index.html#L6).

**Proposal:** when a page in Wega opens a file chooser, the viewer shows *A page
is asking for a file* with **Choose from this computer**; the chosen file is
uploaded and handed to the chooser directly. Keep drag-and-drop onto the screen
as a shortcut. The current path dialog stays as the fallback.

### A4. P2 — Clipboard needs a dialog round trip

**Proposal:** use the async Clipboard API when the page is focused and permitted:
Ctrl/Cmd+V in the viewer sends local text, and text copied inside Wega is
offered as *Copied in browser — Copy here* in a toast. The dialog remains for
browsers without permission.

### A5. P2 — Picture quality is fixed

**Observed:** quality level 6 and compression level 2 are hard-coded.
[Settings](../docker/viewer/viewer.js#L19).

**Proposal:** a *Quality* choice in the View menu: Auto (default; lowers
quality when frames queue), Sharp, Fast. Remember it per device.

### A6. P2 — Phones and tablets

**Proposal:** a keyboard button in the pill to bring up the on-screen keyboard,
two-finger scroll and pinch-to-zoom in Actual size mode, and a first-visit
hint for these gestures.

### A7. P3 — Sign-in page

**Observed:** the *WEGA* label repeats the heading's message; the *Sign in*
button is dark on dark and does not read as the primary action; there is no way
to show the password. ![Sign-in page](ui-ux-2026-10-23/docker-login.jpg)

**Proposal:** drop the small *WEGA* label, make the button the brand mint with
dark text, add a show-password toggle and a Caps Lock warning, and show
rate-limit and wrong-password errors in the notice line in plain words.

## B. Desktop browser chrome

![Sidebar layout](ui-ux-2026-10-23/chrome-sidebar.jpg)

### B1. P1 — Websites are always told the user prefers dark

**Observed:** every new tab calls `notify_theme_change(servo::Theme::Dark)`, and
changing the theme only restyles Wega's own pages. With Paper or Glass Light,
or with Auto on a light system, sites that follow `prefers-color-scheme`
(Google, GitHub, Wikipedia) still render dark. [Source](../src/browser.rs#L497).

**Proposal:** send the effective appearance (`theme.dark(system_dark)`) when
creating each view, and notify all open views when the theme or the system
appearance changes.

**Acceptance:** a page with a `prefers-color-scheme: light` rule renders light
under Paper and Glass Light, and switches live when the theme changes.

### B2. P2 — Developer telemetry in everyday chrome

**Observed:** the status bar always shows *Servo · WebRender* and
*CPU 97.0% · RAM 288 MiB*; the sidebar footer shows *1 tab · Servo 0.7*.
[Status bar](../src/ui.rs#L495), [resources](../src/resources.rs#L79).

**Proposal:** by default the status bar shows only link previews, transient
notices and zoom. Add *Show performance in status bar* under Settings → Advanced
for the CPU/RAM readout. Move engine and version to an *About Wega* page.

### B3. P2 — The site security indicator is a gray word

**Observed:** a small gray *HTTPS*, *HTTP* or *PAGE* label sits before the address.
Plain HTTP is not marked as insecure, and the label is not a control.
[Source](../src/ui.rs#L618).

**Proposal:** replace it with a button: lock icon for HTTPS, *Not secure* with
a warning icon for HTTP, the Wega icon for `wega://` pages. Clicking it opens a
**site panel** anchored to the address bar: connection, permissions, cookies and
site data, and *Clear data for this site* (this replaces the floating Site data
window, B5). Show the registrable domain in full contrast and the rest of the
URL muted.

### B4. P2 — Settings is a floating debug window

**Observed:** *Browser settings* is an `egui::Window` with a collapse arrow,
resize grip, no backdrop, and it covers the sidebar and page at an arbitrary
position. ![Settings](ui-ux-2026-10-23/settings-window.jpg)
[Source](../src/capabilities_ui.rs#L264).

**Proposal:** open settings as a page (`wega://settings`) in its own tab, with
the five existing categories as a left navigation, like other browsers. If it
stays an overlay, make it a centered modal sheet with a dimmed backdrop, no
collapse arrow and a fixed size.

### B5. P2 — Site data shows internal names

**Observed:** stored sites list the Rust debug value `StorageType(Local)`; the
heading reads *Cookies for current URL (including HttpOnly)*; *Delete* comes
before the site name in neutral styling. ![Site data](ui-ux-2026-10-23/site-data-window.jpg)
[Source](../src/capabilities_ui.rs#L120).

**Proposal:** show human labels (*Local storage*, *Cookies*, *Cache*), put the
site name first and **Remove** last in the row, styled as a destructive action,
and title the section *Cookies on this site*. Keep the HttpOnly detail in the
developer toolbox, where it already exists.

### B6. P2 — Theme picker

**Observed:** names mix platforms and brand voice (*macOS Glass · Auto*,
*Wega · Emerald Glass*), there are no previews, and *Tabs on top / Tabs in
sidebar* do not show which layout is active. [Names](../src/model.rs#L25),
[layout buttons](../src/capabilities_ui.rs#L282).

**Proposal:** cards with a three-color swatch and plain names (*Auto*, *Glass
Light*, *Glass Dark*, *Midnight*, *Paper*, *Emerald*); show only themes that
render properly on the current platform (C5). Make tab placement a segmented
control with the active option selected.

### B7. P2 — Sidebar space and controls

**Observed:** the brand block (*Wega / Your space on the web*) uses about 70 px
above the tabs; every tab shows its close button; the bottom row has four
unlabeled icons (menu, bookmarks, history, terminal), and bookmarks and
downloads also appear in the top bar. The *+* in top-tab mode is a very small
target. [Brand](../src/ui.rs#L399).

**Proposal:** a compact brand row (icon only, or none); show close on hover and on the active tab; give the bottom
icons tooltips and accessible names, and drop duplicates of top-bar actions;
make *New tab* a full-width row in the sidebar and a 28 px button in top-tab
mode.

### B8. P3 — New tab page

**Observed:** a plain document: underlined links, a bulleted *Saved pages* list,
navigation links that repeat the chrome, and a fixed *Search Google*
placeholder. ![New tab](ui-ux-2026-10-23/new-tab.jpg)
[Source](../src/capabilities_ui.rs#L42).

**Proposal:** a centered search field with the chosen engine's name, focused on
open; below it a grid of up to eight tiles (favicon and title) from bookmarks
and most-visited pages; *Recent* as a short list with favicons. Remove the
duplicate navigation links. Add a search engine choice under Settings →
Browsing.

### B9. P3 — Library and developer tools polish

- History rows reserve an empty slot where a favicon should be; show the
  favicon or the generic document icon already used for tabs.
- Network: the details pane covers the *Type* column; let the table shrink
  instead. Move the always-visible filter syntax line into the filter field's
  placeholder and a tooltip.

## C. Windows build (EXE)

### C1. P1 — Startup crash and missing icon

**Observed:** `wega.exe` panicked at start with *Reading GL_VERSION failed*, and
Explorer showed a generic icon.

**Status:** ANGLE rendering, shipped DLLs, embedded icon and Windows launch CI
are present in the current checkout. No merge is needed for these source changes
in this branch. A fresh Windows build and real-PC test remain required.

### C2. P1 — No video or audio on Windows

**Observed:** the Windows release is built with
`--no-default-features --features desktop,brotli-streams`, so the `media`
feature (GStreamer) is absent and sites with video or audio do not play.
[Build](../.github/workflows/cd.yml#L161).

**Proposal:** bundle GStreamer for Windows as the macOS package already does,
or, until then, show *Video and audio are not available in this build* on media
elements instead of a silent failure, and state it on the download page.

### C3. P2 — Not installable and cannot be the default browser

**Observed:** Windows gets a portable ZIP only: no installer, no Start menu
entry, an unsigned executable (SmartScreen warning), and no way to make Wega the
default browser on any platform.

**Proposal:**

- An installer (MSIX or Inno Setup) with a Start menu shortcut and uninstall
  entry, and code signing for the executable and installer.
- Register Wega as a browser (`StartMenuInternet` and `Capabilities` registry
  keys, `http`/`https`/`.html` associations) at install time.
- *Make Wega your default browser* in Settings → Browsing. On Windows it opens
  the system Default apps page (apps cannot set it silently); on macOS it uses
  `CFBundleURLTypes` in the app bundle and asks the system for the scheme
  handler.

### C4. P2 — Links to other apps do nothing outside macOS

**Observed:** allowing a `mailto:`, `zoommtg:` or similar link shows *External
application launching is currently available on macOS*.
[Source](../src/capabilities_ui.rs#L212).

**Proposal:** open them through the shell (`ShellExecuteW` on Windows, `xdg-open`
on Linux) after the existing confirmation.

### C5. P3 — Glass themes are flat on Windows

**Observed:** native vibrancy is macOS-only, and the window is transparent only
on macOS. [Window](../src/main.rs#L78), [vibrancy](../src/native_theme.rs#L93).

**Proposal:** use Windows 11 Mica/Acrylic (`DwmSetWindowAttribute` with
`DWMWA_SYSTEMBACKDROP_TYPE`) for Glass themes, and hide them where no backdrop
is available.

## D. Engine-visible rendering

Interface icons on Wikipedia render as solid black squares (likely unsupported
CSS `mask-image`). ![Wikipedia](ui-ux-2026-10-23/wikipedia.jpg) This is an
engine gap rather than a chrome issue, but users read it as a broken browser.
Track it in [servo-capabilities.md](servo-capabilities.md) and check the next
Servo release.

## Suggested order

1. **Now:** A1 (viewer never stuck), B1 (color-scheme bug), C1 merge and test,
   C2 at least the message.
2. **Next:** A2 (single toolbar), B2 (telemetry off by default), B3 + B5 (site
   panel), B4 (settings page).
3. **Then:** A3–A5 (upload, clipboard, quality), B6–B7, C3–C4.
4. **Polish:** A6–A7, B8–B9, C5, D.

Each item should land with a capture in the native suites (desktop) or
`scripts/docker-ux-test.py` (viewer), so the visual result is checked, not just
the behaviour.
