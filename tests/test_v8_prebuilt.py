"""Mirror selection must never trust unchecked binaries or hide a checksum failure."""
import hashlib
import html
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('v8_prebuilt', ROOT / 'scripts/v8_prebuilt.py')
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


class MirrorTests(unittest.TestCase):
    def test_binary_engine_license_is_in_distribution_template(self):
        license_text=(ROOT/'engines/vendor/v8-notices/LICENSE').read_text()
        self.assertIn(html.escape(license_text),(ROOT/'about.hbs').read_text())

    def test_explicit_mirror_and_source_build_need_no_network(self):
        for options in [{'RUSTY_V8_MIRROR': '/mirror'}, {'V8_FROM_SOURCE': '1'}]:
            self.assertEqual(helper.prepare(environment=options, opener=lambda *_a, **_k: self.fail()), options)

    def test_upstream_available_leaves_default_downloads_in_place(self):
        requests = []
        def opener(request, **_options):
            requests.append(request.full_url)
            return io.BytesIO()
        result = helper.prepare('aarch64-apple-darwin', {}, opener)
        self.assertNotIn('RUSTY_V8_MIRROR', result)
        self.assertEqual(len(requests), 2)
        self.assertTrue(all('denoland/rusty_v8' in address for address in requests))

    def fallback(self, tamper=False):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            sums = root / 'engines/vendor/deno/V8-PREBUILT-SHA256SUMS'
            sums.parent.mkdir(parents=True)
            assets = {'librusty_v8_simdutf_release_aarch64-apple-darwin.a.gz': b'library', 'src_binding_simdutf_release_aarch64-apple-darwin.rs': b'bindings'}
            sums.write_text('\n'.join(f'{hashlib.sha256(value).hexdigest()}  {name}' for name, value in assets.items()))
            def opener(request, **_options):
                if not isinstance(request, str):
                    raise urllib.error.URLError('upstream unavailable')
                self.assertIn('/owner/repository/releases/download/deno-v8-mirror-', request)
                return io.BytesIO(b'tampered' if tamper else assets[request.rsplit('/', 1)[1]])
            (root / 'mirror').mkdir()
            with patch.object(helper, 'ROOT', root), patch.object(helper.tempfile, 'mkdtemp', return_value=str(root / 'mirror')):
                result = helper.prepare('aarch64-apple-darwin', {'GITHUB_REPOSITORY': 'owner/repository'}, opener)
                self.assertEqual(Path(result['RUSTY_V8_MIRROR']), root / 'mirror')
                for name, value in assets.items():
                    self.assertEqual((root / 'mirror' / f'v{helper.VERSION}' / name).read_bytes(), value)

    def test_fallback_requires_both_matching_checksums(self):
        self.fallback()
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            self.fallback(tamper=True)


if __name__ == '__main__':
    unittest.main()
