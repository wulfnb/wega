# Native multi-account containers

Containers are built in and available by default from the **Containers** button
beside the engine badge. Personal, Work, Banking and Shopping are provided on
first use, including for existing profiles. **Manage containers…** adds identities
and changes their names and colors. The selected identity appears in the toolbar
and window title.

Choose a container to open or focus its window. **Open copy of this page in…**
opens the current URL there while leaving the source tab, drafts and sign-in
untouched. Only the URL is transferred; cookies and page state are never copied.
Every tab, popup, download and engine fallback in that window retains its profile.

## Isolation boundary

This implementation uses a separate native browser process and engine profile for
each container. Servo 0.7 exposes process-wide network/storage contexts and has no
per-WebView container selector. A colored tab alone would not isolate accounts.
Same-window container tabs require a future Servo storage-context API or a Servo
worker embedding. Until then, the UI deliberately calls these container windows.

An identity with ID `work` uses `<parent-profile>/containers/work/`:

- `engine/`: Servo cookies, storage, cache and media registry;
- `wega-storage/`: Wega broker cookies, origin storage and worker state;
- `browser.json`: its session, history, bookmarks, permissions and settings;
- `Downloads/` and `download-work/`: its completed and active downloads.

The default profile remains where it was. Its existing cookies, sessions and
settings are not migrated into any container. Renaming an identity preserves its
ID and storage path. Removing it from the menu retains its data on disk, and an
already open window can finish its work. Data deletion is not part of removal.
New custom identities receive random IDs and do not reuse removed storage.

`containers.json` lives in the parent profile. Its versioned registry is shared
across windows, edited under a short-lived exclusive lock and saved atomically.
Malformed registries are reported and never overwritten with defaults. Invalid
IDs and linked container directories are rejected before an engine starts;
failure never opens the default account instead.

An existing container window receives another launch through an authenticated
loopback endpoint. A private profile file holds a random capability, and the
endpoint accepts only focus or one validated web/file URL. Test and diagnostic
launches refuse to attach to an existing process. Closing the parent window does
not close container windows or discard their live pages.
Shutdown stops the listener and removes its endpoint before explicitly releasing
its launch lock, so a briefly duplicated or inherited handle cannot delay reopening.

## Scope

This is native account isolation inspired by Mozilla's
[Multi-Account Containers](https://addons.mozilla.org/en-US/firefox/addon/multi-account-containers/).
It does not install that Firefox extension. Firefox extension APIs, automatic
site-to-container routing, Firefox Sync and Mozilla VPN integration are not
provided. Containers share the host network address and operating-system user;
they are not OS sandboxes. Servo and Wega still use their own cookie/storage
implementations, so switching engines does not transfer a login between them.
In the Docker viewer, container windows share the same remote desktop. Their
profile-local downloads are not listed by the viewer's top-level download API.

CLI examples (the profile argument selects the parent profile):

```sh
cargo run -- --container work https://example.com/
target/debug/wega --profile-dir /tmp/wega-accounts --container personal
```

## Verification

```sh
cargo test --locked --no-default-features --lib
cargo build --locked --features test-harness
python3 scripts/build-wega-engine.py --debug
python3 tests/containers_smoke.py --binary target/debug/wega
python3 scripts/native-tests.py --suite smoke
```

The core contracts cover registry defaults, durable edits, stable identity on
rename, retained data on removal, corruption/locking errors, path alias rejection,
startup profile selection and real authenticated handoff. The native smoke suite
drives the container menu and manager through AccessKit and verifies that the
source live view survives create/edit/remove operations.

`tests/containers_smoke.py` checks the actual engine in each capture so automatic
fallback cannot earn Wega credit. It uses the same site in separate identities,
tests ordinary and HttpOnly cookies, localStorage persistence, fresh sessionStorage
after restart, an empty third identity, an unchanged parent profile, simultaneous
Servo accounts and URL handoff into an already running Work window. All accounts
and profiles are disposable. These checks do not certify real-site sign-in or
replace Windows/Linux packaged runtime validation.

### Local checkpoint — 27 October 2026

macOS ARM64: 92 core tests and 34 desktop unit tests pass. Core line coverage is
2118/2228 (95.06%), passing the unchanged 95% component gate; whole-project 100%
is still unmet. Strict core/desktop/harness Clippy, formatting and workflow lint
pass. Native smoke (including the container manager) and engine suites pass.
The engine-checked isolation/persistence suite passes for both Servo and Wega,
including concurrent Servo accounts and existing-window handoff.

The [validation record](validation/containers-2026-10-27.json) includes source and
binary fingerprints, artifact locations, the discarded fallback attempt and the
initial transient smoke failure. Remote CI and packaged Windows/Linux runtime
validation for these changes remain pending.
