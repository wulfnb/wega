#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=12"]
# ///
"""Generate Wega's app and web icons from assets/icons/browser-icon-source.png.

Run with `uv run scripts/generate-icons.py`. On macOS, `iconutil` also builds
BrowserIcon.icns; elsewhere the existing .icns is kept.

The source artwork has a faint haze (alpha 1-8) across the whole canvas and an
off-centre tile filling about 90% of it. macOS 26+ boxes icons that do not
follow Apple's grid in a grey system tile, which shows as a dark frame in the
Dock and Spotlight. Every output therefore starts from the cleaned, centred
tile and places it on the grid each platform expects.
"""
from pathlib import Path
import io
import shutil
import struct
import subprocess
import sys

from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parents[1]

# Alpha at or below this is generation haze, not artwork.
HAZE_ALPHA = 8
# Apple's Big Sur grid: an 824 px tile centred on a 1024 px canvas.
MACOS_TILE_RATIO = 824 / 1024
# Windows and Linux do not mask icons, so the tile nearly fills the canvas.
DESKTOP_TILE_RATIO = 0.94
# At these sizes the whole tile fills the canvas so the globe stays legible.
SMALL_SIZES = {16, 24, 32}
HICOLOR_SIZES = [16, 24, 32, 48, 64, 128, 256, 512]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
ICNS_SIZES = [16, 32, 128, 256, 512]


def clean_tile(source):
    """The tile with haze removed, cropped to its artwork and padded square."""
    image = source.convert("RGBA")
    alpha = image.getchannel("A").point(lambda a: 0 if a <= HAZE_ALPHA else a)
    image.putalpha(alpha)
    box = alpha.getbbox()
    if box is None:
        raise ValueError("source icon has no visible artwork")
    tile = image.crop(box)
    side = max(tile.size)
    square = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    square.alpha_composite(tile, ((side - tile.width) // 2, (side - tile.height) // 2))
    return square


def place(tile, canvas, ratio, shadow=False):
    """`tile` centred on a transparent `canvas` px square at `ratio` of its width."""
    if canvas in SMALL_SIZES and not shadow:
        ratio = 1.0
    size = max(1, round(canvas * ratio))
    scaled = tile.resize((size, size), Image.Resampling.LANCZOS)
    out = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    offset = (canvas - size) // 2
    if shadow:
        # Soft drop shadow like Apple's template: ~1% offset, ~3% blur.
        layer = Image.new("L", (canvas, canvas), 0)
        layer.paste(scaled.getchannel("A").point(lambda a: a * 45 // 100),
                    (offset, offset + max(1, round(canvas * 0.012))))
        out.putalpha(layer.filter(ImageFilter.GaussianBlur(max(1.0, canvas * 0.028))))
    out.alpha_composite(scaled, (offset, offset))
    return out


def opaque_square(tile, canvas):
    """Full-bleed opaque square for platforms that apply their own mask (iOS)."""
    # Average the opaque pixels of a strip just inside the tile's top rim,
    # away from the globe (the tile may sit lower in its square padding).
    top = tile.getchannel("A").getbbox()[1]
    y = top + tile.height * 4 // 100
    strip = [p for x in range(tile.width * 3 // 10, tile.width * 7 // 10)
             if (p := tile.getpixel((x, y)))[3] > 200]
    background = tuple(sum(p[c] for p in strip) // len(strip) for c in range(3))
    out = Image.new("RGBA", (canvas, canvas), background + (255,))
    out.alpha_composite(tile.resize((canvas, canvas), Image.Resampling.LANCZOS))
    return out.convert("RGB")


def ico_bytes(images):
    """A Windows Vista+ ICO containing `images` as PNG entries."""
    payloads = []
    for image in images:
        buffer = io.BytesIO()
        image.save(buffer, format="PNG", optimize=True)
        payloads.append((image.width, image.height, buffer.getvalue()))
    header = struct.pack("<HHH", 0, 1, len(payloads))
    offset = 6 + 16 * len(payloads)
    entries = b""
    for width, height, data in payloads:
        # A width/height byte of 0 means 256.
        entries += struct.pack("<BBBBHHII", width % 256, height % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return header + entries + b"".join(data for _, _, data in payloads)


def save(image, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    image.save(path, optimize=True)


def build_icns(tile, icons, iconutil):
    """BrowserIcon.icns from the Apple grid; skipped without `iconutil`."""
    if iconutil is None:
        print("iconutil not found (macOS only); keeping the existing BrowserIcon.icns")
        return False
    iconset = icons / "BrowserIcon.iconset"
    shutil.rmtree(iconset, ignore_errors=True)
    for size in ICNS_SIZES:
        for scale, suffix in ((1, ""), (2, "@2x")):
            save(place(tile, size * scale, MACOS_TILE_RATIO, shadow=True),
                 iconset / f"icon_{size}x{size}{suffix}.png")
    subprocess.run([iconutil, "-c", "icns", str(iconset), "-o", str(icons / "BrowserIcon.icns")], check=True)
    shutil.rmtree(iconset)
    return True


def generate(root, iconutil):
    icons = root / "assets" / "icons"
    source = Image.open(icons / "browser-icon-source.png")
    tile = clean_tile(source)

    save(place(tile, 1024, DESKTOP_TILE_RATIO), icons / "browser-icon.png")
    save(place(tile, 1024, MACOS_TILE_RATIO, shadow=True), icons / "browser-icon-macos.png")
    # Window icon (src/main.rs) and sidebar brand mark (src/ui.rs).
    save(place(tile, 256, DESKTOP_TILE_RATIO), icons / "browser-icon-256.png")
    for size in HICOLOR_SIZES:
        save(place(tile, size, DESKTOP_TILE_RATIO), icons / "hicolor" / f"{size}x{size}" / "apps" / "wega.png")
    (icons / "BrowserIcon.ico").write_bytes(ico_bytes([place(tile, s, DESKTOP_TILE_RATIO) for s in ICO_SIZES]))
    build_icns(tile, icons, iconutil)

    for web in (root / "site", root / "docker" / "viewer"):
        save(place(tile, 64, DESKTOP_TILE_RATIO), web / "favicon.png")
    save(place(tile, 256, DESKTOP_TILE_RATIO), root / "site" / "logo.png")
    save(opaque_square(tile, 180), root / "docker" / "viewer" / "apple-touch-icon.png")
    print("Icon suite generated")


def main(argv=None):
    root = Path(argv[0]) if argv else ROOT
    generate(root, shutil.which("iconutil"))


if __name__ == "__main__":
    main(sys.argv[1:])
