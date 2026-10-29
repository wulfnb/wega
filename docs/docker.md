# Run Wega Browser in Docker

This image runs **our native Rust/Servo browser and its existing tabs, menus and
DevTools** on Linux. It does not install Firefox or use a LinuxServer image,
Selkies, XFCE, a video encoder, or a desktop control panel. A small project-owned
web page uses noVNC to forward display changes and keyboard/mouse input. Openbox
only manages the native browser and detached DevTools windows.

## Start

```sh
cp .env.docker.example .env
# Edit .env: set WEGA_WEB_USER, a unique WEGA_WEB_PASSWORD and WEGA_DOWNLOAD_DIR.
docker compose up -d --build
```

Open **https://localhost:7443** and accept the locally generated certificate.
The certificate and browser profile persist in the `wega-profile` named volume.

### TLS certificate

Without a certificate of your own, the container generates a self-signed ECDSA
P-256 certificate valid for 365 days with the names `localhost` and `127.0.0.1`.
Add names or addresses you use to reach it with `WEGA_TLS_SANS` in `.env`
(`BROWSER_TLS_SANS` for plain `docker run`), a comma list such as
`wega.home.arpa,192.168.1.20,fd00::5`. Entries may also be written as
`DNS:name` or `IP:address`. The certificate is regenerated when this list
changes and when fewer than 30 days of validity remain; the check runs at
startup and hourly, and nginx reloads the new certificate without a restart.
Browsers show a new warning after each regeneration.

To use your own certificate, either:

- mount the files and set `WEGA_TLS_CERT_FILE` and `WEGA_TLS_KEY_FILE` to
  their container paths (both required). They are copied at startup and
  re-checked hourly, so a certificate renewed on the host is picked up; or
- place `server.crt` and `server.key` into `/config/tls` in the profile volume
  and restart. A certificate there that the container did not generate is never
  replaced or renewed.

The first build compiles a web engine and needs considerable time and disk space.
The default builds an optimized release binary. Native Linux ARM64 and AMD64 are
selected by Docker; cross-architecture emulation makes this large build slower.
Rust is confined to the build stage. BuildKit can reuse Cargo registry and compilation caches after successful
builds. They are not copied into the runtime image, and interrupted builds or
cache eviction may require recompilation.

Optional settings can be placed in `.env` (see `.env.docker.example`):

```sh
WEGA_PORT=7443
WEGA_BIND_ADDRESS=127.0.0.1
WEGA_WEB_USER=wega
WEGA_WEB_PASSWORD='your-unique-password-here'
WEGA_DOWNLOAD_DIR=./downloads
WEGA_RESOLUTION=1280x800
WEGA_START_URL=https://example.com/
WEGA_BUILD_PROFILE=release
WEGA_BUILD_JOBS=2
```

Build options: `WEGA_CJK_FONTS=0` omits the Noto CJK fonts (about 90 MB;
Chinese, Japanese and Korean pages then render without suitable glyphs) and
`WEGA_LICENSE_NOTICES=0` skips installing cargo-about for faster local builds
(the image then carries only `LICENSE` and `NOTICE`). Normally
`/usr/share/doc/wega/` holds `LICENSE`, `NOTICE` and
`THIRD-PARTY-NOTICES.html`.

The image does not include `gstreamer1.0-libav`, which bundles GPL-licensed
FFmpeg components. Without it, H.264 video and AAC audio may not play;
VP8/VP9/AV1, Opus and Vorbis use other plugins. If you need those codecs and accept
the licensing implications, build a derived image that installs that package.

Use `WEGA_BUILD_JOBS=1` on memory-constrained builders. For a development build:

```sh
WEGA_BUILD_PROFILE=dev docker compose up -d --build
```

`WEGA_START_URL` sets the first-run page. Later starts restore the profile’s
saved tabs.

The viewer scales the fixed remote resolution to fit the client. The default is
`1280x800`; smaller values reduce display/rendering work. Allowed dimensions are
800–3840 pixels wide and 600–2160 pixels high. Change the environment setting and
recreate the container to change resolution. The viewer offers full screen,
reconnect, and a text clipboard dialog; the rest of the UI is our native browser.
The remote browser uses Linux Ctrl shortcuts. Your client browser may intercept
combinations such as Ctrl+L, Ctrl+T, or F12; use the remote browser’s address bar
and menu controls when that happens.

## Profile, logs and diagnostics

```sh
docker compose logs -f wega
docker compose exec wega supervisorctl -c /etc/wega-supervisord.conf status
docker stats --no-stream
# Stop while preserving the profile:
docker compose down
```

The browser runs as UID/GID 1000, with its profile at `/config/browser` and a
Downloads directory at `/config/Downloads`, mounted from `WEGA_DOWNLOAD_DIR` on
the host (default `./downloads`). Changing that variable selects a different host
folder; existing files stay in the previous folder. All display, viewer and application
workers, except the authentication gateway, run as that unprivileged user. The
gateway and its WebSocket bridge run as a separate `wega-auth` user (UID 1001),
so the browser cannot read the account key or credential record. The small
supervisor stays root to manage workers and forward their logs after initial
volume/certificate setup. No host Docker socket or privileged mode is used.
Closing the browser restarts it inside the same container; use Compose to stop
the session. If a program keeps failing to start, supervisord shuts down and
the container exits; Compose's `restart: unless-stopped` then starts a fresh
container (with plain `docker run`, add `--restart unless-stopped`).

The runtime has six supervised programs: display, minimal window manager, VNC,
authentication gateway with its WebSocket bridge, HTTPS server, and Wega
Browser, plus a small watchdog event listener. Health checks require all of them
to run and the HTTPS endpoint and authentication gateway to respond.

The only TCP listener is nginx's HTTPS port. Everything internal uses Unix
sockets, because any web page the browser opens can connect to TCP ports on the
container's loopback interface:

- x11vnc (browser UID) listens only on `/run/wega/vnc/rfb.sock`; its TCP
  ports are disabled (`-rfbport 0 -rfbportv6 -1`).
- websockify and the authentication gateway (`wega-auth` UID) listen on
  `/run/wega/auth/websocket.sock` and `/run/wega/auth/auth.sock`.
- The socket directories are accessible only to the `wega-ipc` group (the
  browser/nginx and gateway users); nginx is the only client of the gateway and
  the WebSocket bridge, after its login check.

Only HTTPS is published, on host loopback by default.

### Hardening and resources

`compose.yaml` runs the container with a read-only image filesystem (tmpfs for
`/tmp` and `/run`), `no-new-privileges`, Docker's default seccomp profile, and
all capabilities dropped except those the root entrypoint and supervisor need
(`CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `FSETID`, `SETUID`, `SETGID`, `KILL`).
Worker processes run as non-root users and hold no capabilities. Override the
limits in `.env`:

| Variable | Default | Effect |
| --- | --- | --- |
| `WEGA_MEM_LIMIT` | `2g` | Container memory limit |
| `WEGA_CPUS` | `2` | CPU limit (cores) |
| `WEGA_PIDS_LIMIT` | `512` | Maximum processes/threads |
| `WEGA_SHM_SIZE` | `512mb` | Shared memory size |
| `WEGA_RENDER_THREADS` | `2` | llvmpipe rasterizer threads (`LP_NUM_THREADS`) |
| `WEGA_VNC_WAIT` | `50` | x11vnc screen polling interval in ms (higher = less CPU, lower frame rate) |
| `WEGA_UPLOAD_RETENTION_DAYS` | `7` | Delete viewer uploads older than this (0 keeps them) |
| `WEGA_TLS_SANS` | empty | Extra certificate names/addresses |
| `WEGA_TRUSTED_PROXIES` | empty | Reverse proxies trusted for `X-Forwarded-For` |
| `WEGA_HTTP_PORT` | empty | Extra plain-HTTP port (1024–65535) for a TLS-terminating reverse proxy |

Authentication is required. On first start, set `WEGA_WEB_USER` and
`WEGA_WEB_PASSWORD` in `.env`; Compose passes them as `BROWSER_USER` and
`BROWSER_PASSWORD`. The username defaults to `servo`; there is no default password.
Startup refuses missing or placeholder credentials. Passwords contain 12–128 UTF-8
bytes without line breaks. Single-quote values in `.env` to preserve literal `$`,
`#` and spaces. Keep `.env` private (`chmod 600 .env`).

Sign in using the web login page. Open **Account** in the viewer toolbar to change
the username and password; enter the current password and confirm the new one.
The new account takes effect immediately and persists across restarts. Other
sessions and existing browser-control connections are revoked. **Sign out** signs
out all viewers of this shared account. Login sessions last up to 12 hours and use
Secure, HttpOnly, SameSite=Strict cookies. Password changes require a same-origin
request and a session CSRF token. Failed authentication attempts are rate-limited.

The account username and salted password hash are stored inside an encrypted
`/config/auth/account.enc` file. Passwords use scrypt with N=2^17, r=8, p=1 and a
random 16-byte salt, matching [OWASP's scrypt parameters](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#scrypt).
[PyCA Fernet](https://cryptography.io/en/stable/fernet/) provides authenticated
account encryption. Its random key is a separate `/config/auth/key` file; both
files are mode 0600 inside a mode 0700 directory. Protect profile backups: someone
who obtains both files can decrypt the username and attempt to crack the password
hash. The changed password itself is never stored reversibly.

After the first successful start, encrypt the initial `.env` username/password:

```sh
python3 scripts/web-credentials.py encrypt-env
docker compose up -d --no-build
```

This replaces both values with `enc:v1:…` ciphertext. The key stays in the profile
volume, outside `.env` and the image. This is obfuscation, not protection against
anyone who can read the profile volume: the key needed to decrypt the values is
stored in that same volume. It keeps the plain password out of `.env` files,
shell history and screenshots; delete the bootstrap values from `.env` once the
account exists if you prefer. Worker processes do not inherit plaintext
bootstrap credentials. To view your **initial** login locally, deliberately run
`python3 scripts/web-credentials.py show`. This cannot recover a password changed
in Account. Encrypted bootstrap values require the original profile's key; keep
that volume when recreating the container. `.env` values initialize a new account
only; they do not overwrite changes made in Account on later starts.

The same login protects the viewer, its assets, the WebSocket control endpoint,
download listings and files. Only the login page, its styles/script and the small
health response are public. All viewers share one browser session and profile;
these are not separate user accounts. Credentials and session cookies travel over
HTTPS. The username is visible in the signed-in Account dialog, not in storage.

Alternatively, mount a password file and set `BROWSER_PASSWORD_FILE` to its
container path. It takes precedence over the environment password on the first start. It may contain a plain bootstrap password or its `enc:v1:…` encrypted value. Example Compose override:

```yaml
services:
  wega:
    environment:
      BROWSER_PASSWORD_FILE: /run/secrets/browser-password
    secrets:
      - browser-password
secrets:
  browser-password:
    file: /path/to/browser-password.txt
```

The health URL remains unauthenticated and returns only `ok`. The control
WebSocket is reachable only through nginx, after the session check, and only
from the viewer’s own HTTPS origin; internally it is a Unix socket that pages in
the browser cannot open. Keep the default loopback publication for local use.

For access from other devices, prefer a private network such as Tailscale or
WireGuard and bind to that interface's address (`WEGA_BIND_ADDRESS`), adding the
name/address to `WEGA_TLS_SANS` or using your own certificate. Do not publish
the port directly to the internet. If you put it behind a reverse proxy, proxy
to `https://<container>:7443`, pass the original `Host` header (the gateway
compares it with `Origin`), support WebSocket upgrades on `/websockify`, and set
`WEGA_TRUSTED_PROXIES` to the proxy's address or network. nginx then takes
the client address from `X-Forwarded-For` (`real_ip_recursive on`), so login
rate limiting applies per client instead of to the proxy as a whole. Without
that setting, `X-Forwarded-For` is ignored.

If the proxy terminates TLS itself and cannot verify the generated certificate
(Traefik, Caddy or an ingress controller talking to the container by IP), set
`WEGA_HTTP_PORT` (for example `7080`) and proxy to `http://<container>:7080`
instead. Port 7443 keeps serving HTTPS. Only do this when the proxy reaches
the container over a trusted network: the same host, an encrypted overlay
(`docker network create --opt encrypted`) or a cluster network you already
trust, because credentials and the screen stream cross that hop unencrypted.
Never publish the HTTP port. A browser cannot sign in on it directly anyway:
the gateway accepts only an `https://` `Origin` matching the forwarded `Host`,
and the `__Host-` session cookie is `Secure`, so login and the WebSocket work
only through an HTTPS front end. The proxy must still pass the original `Host`
header, and `WEGA_TRUSTED_PROXIES` should name it.

Failed logins are limited to ten per minute per client address. Up to 4096
addresses are tracked; beyond that the oldest entries are evicted.

## What differs from the desktop app

Rendering uses Linux OpenGL/Mesa. There is no macOS vibrancy; the browser uses its
opaque theme variants. Mesa renders on the CPU with llvmpipe (`LP_NUM_THREADS`
from `WEGA_RENDER_THREADS`, default 2). An idle page costs little CPU; heavy
scrolling, animations and video can use the CPUs the container is allowed.
Raising `WEGA_RENDER_THREADS` and `WEGA_CPUS` speeds up heavy pages;
raising `WEGA_VNC_WAIT` lowers the cost of screen polling.

**GPU:** `compose.yaml` contains a commented `devices: /dev/dri` and
`group_add` example. Passing the device through does not accelerate this image
yet. The browser draws into Xvfb, a virtual X server without GPU support, so
Mesa still uses llvmpipe. GPU rendering would need an offscreen GPU path such as
VirtualGL's EGL back end, which is not wired up; it is future work.
noVNC sends changed screen regions rather than encoding a continuous video stream.
The VNC server polls Xvfb for changes because OpenGL updates can be missed by
XDamage tracking; this also allows a newly connected viewer to see an idle page.
This is intended for interactive browsing, not high-frame-rate video streaming.

Media support remains compiled into Servo, but this lightweight viewer does not
transport audio, microphones or webcams. It supports text clipboard transfer and
file uploads (see [Viewer uploads](#viewer-uploads-and-display-modes)).
Files can also be placed in the mounted download directory.
Completed files are also available through **Downloads** in the viewer toolbar.
Start transfers with **↓** in the native browser: enter an HTTP(S) URL, use
**Download link/image…** from a page's context menu, or click an HTTP(S)
`<a download>` link. A native confirmation window lets you choose a file name and
1–8 connections (default four). Network request context menus offer downloads too.

Multipart downloads require `Accept-Ranges: bytes`, a known size and a strong ETag
or Last-Modified validator. Every response range and length is checked; invalid
ranges trigger a fresh single-connection transfer. Pause/resume retains completed
2 MiB segments; unfinished segments restart. If the server changes its validator,
old segments are discarded. Completed files persist in `/config/Downloads` and
are published atomically without overwriting existing names. The web viewer lists
only completed files; its authenticated HTTPS file endpoint supports byte ranges.

The downloader reuses site cookies for the original host, verifies TLS certificates,
and supports HTTP(S) GET files. Browser-held HTTP authentication, POST exports,
blob/data URLs and cross-host cookie propagation are not supported. For ordinary
links without a `download` attribute, use the context-menu download action.
Active download history/partials do not resume across browser restarts; completed
files remain. At most three jobs run at once. Downloads follow normal HTTP redirects.
Pages and DevTools retain the same Servo engine limitations as the native app.
Actual memory/speed depend on the sites opened and software rendering; reducing
desktop services alone does not guarantee lower total RAM than Firefox.

## Image without Compose

```sh
docker build -t wega:local .
docker run -d --name wega --restart unless-stopped \
  -p 127.0.0.1:7443:7443 --shm-size=512m --memory 2g --cpus 2 --pids-limit 512 \
  --read-only --tmpfs /tmp:size=256m,mode=1777 --tmpfs /run:size=16m,mode=0755 \
  --security-opt no-new-privileges:true --cap-drop ALL \
  --cap-add CHOWN --cap-add DAC_OVERRIDE --cap-add FOWNER --cap-add FSETID \
  --cap-add SETUID --cap-add SETGID --cap-add KILL \
  -e BROWSER_USER=servo -e BROWSER_PASSWORD \
  -v wega-profile:/config -v /absolute/path/to/downloads:/config/Downloads \
  wega:local
```

Export `BROWSER_PASSWORD` in your shell before the `docker run` command. For direct
Docker use, credentials are named `BROWSER_USER`/`BROWSER_PASSWORD`; the `WEGA_*`
settings above are the Compose interface. Ensure a custom download folder is
writable by container UID/GID 1000 on Linux.

Run `python3 scripts/docker-auth-test.py` after startup to verify authentication,
the control connection, and the configured download mount using the initial login.
After changing the account, use the UI with the new credentials.
`scripts/docker-account-test.sh` tests credential changes, encryption, CSRF, live
connection revocation, persistence and rate limiting in an isolated temporary account.

The Docker shared-memory limit is capacity, not preallocated application RAM.
The build-time `BUILD_JOBS` argument controls compilation concurrency only.

The browser's bottom status bar shows live CPU and RAM in small text next to
the zoom controls. In this image it reads cgroup v2 totals for the whole container,
including display/viewer services, and excludes inactive file cache from RAM as
`docker stats` does. It samples every two seconds; CPU 100% represents one core.
Hover over the readout for scope and browser uptime. If container counters are
unavailable, it shows browser-process readings and identifies that scope instead.

Native test scripts below require a test image built with `BUILD_FEATURES=test-harness`; production images exclude these drivers. Prefer the [dedicated isolated quality environment](testing.md#docker-verification).

After startup, `scripts/docker-smoke.sh` checks runtime libraries, viewer assets
and the browser’s native interaction smoke test on an isolated X11 display.
The script copies the small test fixtures into the running container; production
images do not carry test pages or source code.

`scripts/docker-download-test.sh` runs a real download-link test with an HttpOnly
cookie, four parallel connections and byte-for-byte verification on an isolated
display/profile. It does not write into the user's completed-download directory.

Run `scripts/docker-toolbox-test.sh` for the full developer-tools integration
suite. It starts a local HTTP fixture and a separate display/profile inside the
container, checks real requests, debugging, storage, CSS editing and docking,
then stops its fixture server and display. Run these test scripts one at a time.
Linux ARM64 has passed both suites; AMD64 has not been runtime verified here.

`scripts/docker-idle-test.sh` checks an isolated two-tab session with live resource
counters enabled. It catches the software-renderer CPU loop caused by scheduling
foreground redraws for hidden offscreen tabs: after 8 s to settle, the test
browser must use under 10% of one core over 10 s. It also reports whole-container
CPU for the same window. Run it separately from the other tests.

On 20 October 2026, the ARM64 container on an Apple Silicon Docker host used
about 239 MiB and 1.3% CPU at idle with one Example Domain tab, a connected viewer,
1440×900 resolution and software Mesa rendering (before the default became
1280×800). This includes all six services,
not just the browser process. Both Linux integration suites, remote menu input,
static-frame delivery and session restoration after restart were verified.
These are measurements for that workload, not limits or a Firefox benchmark.


## Viewer uploads and display modes

Use **Upload** in the HTTPS viewer to select a file from your local computer.
After it finishes, copy the returned remote path into the website's file chooser
inside Wega. These are different filesystems: a local path alone cannot select
an item inside the container. Uploads require the active session and CSRF token,
are limited to 32 MiB each, and are stored under `/config/uploads` in the
persistent profile. That folder is owned by the gateway user and readable by the
browser group only; it is separate from the private credential directory.
Uploads older than `WEGA_UPLOAD_RETENTION_DAYS` (default 7; 0 keeps them) are
deleted at startup and hourly. The upload area is limited to 256 MiB and 100
files and is not available through the HTTP download listing. Uploads from
earlier images in `/config/auth/uploads` are moved there on startup.

The **View** selector offers Fit to window, Actual size / pan, and Resize remote
desktop. If remote resizing is unsupported by the VNC server, return to Fit or
use Actual size and scroll. **Full screen** remains available. Desktop file-open
and folder-reveal actions are not exposed as server-side launches in the viewer;
use **Downloads** to save completed downloads to your computer.

To test the current gateway/viewer/nginx files with an existing local image:

```sh
python3 scripts/docker-ux-test.py wega:local
```

This creates and removes its own container and temporary profile. CI runs the
same check against the image it has just built.


## Future Plans & Roadmap

### Multi-Architecture Container Images (`linux/amd64` & `linux/arm64`)
* **Current Status**: CI/CD publishes `linux/amd64` images to GHCR using GitHub-hosted x86_64 runners. Local builds natively compile and run on Apple Silicon / Linux ARM64 hosts (`docker compose up --build`).
* **Why not QEMU cross-compilation in CI?**: Compiling the full native Rust/Servo browser engine under QEMU emulation in GitHub Actions is extremely CPU-intensive and exceeds GitHub Actions' 6-hour job timeout.
* **Planned Implementation**:
  1. Leverage native GitHub-hosted Linux ARM64 runners (`ubuntu-24.04-arm64`) once available for the repository.
  2. Build native `linux/amd64` and `linux/arm64` container layers in parallel.
  3. Merge manifests via `docker buildx imagetools create` into a unified multi-arch image tag (`ghcr.io/wulfnb/wega:latest`) with cryptographically attested provenance.

### Code Signing & Distribution Integrity
* **macOS Gatekeeper Notarization**: Ad-hoc code signing is used currently. Automated notarization without Gatekeeper warnings requires an active Apple Developer Program membership (\$99/year) and automated submission via `xcrun notarytool`.
* **Windows Defender SmartScreen**: Eliminating SmartScreen untrusted publisher warnings requires an Authenticode EV (Extended Validation) code-signing certificate (from CAs such as DigiCert, Sectigo, or SSL.com, or Microsoft Azure Trusted Signing), which typically costs \$200–\$450/year and requires hardware security module (HSM) key storage. Free open-source alternatives like [SignPath Foundation](https://about.signpath.io/) may be evaluated once the project reaches public beta.
