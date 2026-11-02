# CI and release fixes — 2 November 2026

Base: main `1e4d29807cf6c018e65696f42e78c804404d052d`.
Branch: `fix/ci-engine-integration`.

## Behavior fixed

- The desktop supervisor explicitly rejects the four reserved frame-coordination
  events. This restores exhaustive protocol handling and the desktop build without
  admitting renderer-assigned frames or trusting renderer-supplied origins. Real
  frame coordination remains unfinished as described in
  `step-14-15-browsing-contexts-and-components.md`.
- Terminal inherited-broker rejections have a distinct reply. The client consumes
  it and closes that channel. Windows waits at most two seconds for that close
  before disconnecting, so an immediate named-pipe disconnect cannot discard the
  rejection. Ordinary request errors preserve reusable channels. Regressions cover
  retry prevention, ordinary-error reuse and a delayed Windows reader. The teardown
  follows [Microsoft's documented pipe behavior](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-disconnectnamedpipe).
- Successful preview publishing now moves the actual `test-preview` Git ref to
  the release commit and removes only the four known obsolete `Lotse-*` assets.
  Version, custom and per-commit tags are preserved. Nothing changes on GitHub
  until the updated publishing workflow runs.

## CI changes

- Run product engine tests, lints and native confinement once per OS through a
  reusable workflow required by the CI completion gate. CD already requires that
  CI workflow. Remove the branch-specific push trigger and duplicate product
  engine regressions from the V8 spike and macOS desktop jobs. The separate V8
  spike smoke tests and prebuilt mirror remain.
- This reduces complete product engine suite invocations from seven to three for
  the former feature-branch push/PR combination, and from four to three on main.
  These are workflow counts, not measured wall-clock speedups.
- Compare the tested PR merge with its base using checkout depth two. Markdown-only
  PRs skip product builds; spike-only PRs retain their separate spike checks.
  Vendored product changes and every merge/release still run the complete suite.
- Explicitly map the independent engine workspace and its build directory in
  browser/package caches. Install the pinned coverage tool from a prebuilt binary.
- Remove uninstrumented Python runs duplicated by Linux/macOS coverage runs and
  the second macOS native suite already executed by desktop coverage. Preserve
  instrumented tests and existing coverage thresholds.
- Build the Windows browser before its independent worker so browser compilation
  errors are reported before the separate worker build.
- Include the error collector in the completion gate. Audit the collector and
  spike lockfiles, and monitor all independent workspaces with Dependabot. Docker
  scans now fail on fixable high/critical findings instead of always succeeding.

## Validation

Local macOS arm64 checks passed:

- Production and test-harness desktop Clippy across all targets, with warnings denied.
- Engine workspace Clippy across all targets, with warnings denied.
- 106 core tests; 50 desktop unit tests; 258 engine workspace tests; 28 broker-backed
  engine contract tests.
- Both normally ignored native acceptance tests, run explicitly with signed helpers:
  ambient-authority denial/explicit grants, and required worker JIT/storage/cookies/frames.
- 81 Python tests. The new preview helper and existing authentication gateway each
  achieved 100% line and branch coverage in this run.
- Actionlint, both Cargo formatting checks, Python syntax and `git diff --check`.
- Pinned upstream archive/patch verification for all eleven vendored engine crates.
- Scope execution against a real depth-two Git clone containing a PR merge and
  independent base changes; documentation, vendor and push cases passed.
- Completion-gate execution accepted permitted skips and rejected failed scope,
  failed jobs, cancellations and missing checks on a full run.

Logs are under this worktree's ignored `target/` directory. These local results do
not establish hosted cross-platform execution, packaging, Docker scan results or
CI wall-clock speedups; those require the workflows to validate the published revision.

## Remaining repository and dependency work

The GitHub CLI account used for inspection has pull access, without push or admin
permission. Git publication uses the separately configured personal SSH identity.
This patch does not change repository branch-protection settings.
Main should require the always-running `CI Completion Gate` before merging;
individual conditionally skipped jobs should not be made unconditional requirements.

The engine's pinned Deno dependency graph reports seven advisory findings:
`crypto-bigint 0.5.5`, two Hickory findings on `0.25.2`, and four rustls findings on
`0.23.40`, plus the `rsa` Marvin finding that has no fixed release. Deno TLS pins
that rustls version exactly, so a lockfile-only rustls update is rejected. Updating
Deno TLS/net/crypto coherently is separate dependency work.

A non-blocking audit hid every *new* engine advisory along with the known ones, so
those eight IDs are now listed explicitly in `engines/.cargo/audit.toml`, each with
its fix version and a review date (2027-02-01). The engine audit is blocking again:
anything not on that list fails CI. Remove each ID when the Deno pin moves.
Existing third-party warnings are not a claim of zero security risk.

## Keeping up with advisories

- `security.yml` runs daily, not only on commits: CodeQL (JavaScript, Python and
  Rust, vendored sources excluded), `cargo audit` over all four lockfiles, and a
  Trivy rescan of the published `:latest` image with results in code scanning.
- Dependabot groups minor/patch updates per workspace into one weekly PR;
  `dependabot-automerge.yml` squash-merges those groups once CI has passed for the
  exact head commit. Majors and the GitHub Actions group stay manual.
- Dependabot *alerts* and *security updates* are repository settings, not files.
  They must stay enabled (Settings → Code security), otherwise advisories against
  already-locked crates open no PR at all.

## Release platforms and pull-request scope

- CD packages macOS for Apple Silicon (`macos-15`) and Intel (`macos-15-intel`).
  The pinned V8 prebuilts already include `x86_64-apple-darwin`; the Intel build is
  checked to be a single-architecture `x86_64` binary. `macos-15-intel` is GitHub's
  last Intel image; when it is retired, drop that matrix row or cross-compile.
- The Windows installer is versioned from `Cargo.toml` (previews append the commit),
  closes a running Wega, deletes the previous binaries before copying, and
  `windows-installer-check.ps1` verifies an upgrade on top of an existing install.
- Pull requests run only the CI jobs their paths can affect (`scripts/ci-scope.py`,
  tested by `tests/test_ci_scope.py`). Engine-only changes run engine validation and
  both desktop suites; unknown or shared paths run everything; pushes, merges and CD
  always run the complete suite. The completion gate accepts a skipped job only when
  the scope did not select it.
