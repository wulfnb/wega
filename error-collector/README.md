# Wega error collector

Small Rust/Actix service following Flow's route/controller/validator layout. SQLite
replaces Flow's Postgres dependency so this service needs one container and a data
directory. The runtime image is `scratch`, with a statically linked musl executable
and no shell. Build from the Wega repository root:

```sh
docker build -f error-collector/Dockerfile -t wega-error-collector:local .
cargo test --locked --manifest-path error-collector/Cargo.toml
```

The container smoke test uses Python's `cryptography` package to independently
sign real HTTP requests. Run it in a virtualenv where that package is installed:

```sh
python error-collector/tests/container_smoke.py --image wega-error-collector:local
```

With a native browser build and a working display, the complete browser-to-server
test creates isolated profiles for both engines, queues errors without a URL,
enrolls their public keys, enables the local endpoint, and verifies delivery:

```sh
cargo build --locked --features test-harness
python3 scripts/build-wega-engine.py --debug
python3 tests/error_reporting_smoke.py --collector-image wega-error-collector:local
```

`GET /healthz` checks database availability. `POST /v1/errors` accepts signed JSON
reports. The container runs as UID/GID 65532 on port 7090. Its `/data` directory must
be writable by that user. `DATA_DIR`, `BIND_ADDRESS` (an IP socket address), and
`RETENTION_DAYS` (default 30, clamped to 1–365) configure it. A built-in
`healthcheck` command probes the actual HTTP server without a shell or curl.

## Approving browser installations

On startup Wega creates an Ed25519 keypair in the current profile's
`error-reporting/identity.json` (private directory 0700 and file 0600 on Unix).
`error-reporting/enrollment.json` contains only the public client ID and public key.
Copy those two public values to the server and explicitly enroll them:

```sh
docker exec COLLECTOR /wega-error-collector enroll CLIENT_ID PUBLIC_KEY
docker exec COLLECTOR /wega-error-collector revoke CLIENT_ID
docker exec COLLECTOR /wega-error-collector export 1000 > errors.jsonl
```

There is no public enrollment, export, or administration endpoint. Enrollment is
idempotent; re-enrolling replaces the key and re-enables that client. Revocation
persists across restarts. Keep the private identity out of images, source control,
support bundles, and enrollment requests.

Approved keys distinguish enrolled installations from unsigned or unknown callers.
This is installation authentication, not proof of an untampered browser executable.
Someone with a stolen private key can impersonate that installation until it is
revoked. A shared secret embedded in every browser would not provide this isolation.

## Enabling delivery later

The current home-server deployment is available at
`https://errors.noufal.de/v1/errors`. The installed Mac browser's default profile
is enrolled and configured. Other profiles must enroll their own public keys and
configure the endpoint separately.

To initialize a profile identity without opening the browser, use the helper
below. It prints only public enrollment information. Adding `--queue-probe`
queues one explicitly labeled synthetic connectivity report for the next browser
launch:

```sh
cargo run --locked --no-default-features --example error-reporting-identity -- PROFILE_DIRECTORY
```

No URL is hard-coded and no remote request is made before configuration. Errors
are queued locally even before the collector exists. Once an HTTPS URL and client
enrollment are ready, create `error-reporting.json` in the browser's profile:

```json
{"endpoint":"https://YOUR-HOST/v1/errors"}
```

Alternatively set `WEGA_ERROR_REPORT_URL`. Restart the browser to apply it. HTTP is
allowed only for loopback development. Redirects, URL credentials, query strings,
and fragments are rejected. The endpoint path must be `/v1/errors`.

Each report includes a random ID, occurrence time, browser version/platform,
source, category, severity, message/stack, and a URL/line/column when the engine
provides them. Worker HTML parser errors (including fragments), stylesheet/inline
CSS/CSSOM parse errors, JavaScript exceptions and console warnings/errors, Servo
parser/console diagnostics, browser panics and worker failures use this pipeline.
Parse errors are warnings because HTML/CSS engines commonly recover from them.
The persistent local log also keeps raw engine stderr.

Errors emitted during process shutdown can remain in the outbox after the last
HTTP attempt. The next launch retries them with their original report IDs.

Messages are limited to 16,384 characters (marked `truncated`), and URLs to 8,192.
Reports can contain page URLs and application-supplied error text. Do not treat
their contents as trusted HTML or shell input; restrict access to exports and the
data directory. Neither form values nor cookies are collected deliberately, but
page scripts can include sensitive values in their own error messages. Exceptions
that an OS kill, process abort, or power failure prevents the engine from emitting
cannot be reported by this path.

## Durability and request verification

Every emitted report is atomically written to `<profile>/logs/error-outbox/` before
delivery. Successful responses remove it. Offline, unauthorized, rate-limited,
and server failures retain it, retrying after 1 second up to 5 minutes. Repeated
errors have individual IDs; a UI status queue limit does not discard them. The
outbox has no automatic size purge, so an unavailable endpoint or error storm can
grow disk use. Corrupt files are preserved with a `.corrupt` extension without
blocking later reports. Rotated local logs are independent of this outbox.

Requests carry `X-Wega-Client`, `X-Wega-Timestamp` (Unix seconds), `X-Wega-Nonce`
(16 random bytes, hex), and `X-Wega-Signature` (Ed25519 signature, hex). The signed
UTF-8 payload is exactly, with no trailing newline:

```text
wega-errors-v1\nPOST\n/v1/errors\nCLIENT\nTIMESTAMP\nNONCE\nSHA256_OF_EXACT_BODY_HEX
```

The server requires an approved key, a timestamp within 5 minutes, a valid
signature over the exact body, and a previously unseen nonce. Nonces live in
SQLite for 10 minutes, including across service restarts. Retries use the same
report ID/body and a fresh nonce/signature: identical reports return 200 without
duplicate storage; first acceptance returns 202. Replays or a reused report ID
with a different body return 409. Invalid/missing/stale headers return 401;
unknown/revoked clients or bad signatures return 403. Rejected authentication,
replays, and ID conflicts have JSON audit reasons on stderr. The server limits
bodies to 256 KiB and each authenticated client to 6,000 new reports/minute (429).
Retention is based on server receipt time; expired rows are pruned on ingestion.

SQLite data is local persistent state: keep one replica, use stop-first updates,
and back up the database through SQLite's backup API or stop the service before
copying the database together with any WAL files. The home-server deployment
example is `home-server/wega-errors-compose.yml`; its public route defaults off.
