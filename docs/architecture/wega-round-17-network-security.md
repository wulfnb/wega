# WEGA ROUND 17 — NETWORKING AND SECURITY ARCHITECTURE REPORT

## Status: COMPLETE

### 1. Executive Summary
Round 17 hardens the Wega network and browser security subsystem across transport, broker boundaries, cookie semantics, origin isolation, Content Security Policy, and HTTP protocol edge cases without regressing any existing security limits (32 max frames, 8 nesting depth, 128 layout depth).

### 2. Implemented Functionality & Architectural Changes

#### 2.1 HTTP & Navigation Hardening (`wega-network`)
- **Redirect Method Rewriting & Body Drop**:
  - `303 See Other`: Rewrites method to `GET` (unless `HEAD`), clears body, strips `Content-Type` and `Content-Length`.
  - `301 Moved Permanently` and `302 Found`: Rewrites `POST` to `GET`, clears body, strips `Content-Type` and `Content-Length`.
  - `307 Temporary Redirect` and `308 Permanent Redirect`: Strictly preserves HTTP method and request body across redirect hops.
- **Cross-Origin Sensitive Header Stripping**:
  - On cross-origin redirects (`next.origin() != url.origin()`), `Authorization`, `Proxy-Authorization`, and `Cookie` headers are stripped to prevent credential leakage.
  - Strips non-CORS `Origin` header on cross-origin redirects.
- **Decompression-Bomb & Body Protection**:
  - Validates `Content-Length` and checks decompression expansion ratio for compressed payloads (`Content-Encoding` != `identity`).
  - Guards against high-ratio decompression bombs (>100x expansion ratio over 512 KiB).
  - Enforces hard response size limit (`request.limit`), terminating streams immediately with `"Response exceeds its size limit"`.
- **MIME Types and `X-Content-Type-Options: nosniff`**:
  - Enforced in `cors_after` and broker responses.
  - Scripts (`ResourceDestination::Script` and `Module`): Must match an authorized JavaScript MIME type (`application/javascript`, `text/javascript`, `application/ecmascript`, etc.). Rejects invalid types like `text/plain` or `text/html`.
  - Stylesheets (`ResourceDestination::Style`): Must be `text/css`. Rejects non-CSS MIME types like `text/html`.
- **Referrer-Policy Calculation (`compute_referrer`)**:
  - Full W3C Referrer Policy implementation supporting: `no-referrer`, `no-referrer-when-downgrade`, `same-origin`, `origin`, `strict-origin`, `origin-when-cross-origin`, `strict-origin-when-cross-origin`, and `unsafe-url`.
  - Automatically strips userinfo (username/password) and fragments.
  - Protects against HTTPS to HTTP protocol downgrade leaks.
- **Active Mixed-Content Blocking**:
  - Enforced in `Policy::permits_request`.
  - Blocks active subresource requests (`Script`, `Module`, `Style`, `Iframe`, `Fetch`, `Xhr`, `WebSocket`) directed to insecure `http://` targets when the initiator or origin is `https://`.
- **User Agent Modernization**:
  - Updated `LocalClient` user agent string from `Wega/0.1 (Blitz/V8)` to `Wega/0.1 (Native/V8)`.

#### 2.2 Cookie Semantics (`wega-cookies`)
- Verified RFC 6265bis cookie store with public suffix list matching (`publicsuffix` integration).
- Cookie prefix validation:
  - `__Secure-`: Rejects cookies unless set over secure HTTPS transport with `Secure` attribute present.
  - `__Host-`: Rejects cookies unless set over secure HTTPS transport with `Secure` attribute, `Path=/`, and NO `Domain` attribute (host-only).
- `SameSite` enforcement:
  - `SameSite=None` strictly requires `Secure`.
  - Cross-site subresource responses cannot store non-`SameSite=None` cookies.
  - Script access respects `HttpOnly`.

#### 2.3 Content Security Policy & Reporting (`wega-security`)
- Added `report-uri` directive parsing.
- Added `Csp::build_violation_report()` generating specification-compliant `csp-report` JSON payloads for blocked directives.
- Preserved frame-ancestors, nonces, sha256/sha384/sha512 hashes, `strict-dynamic`, and unsafe-inline restrictions.

#### 2.4 Process Isolation Assessment (17.3)
- **Current Model**:
  - Two-tier privilege separation: Browser host process holds network sockets, disk cache, SQLite storage, and private key/cookie databases. Content worker runs in a separate child process.
  - Communication is strictly over private binary IPC (`wega-protocol`), never direct raw network sockets or direct filesystem access outside the broker grants.
  - Iframes and cross-origin documents inside a worker execute with opaque principal facades and frame boundaries (exact 32-frame limit, 8-frame nesting limit).
- **Architectural Isolation Boundary**:
  - Out-of-Process Iframes (OOPIF) — where each cross-origin frame runs in a distinct OS process — requires multi-process renderer orchestration in the host UI process.
  - Current status: Site-isolated broker with process-isolated renderers, but within-worker frame isolation relies on capability tokens and principal boundaries. We do not falsely claim full OOPIF site isolation.

---

### 3. Verification & Test Evidence

#### Test Commands & Results
```bash
cargo test --manifest-path engines/Cargo.toml -p wega-network --offline
# Result: 15 passed in lib.rs, 3 passed in broker.rs, 2 passed in inherited.rs, 7 passed in security.rs, 5 passed in round17_acceptance.rs. Total: 32 passed, 0 failed.

cargo test --manifest-path engines/Cargo.toml -p wega-cookies --offline
# Result: 5 passed (including cookie_prefixes_secure_and_host_enforcement), 0 failed.

cargo test --manifest-path engines/Cargo.toml -p wega-security --offline
# Result: 9 passed (including csp_violation_reporting_and_report_uri), 0 failed.

cargo test --manifest-path engines/Cargo.toml -p wega-engine --test iframes --offline
# Result: 17 passed (all frame limit and sandbox invariants verified), 0 failed.

cargo test --manifest-path engines/Cargo.toml -p wega-engine --test inherited_broker --offline
# Result: 1 passed (signed sandbox verified), 1 ignored (requires helper daemon), 0 failed.
```

#### New Regression Fixtures
- `engines/network/tests/round17_acceptance.rs`:
  - `test_referrer_policy_computations`: Comprehensive matrix of 7 referrer policies across same-origin, cross-origin, and downgrade.
  - `test_mixed_content_blocking`: Verifies active destinations blocked from HTTPS contexts while HTTP contexts remain permitted.
  - `test_redirect_method_rewriting_and_credential_stripping`: Multi-server fixture testing 303 POST->GET method rewrite and cross-origin authorization stripping.
  - `test_nosniff_mime_enforcement`: Tests nosniff rejection of text/plain scripts and text/html styles against accepted MIME types.
  - `test_oversized_response_body_rejected`: Validates request limit cutoff.
- `engines/services/cookies/src/lib.rs`:
  - `cookie_prefixes_secure_and_host_enforcement`: Tests `__Secure-` and `__Host-` specification compliance.
- `engines/services/security/src/csp.rs`:
  - `csp_violation_reporting_and_report_uri`: Tests report generation and structure.

---

### 4. Known Limitations & Remaining Work
- Full Out-of-Process Iframes (OOPIF): Renderer threads inside a worker share address space; separation of different origin iframes into separate OS processes remains scheduled for a future dedicated multi-process campaign.
- Permissions Policy: Core device permissions are registered in `wega-permissions`; delegating permission directives across nested iframe boundaries is ready for Round 18 input integration.
