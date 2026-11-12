# Wave 1 baseline (recorded before any extraction)

| Item | Value |
|---|---|
| Git commit | `c147bd3c41e49958d1ffd16384b82c3099cdd59a` (after Step 0 documentation fixes) |
| Branch | `feature/browser-cache-accessibility` |
| Toolchain | `rustc 1.98.0 (88d9e12ae 2026-08-18)`, `cargo 1.98.0 (797e8a9bc 2026-08-05)` (pinned by `rust-toolchain.toml`) |
| Machine | Apple M4, macOS 27.2 (Darwin 27.2.0) |
| `engines/Cargo.lock` blob | `51d9149284e22cddf01cc92012ec7fb426c69609` |
| Engines workspace members | `wega-engine`, `wega-network`, `wega-platform`, `wega-protocol` |
| Root workspace members | `wega` (path deps: `wega-protocol`, `wega-platform`, `wega-network`) |

Dependency graph and architecture report: [`dependency-graph-baseline.md`](./dependency-graph-baseline.md), [`architecture-check-baseline.md`](./architecture-check-baseline.md).

## Commands

```sh
E="--locked --manifest-path engines/Cargo.toml --target-dir target/wega-engine"
cargo test   $E --workspace --all-targets --no-fail-fast -- --test-threads=1
cargo clippy $E --workspace --all-targets -- -D warnings
cargo fmt --manifest-path engines/Cargo.toml --all --check
```

Note: `cargo fmt --manifest-path engines/Cargo.toml --check` (without `--all`) fails with "Failed to find targets" because the manifest is a workspace root; `--all` is required. The plan's command is corrected accordingly.

## Results

| Check | Result |
|---|---|
| `cargo test` (43 test binaries) | **359 passed, 1 failed, 2 ignored**; wall 375 s (warm `target/wega-engine` cache, incremental recompile of `wega-network` and `wega-engine`), max RSS 1.47 GB, peak footprint 177 MB |
| `cargo clippy ... -D warnings` | passes (exit 0); 12.7 s, max RSS 530 MB (cached) |
| `cargo fmt --all --check` | passes |

### Pre-existing failure (not a regression)

`wega-engine` integration test `cache_api::cache_add_all_fetches_real_responses_atomically_and_reads_without_the_server` (`engines/wega/tests/cache_api.rs:253`) **fails before any change**. It was run three times and failed each time, with differing observed values (for example `failures: [TypeError, TypeError]` instead of `[TypeError, InvalidStateError]`, or an overall `TypeError: Cache.addAll received an unsuccessful response`), so the failure is real but its exact output varies. It directly exercises the Cache API that Step 4 (`wega-storage`) moves, so after Step 4 the expectation is **the same single test failing and nothing else**.

### Pre-existing warnings (not suppressed)

- 7+ `tokio` deprecation warnings (`AsyncFd::new`, `TcpStream::set_linger`) in engine code.
- 8 warnings from the vendored `deno_core` build.
