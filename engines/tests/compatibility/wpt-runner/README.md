# wega-wpt-runner

Runs upstream [web-platform-tests](https://github.com/web-platform-tests/wpt) against the Wega
engine in-process (`wega_engine::Engine` / `View`) and gates CSS work on the results. Modeled on
`blitz/wpt/runner` and pinned to the same WPT commit (`WPT_COMMIT`).

## Setup (once)

A sparse, shallow checkout of the CSS suites is about 140 MB:

```sh
git init ~/my/wpt && cd ~/my/wpt
git remote add origin https://github.com/web-platform-tests/wpt.git
git sparse-checkout init --no-cone
printf '%s\n' /resources/ /fonts/ /images/ /common/ /css/support/ /css/reference/ \
  '/css/*/support/' '/css/*/resources/' '/css/CSS2/*/support/' /css/selectors/ /css/css-cascade/ \
  /css/css-color/ /css/css-values/ /css/mediaqueries/ /css/css-conditional/ /css/css-variables/ \
  /css/css-logical/ /css/css-flexbox/ /css/css-grid/ /css/CSS2/floats/ /css/CSS2/floats-clear/ \
  /css/CSS2/tables/ /css/css-backgrounds/ /css/css-display/ /css/css-sizing/ /css/css-position/ \
  > .git/info/sparse-checkout
git fetch --depth 1 --filter=blob:none origin "$(cat <engines>/tests/compatibility/wpt-runner/WPT_COMMIT)"
git checkout FETCH_HEAD
```

Set `WPT_DIR` if the checkout is somewhere other than `~/my/wpt`.

## Running

```sh
cd engines
cargo build -p wega-wpt-runner
target/debug/wpt-runner                      # default suites, compared with baseline.json
target/debug/wpt-runner css/css-grid         # one suite, or a single test file
target/debug/wpt-runner --save-failures css/css-color   # PNGs of failing reftests in target/wpt/failures
target/debug/wpt-runner --update-baseline    # after an improvement: record the new passes
```

The exit code is 1 when a test listed as passing in `baseline.json` no longer passes. Results for
each test (status, subtest counts, the first failing assertions) are written to
`target/wpt/results.json`.

## How it works

- `discover.rs` classifies files the way WPT's manifest does: reftests use `<link rel=match|mismatch>`
  and testharness tests load `/resources/testharness.js`. References, support files, `.sub.`,
  `.https.`, `.any.`/`.window.`/`.worker.` and manual tests are skipped.
- `server.rs` serves the checkout on loopback, since tests use root-relative paths. It replaces
  `testharnessreport.js` with a collector the runner polls.
- `execute.rs` runs testharness tests (a file passes when every subtest passes) and reftests (an
  800×600 screenshot compared with each reference, honouring `<meta name=fuzzy>`).
- `shard.rs` runs in child processes, one per `--jobs`. A crash or a hang (45 s) is recorded as
  CRASH or TIMEOUT for the test that caused it, and the shard restarts after it.
- `report.rs` builds per-suite tallies and the baseline comparison.

Not yet supported: wptserve substitutions and `.headers`, reference chains, `print` reftests, and
`testdriver.js` interactions.
