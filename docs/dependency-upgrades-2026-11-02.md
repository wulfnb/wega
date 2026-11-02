# Dependency upgrades, 2 November 2026

These upgrades are part of `feature/standalone-wega-engine` / PR #40.
The browser, engine, standalone spike and error collector keep separate lockfiles;
all four are refreshed together. Older transitive majors remain where required
by Servo or Deno extensions.

| Dependabot PR | Dependency | Result |
| --- | --- | --- |
| #42 | accesskit | 0.25.2 |
| #42 | winresource | 0.1.34 |
| #42 | pkcs1 | Retained at 0.8.0-rc.4; see below |
| #38 | sha1, engine and network | 0.11.0 |
| #37 | base64, engine and network | 0.23.1 |
| #36 | actix-web, error collector | 4.18.1 |
| #35 | getrandom, network and platform | 0.4.3 |
| #34 | syn, vendored deno_ops | 3.0.9 |
| #33 | bit-vec, vendored deno_core | 0.10.1 |
| #32 | rusqlite, error collector | 0.41.1 |
| #31 | sha2, engine and network | 0.11.1 |
| #30 | strum_macros, vendored deno_ops | 0.28.0 |
| #29 | num-bigint, vendored deno_core and serde_v8 | 0.5.1 |
| #28 | windows-sys, vendored deno_core | 0.61.2 |

## Compatibility changes

- Pair bit-vec with bit-set 0.11.1 so Deno's arena passes the same BitVec type
  to `BitSet::from_bit_vec`.
- Pair strum_macros with strum 0.28 and Syn 3 with prettyplease 0.3. Adapt Deno's
  generic lifetime construction, function modifiers and pointer mutability to
  Syn 3's AST. The engine and spike compile their real op macros against this.
- SHA-2 0.11's output array does not implement `LowerHex`. Explicitly encode
  digest bytes as two lowercase hex digits for origin directories, captured
  frames and pinned WPT checksums. The existing-storage regression now opens
  a database at a fixed pre-upgrade origin hash so an encoding change cannot
  silently create an empty replacement store.
- Record all vendored changes as exact patches in `engines/vendor/sources.json`.
  Original crate versions and archive checksums remain pinned and verified.

## PKCS#1 constraint

Servo 0.7.0's `servo-script` pins `rsa = "=0.10.0-rc.18"`. Compiling that RSA
crate against pkcs1 rc.5 fails in `rsa/src/encoding.rs`: `RsaPrivateKey<'_>` and
`RsaPublicKey<'_>` have become generic types without lifetime parameters.
RSA rc.19 supports the new API, but Cargo rejects it against Servo's exact pin.

Keep pkcs1 rc.4 until Servo's RSA dependency can be upgraded. Dependabot ignores
only rc.5 in the browser workspace, with the reason recorded beside the rule;
other dependency and security updates remain enabled.

## Local verification

- All engine workspace targets compile, including tests and binaries.
- The standalone V8 spike compiles with the shared vendor patches.
- Deno core type-checks for `x86_64-pc-windows-msvc` with `DOCS_RS=1`; this
  checks the windows-sys migration without linking or running a Windows binary.
- All eleven vendored archives match their recorded patches and checksums.
- Network, platform and protocol suites pass: 78 tests; one native sandbox
  probe is intentionally ignored without its configured helper.
- The collector's five regression tests and strict Clippy checks pass.
- The engine's 17 unit tests pass.
- Browser core strict Clippy, formatting checks and Actionlint pass.

Hosted CI remains the authority for full native Windows/macOS/Linux validation
and desktop packaging; local cross-compilation alone does not establish runtime
behavior on another operating system.
