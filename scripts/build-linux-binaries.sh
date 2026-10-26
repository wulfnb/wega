#!/usr/bin/env bash
# Compile wega and wega-engine for the Docker `runtime-prebuilt` target:
#   OUT_DIR/wega, OUT_DIR/wega-engine (stripped) and OUT_DIR/notices/
# Mirrors the Dockerfile builder stage, but runs outside Docker so CI can keep
# the Cargo cache between runs. Run it in the same RUST_IMAGE as the Dockerfile
# (rust:1.98.0-slim-trixie) so the binaries match the debian:trixie runtime.
# --notices writes the full cargo-about notices; otherwise only LICENSE and NOTICE.
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
if [[ $# -lt 2 ]]; then
  echo 'Usage: scripts/build-linux-binaries.sh OUT_DIR FEATURES [--notices]' >&2
  exit 1
fi
out_dir="$1"
features="$2"
notices="${3:-}"
jobs="${BUILD_JOBS:-4}"
cd "$project_dir"

# Same profile settings as the Dockerfile builder stage.
export CARGO_INCREMENTAL=0 \
  CARGO_PROFILE_RELEASE_DEBUG=0 \
  CARGO_PROFILE_RELEASE_OPT_LEVEL=2 \
  CARGO_PROFILE_RELEASE_LTO=false \
  RUSTFLAGS="-C link-arg=-fuse-ld=lld"
cargo build --locked --features "$features" --release --jobs "$jobs"
python3 scripts/build-wega-engine.py --profile release --jobs "$jobs"

rm -rf "$out_dir"
mkdir -p "$out_dir"
cp target/release/wega target/release/wega-engine "$out_dir/"
strip "$out_dir/wega" "$out_dir/wega-engine"
if [[ "$notices" == --notices ]]; then
  bash scripts/third-party-notices.sh "$out_dir/notices" --features "$features"
else
  mkdir -p "$out_dir/notices"
  cp LICENSE NOTICE "$out_dir/notices/"
fi
printf 'Wrote Linux binaries to %s\n' "$out_dir"
