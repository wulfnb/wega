"""Icon generation works on a synthetic source in a temporary tree, never the real assets."""
import contextlib
import importlib.util
import io
from pathlib import Path
import runpy
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

try:
    from PIL import Image
except ImportError:  # pragma: no cover - CI installs requirements-dev.txt
    raise unittest.SkipTest("Pillow is required (pip install -r requirements-dev.txt)")

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'scripts' / 'generate-icons.py'


def load():
    spec = importlib.util.spec_from_file_location('generate_icons', SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


icons = load()


def source_image():
    """An off-centre opaque tile on a hazy transparent canvas, like the real artwork."""
    image = Image.new('RGBA', (200, 200), (0, 0, 0, 3))
    image.paste(Image.new('RGBA', (120, 110), (10, 60, 40, 255)), (30, 50))
    return image


def make_tree(root):
    (root / 'assets' / 'icons').mkdir(parents=True)
    source_image().save(root / 'assets' / 'icons' / 'browser-icon-source.png')


class IconToolTests(unittest.TestCase):
    def test_clean_tile_drops_haze_and_centres_the_artwork(self):
        tile = icons.clean_tile(source_image())
        self.assertEqual(tile.size, (120, 120))
        alpha = tile.getchannel('A')
        self.assertEqual(alpha.getpixel((0, 0)), 0)
        self.assertEqual(alpha.getbbox(), (0, 5, 120, 115))
        with self.assertRaises(ValueError):
            icons.clean_tile(Image.new('RGBA', (10, 10), (0, 0, 0, 2)))

    def test_place_uses_the_platform_grid_and_fills_small_sizes(self):
        tile = icons.clean_tile(source_image())
        macos = icons.place(tile, 1024, icons.MACOS_TILE_RATIO, shadow=True)
        opaque = macos.getchannel('A').point(lambda a: 255 if a > 200 else 0).getbbox()
        self.assertEqual(opaque[2] - opaque[0], 824)
        self.assertEqual(macos.getchannel('A').getpixel((0, 0)), 0)
        small = icons.place(tile, 16, icons.DESKTOP_TILE_RATIO)
        self.assertEqual(small.getchannel('A').getbbox(), (0, 0, 16, 16))

    def test_touch_icon_is_opaque_and_ico_entries_are_valid(self):
        tile = icons.clean_tile(source_image())
        touch = icons.opaque_square(tile, 180)
        self.assertEqual(touch.mode, 'RGB')
        self.assertEqual(touch.getpixel((0, 0)), (10, 60, 40))
        data = icons.ico_bytes([icons.place(tile, s, 0.94) for s in (16, 256)])
        self.assertEqual(struct.unpack('<HHH', data[:6]), (0, 1, 2))
        width, height = struct.unpack('<BB', data[22:24])
        self.assertEqual((width, height), (0, 0))  # 256 is stored as 0

    def test_generate_writes_every_platform_and_skips_icns_without_iconutil(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()) as output:
            root = Path(directory)
            make_tree(root)
            icons.generate(root, None)
            self.assertIn('iconutil not found', output.getvalue())
            expected = [
                'assets/icons/browser-icon.png', 'assets/icons/browser-icon-macos.png',
                'assets/icons/browser-icon-256.png', 'assets/icons/BrowserIcon.ico',
                'assets/icons/hicolor/512x512/apps/wega.png', 'site/favicon.png', 'site/logo.png',
                'docker/viewer/favicon.png', 'docker/viewer/apple-touch-icon.png',
            ]
            for name in expected:
                self.assertTrue((root / name).is_file(), name)
            self.assertFalse((root / 'assets/icons/BrowserIcon.icns').exists())
            with Image.open(root / 'docker/viewer/apple-touch-icon.png') as touch:
                self.assertEqual(touch.size, (180, 180))

    def test_icns_is_built_from_a_complete_iconset(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            calls = []

            def iconutil(command, check):
                iconset = Path(command[3])
                calls.append(sorted(p.name for p in iconset.iterdir()))
                Path(command[5]).write_bytes(b'icns')

            with patch.object(icons.subprocess, 'run', side_effect=iconutil):
                self.assertTrue(icons.build_icns(icons.clean_tile(source_image()), folder, '/usr/bin/iconutil'))
            self.assertEqual(len(calls[0]), 10)
            self.assertIn('icon_512x512@2x.png', calls[0])
            self.assertEqual((folder / 'BrowserIcon.icns').read_bytes(), b'icns')
            self.assertFalse((folder / 'BrowserIcon.iconset').exists())

    def test_command_line_targets_the_given_root(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            make_tree(root)
            with patch('shutil.which', return_value=None), patch.object(sys, 'argv', [str(SCRIPT), str(root)]):
                runpy.run_path(str(SCRIPT), run_name='__main__')
            self.assertTrue((root / 'site/logo.png').is_file())
        with patch.object(icons, 'generate') as generate:
            icons.main([])
        self.assertEqual(generate.call_args.args[0], icons.ROOT)


if __name__ == '__main__':
    unittest.main()
