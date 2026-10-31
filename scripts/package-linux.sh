#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"

dist_dir="$project_dir/dist/wega-linux-x86_64"
binary="${1:-$project_dir/target/release/wega}"

if [[ ! -f "$binary" ]]; then
  echo "Binary not found at $binary. Pass path to executable as first argument." >&2
  exit 1
fi
engine="$(dirname "$binary")/wega-engine"
if [[ ! -f "$engine" ]]; then
  echo 'Wega engine worker is missing. Run python3 scripts/build-wega-engine.py first.' >&2
  exit 1
fi

rm -rf "$dist_dir"
mkdir -p "$dist_dir/bin" "$dist_dir/libexec" "$dist_dir/share/applications" "$dist_dir/share/icons"

# The executable lives in libexec/; bin/wega is the relocatable launcher.
cp "$binary" "$dist_dir/libexec/wega"
cp "$engine" "$dist_dir/libexec/wega-engine"
chmod 755 "$dist_dir/libexec/wega"
chmod 755 "$dist_dir/libexec/wega-engine"

# Every hicolor size (16-512 px), from scripts/generate-icons.py.
cp -R "$project_dir/assets/icons/hicolor" "$dist_dir/share/icons/hicolor"

# LICENSE, NOTICE (with the source commit) and THIRD-PARTY-NOTICES.html.
bash "$project_dir/scripts/third-party-notices.sh" "$dist_dir"

cat > "$dist_dir/bin/wega" <<'EOF'
#!/usr/bin/env sh
set -eu
app_dir="$(cd "$(dirname "$0")/.." && pwd)"
exec "$app_dir/libexec/wega" "$@"
EOF
chmod 755 "$dist_dir/bin/wega"

cat > "$dist_dir/share/applications/wega.desktop" <<'EOF'
[Desktop Entry]
Name=Wega
Comment=Wega Native Rust Browser powered by Servo
Exec=wega %U
Icon=wega
Terminal=false
Type=Application
Categories=Network;WebBrowser;
StartupWMClass=wega
MimeType=text/html;text/xml;application/xhtml+xml;x-scheme-handler/http;x-scheme-handler/https;
EOF

tar -czf "$project_dir/dist/Wega-Linux-x86_64.tar.gz" -C "$project_dir/dist" wega-linux-x86_64
echo "Packaged $project_dir/dist/Wega-Linux-x86_64.tar.gz"

