#!/usr/bin/env bash
# Write the license files every Wega download ships:
#   OUT_DIR/THIRD-PARTY-NOTICES.html  (cargo-about, from about.toml + about.hbs)
#   OUT_DIR/LICENSE                   (MPL-2.0)
#   OUT_DIR/NOTICE                    (with the exact source commit appended)
# Extra arguments go to `cargo about generate`, e.g. `--features media` or
# `--target x86_64-pc-windows-msvc`. Requires network access for crate sources.
# The source commit comes from WEGA_SOURCE_COMMIT, GITHUB_SHA or git, in order.
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
if [[ $# -lt 1 || "$1" == -* ]]; then
  echo 'Usage: scripts/third-party-notices.sh OUT_DIR [cargo-about generate options...]' >&2
  exit 1
fi
out_dir="$1"
shift
if ! cargo about --version >/dev/null 2>&1; then
  echo 'cargo-about is required: cargo install cargo-about --version 0.9.2 --features cli --locked' >&2
  exit 1
fi
mkdir -p "$out_dir"
notices="$out_dir/THIRD-PARTY-NOTICES.html"
# --fail turns an unidentified or unaccepted license into a build failure.
cargo about generate --locked --fail \
  --config "$project_dir/about.toml" \
  --manifest-path "$project_dir/Cargo.toml" \
  "$@" --output-file "$notices" "$project_dir/about.hbs"
if [[ ! -s "$notices" ]]; then
  echo "cargo-about produced no $notices" >&2
  exit 1
fi
if [[ -f "$project_dir/engines/wega/Cargo.toml" ]]; then
  mkdir -p "$out_dir/WEGA-ENGINE-NATIVE-LICENSES"
  cp -R "$project_dir/engines/vendor/blitz/licenses/." "$out_dir/WEGA-ENGINE-NATIVE-LICENSES/"
  # The worker has a separate dependency graph. Root features do not apply to it.
  cargo about generate --locked --fail \
    --config "$project_dir/about.toml" \
    --manifest-path "$project_dir/engines/wega/Cargo.toml" \
    --output-file "$out_dir/WEGA-ENGINE-THIRD-PARTY-NOTICES.html" "$project_dir/about.hbs"
  if [[ ! -s "$out_dir/WEGA-ENGINE-THIRD-PARTY-NOTICES.html" ]]; then
    echo 'Missing Wega engine license notices.' >&2
    exit 1
  fi
fi
cp "$project_dir/LICENSE" "$out_dir/LICENSE"
commit="${WEGA_SOURCE_COMMIT:-${GITHUB_SHA:-}}"
if [[ -z "$commit" ]] && command -v git >/dev/null 2>&1; then
  commit="$(git -C "$project_dir" rev-parse HEAD 2>/dev/null || true)"
fi
{
  cat "$project_dir/NOTICE"
  printf '\nThis distribution was built from source commit: %s\n' "${commit:-unknown}"
  [[ -z "$commit" ]] || printf 'Corresponding source: https://github.com/wulfnb/wega/tree/%s\n' "$commit"; } > "$out_dir/NOTICE"
printf 'Wrote license notices to %s\n' "$out_dir"
