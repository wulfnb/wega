#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'This packaging script requires macOS.' >&2
  exit 1
fi
build_profile=release
build_args=(--release)
media_args=(--features media)
bundle_media=true
for argument in "$@"; do
  case "$argument" in
    --debug) build_profile=debug; build_args=() ;;
    --media) media_args=(--features media); bundle_media=true ;;
    --no-media) media_args=(--no-default-features --features brotli-streams); bundle_media=false ;;
    *) echo 'Usage: scripts/package-macos.sh [--debug] [--media|--no-media]' >&2; exit 1 ;;
  esac
done
cargo build --locked "${build_args[@]}" "${media_args[@]}"
if [[ "$build_profile" == debug ]]; then
  python3 scripts/build-wega-engine.py --debug
else
  python3 scripts/build-wega-engine.py
fi
app_dir="$project_dir/target/Wega.app"
mkdir -p "$app_dir/Contents/MacOS"
mkdir -p "$app_dir/Contents/Resources"
cp assets/icons/BrowserIcon.icns "$app_dir/Contents/Resources/BrowserIcon.icns"
cp "target/$build_profile/wega" "$app_dir/Contents/MacOS/wega.new"
# Replace the inode atomically so a running older app keeps its executable intact.
mv -f "$app_dir/Contents/MacOS/wega.new" "$app_dir/Contents/MacOS/wega"
cp "target/$build_profile/wega-engine" "$app_dir/Contents/MacOS/wega-engine.new"
mv -f "$app_dir/Contents/MacOS/wega-engine.new" "$app_dir/Contents/MacOS/wega-engine"
codesign --force --sign - --timestamp=none "$app_dir/Contents/MacOS/wega-engine"
cargo run --locked --manifest-path engines/Cargo.toml --package wega-platform --bin wega-content-bundle -- "target/$build_profile/wega-engine" "$app_dir"
cat > "$app_dir/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>wega</string>
  <key>CFBundleIdentifier</key><string>io.github.wulfnb.wega</string>
  <key>CFBundleName</key><string>Wega</string>
  <key>CFBundleDisplayName</key><string>Wega</string>
  <key>CFBundleIconFile</key><string>BrowserIcon.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleShortVersionString</key><string>0.1.0-alpha.1</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleURLTypes</key><array><dict>
    <key>CFBundleURLName</key><string>Web pages</string>
    <key>CFBundleTypeRole</key><string>Viewer</string>
    <key>CFBundleURLSchemes</key><array><string>http</string><string>https</string></array>
  </dict></array>
  <key>CFBundleDocumentTypes</key><array><dict>
    <key>CFBundleTypeName</key><string>HTML document</string>
    <key>CFBundleTypeRole</key><string>Viewer</string>
    <key>LSHandlerRank</key><string>Alternate</string>
    <key>LSItemContentTypes</key><array><string>public.html</string><string>public.xhtml</string></array>
  </dict></array>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
</dict></plist>
PLIST
if [[ "$build_profile" == release ]]; then
  # License files must be in place before the media bundler seals the signature.
  bash scripts/third-party-notices.sh "$app_dir/Contents/Resources" "${media_args[@]}"
fi
if $bundle_media; then
  python3 scripts/bundle-macos-media.py "$app_dir"
else
  codesign --force --sign - --timestamp=none "$app_dir"
fi
printf 'Built %s\n' "$app_dir"
