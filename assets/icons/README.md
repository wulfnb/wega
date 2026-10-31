# Wega browser icon

The **Emerald Globe** icon (a green glass globe with a luminous mint orbit ring) is
the source for every app and web icon.

## Provenance and license

The Emerald Globe icon was created for Wega by the project author with AI
image-generation assistance. It is released under the Mozilla Public License 2.0
together with the rest of the project (see `LICENSE` and `NOTICE` in the
repository root). It does not reuse artwork from Firefox, Servo or other browsers.

TODO (owner): confirm that the terms of the image-generation service used allow
releasing its output under MPL-2.0, and record the service name and the date of
that check here.

## Regenerating

```sh
uv run scripts/generate-icons.py
```

`browser-icon-source.png` (1254 × 1254, transparent PNG) is the only hand-made
input. It carries a faint haze (alpha 1–8) over the whole canvas and an
off-centre tile filling about 90 % of it. The script clears the haze, crops and
centres the tile, then places it on each platform's grid. Never ship the source
directly: macOS 26+ boxes an icon that does not follow Apple's grid in a grey
system tile, which shows as a dark frame in the Dock and Spotlight. `iconutil`
(macOS only) builds the `.icns`; elsewhere that file is left unchanged.

## Files and per-platform rules

| File | Used by | Rule |
| --- | --- | --- |
| `browser-icon.png` | master | transparent, tile at 94 % of 1024 px |
| `browser-icon-macos.png` | master | Apple grid: 824 px tile centred on 1024 px, soft shadow |
| `BrowserIcon.icns` | macOS app bundle (`scripts/package-macos.sh`) | built from the Apple grid |
| `BrowserIcon.ico` | `wega.exe` (`build.rs`) and the Inno Setup installer | 16–256 px PNG entries, 94 % |
| `browser-icon-256.png` | window icon (`src/main.rs`) and sidebar brand (`src/ui.rs`) | transparent, 94 % |
| `hicolor/<N>x<N>/apps/wega.png` | Linux package with `wega.desktop` (`scripts/package-linux.sh`) | 16–512 px; the window's `wega` app id / WM class matches the desktop entry |
| `site/favicon.png`, `site/logo.png`, `docker/viewer/favicon.png` | website and Docker viewer | transparent |
| `docker/viewer/apple-touch-icon.png` | iOS home screen | opaque full-bleed square; iOS applies its own mask |

At 16–32 px the tile fills the whole canvas so the globe stays legible.

The default **Wega · Emerald Glass** theme uses emerald surfaces and mint
accents matching this icon.
