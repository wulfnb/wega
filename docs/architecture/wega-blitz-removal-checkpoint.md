# Blitz removal recovery checkpoint

- Starting/last verified commit: `8523150487a953394920e69459ad7f374e57297c`.
- Branch: `feature/dom-rounds-9-12`; initial worktree clean.
- Phase 0: inventory recorded before production edits; graph capture in progress.
- Active commands: graph/metadata capture; brokered public-page baseline capture
  (`WEGA_PUBLIC_COMPAT=1 WEGA_TEST_BROKER=1 WEGA_RENDER_BACKEND=native`).
- Evidence directory: `docs/architecture/baseline/blitz-removal-2026-11-17`;
  public baseline `/private/tmp/wega-blitz-before` and matching `.log`.
- Tests in this continuation: not yet complete. Prior totals are historical only.
- Remaining: diagnose public-page geometry; regression fixtures/fixes; design and
  implement real owner migration; runtime independence/security/Chrome/WPT;
  release measurements; safe removal; final full checks and reports.
- Do not push. Do not edit DevTools D4/inspector. Never delete Blitz before
  native-only runtime gates. Preserve budgets, CSP and exact 32-frame assertion.
