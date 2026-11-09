# Step 09: Preserve navigation and browser integrations

Branch: `feature/standalone-engine-steps`. Starts after Step 08 closeout.
This document records the contract, delivery, and verification evidence for Step 09.

## Scope & Capabilities

Step 09 preserves core browser navigation, page safety, status/favicon reporting, external scheme handling, and page tools integration across Wega content processes:

1. **Navigation Commands & Cache Control:**
   - Wire contract: `Command::Load { url, serial, bypass_cache: bool }`.
   - `NavigationPort`: added default methods `reload_bypass_cache` and `clear_history`.
   - `Page`: implemented `reload_bypass_cache` and `clear_history`.
   - `View` & Network Broker: `load_bypass_cache` forwards `bypass_cache: true` via `FetchContext`, sending `request.cache = "reload"` and `Cache-Control: no-cache` / `Pragma: no-cache` headers to bypass network caches.
   - UI: "Reload without cache" (`Shift+R`) is enabled for Wega tabs.

2. **Session History Clearing:**
   - Protocol: `Command::ClearHistory`.
   - Engine DOM: `globalThis.__wega_clear_history()` resets `historyEntries` to just the current location, resets `historyIndex = 0`, and reports `(0, 1)` to host DOM history tracking.
   - Host `Page`: clears host-side history stack and sends `Command::ClearHistory`.

3. **Status Bar & Hover Integration:**
   - Engine pointer handling: on hover change (`hover_changed`), inspects the active node chain for `<a>` tags with `href`, resolves the target URL, and emits `PageEvent::Status(Some(resolved_url))`. Clears status when the pointer moves off anchor elements.
   - Protocol: `ContentEvent::Status(Option<String>)`.
   - Chrome UI: updates `self.status` and bottom status bar when hovering links in active Wega tabs.

4. **Favicon Discovery & Management:**
   - Engine: detects `<link rel="icon">` and `<link rel="shortcut icon">` elements via document queries, with fallback to `/favicon.ico` on HTTP/HTTPS documents. Emits `PageEvent::Favicon(Url)`.
   - Emits dynamically upon document creation and when the document load event fires.
   - Protocol: `ContentEvent::Favicon(String)`.
   - Chrome UI: resolves favicon against current tab URL, looks up cached texture from `self.profile.favicons`, loads into `self.favicons`, and requests redraw.

5. **Authentication Requests:**
   - Engine: detects HTTP 401 responses, parses realm from `WWW-Authenticate`, and emits `PageEvent::Authentication { url, realm }`.
   - Protocol: `ContentEvent::Authentication { url, realm }`.
   - Chrome UI: presents modal authentication prompt to user via `self.dialogs.push_prompt`.

6. **Privileged External Scheme Dispatch:**
   - External schemes (`mailto:`, `tel:`, `sms:`, `zoommtg:`) requested by content are intercepted by the browser in `wega_ui.rs` and dispatched only with explicit user confirmation via `Prompt { decision: Decision::External(...) }`.
   - Web content cannot invoke arbitrary host handlers.

7. **User Scripts & Injected Styles:**
   - `Command::Policy`: carries `user_script: String`.
   - Engine: evaluates `user_script` in global scope on document creation. Updates live across all tabs when altered in settings.
   - Chrome UI: enabled "Reader view" (`reader.js`) and "Toggle page dark style" for Wega tabs via scoped script evaluation.

8. **Downloads with Retained Credentials:**
   - Engine emits `ContentEvent::Download { url, cookies }` with Netscape cookies from the engine cookie jar, without exposing `HttpOnly` cookie tokens to DOM JavaScript.

## Verification Evidence

1. **Protocol Unit Tests:**
   - `engines/protocol`: added `tests::navigation_and_browser_integrations_wire_compatibility` covering `Command::Load` (`bypass_cache`), `Command::ClearHistory`, `Command::Policy` (`user_script`), `ContentEvent::Favicon`, `Status`, `Authentication`. 31 passed in `cargo test -p wega-protocol`.
2. **Engine History & Traversal:**
   - `engines/wega/tests/history.rs`: verified `clear_history_resets_session_history_entries`, traversal bounds, fragment navigation. Passed in `cargo test --test history`.
3. **Engine Page Integrations:**
   - `engines/wega/tests/pages.rs`: verified `favicon_detection_from_link_rel_and_default_fallback`, `user_script_runs_on_document_creation`. 13 passed in `cargo test --test pages`.
4. **Compilation & Python checks:**
   - `cargo check --lib`: passed.
   - `cargo check --bin wega-engine --manifest-path engines/Cargo.toml`: passed.
   - `python3 -m compileall -q scripts tests`: passed.
