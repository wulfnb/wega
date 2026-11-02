"""The MSVC Skia cache may only ever hand a build binaries exported for its exact key."""
import importlib.util
import io
import json
from pathlib import Path, PureWindowsPath
import tarfile
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('skia_msvc_cache', ROOT / 'scripts/skia-msvc-cache.py')
cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cache)

EXPORT_KEY = 'aaaaaaaaaaaaaaaaaaaa-x86_64-pc-windows-msvc-gl-pdf-textlayout'


class Fixture(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.vcs = self.root / 'vcs.json'
        self.vcs.write_text(json.dumps({'git': {'sha1': 'b' * 40}}))

    def tearDown(self):
        self.temporary.cleanup()

    def export(self, staging, key=EXPORT_KEY):
        binaries = Path(staging) / 'skia-binaries'
        binaries.mkdir(parents=True)
        (binaries / 'key.txt').write_text(key)
        (binaries / 'skia.lib').write_bytes(b'lib')


class KeyTests(Fixture):
    def test_download_key_swaps_the_repository_hash_for_the_crate_hash(self):
        self.assertEqual(cache.download_key(EXPORT_KEY, self.vcs),
                         'b' * 20 + '-x86_64-pc-windows-msvc-gl-pdf-textlayout')

    def test_vendored_crate_carries_its_commit(self):
        self.assertEqual(len(json.loads(cache.VCS_INFO.read_text())['git']['sha1']), 40)

    def test_url_template_never_adds_a_slash_before_a_drive(self):
        url = cache.url_template(self.root)
        self.assertTrue(url.endswith('/skia-binaries-{key}.tar.gz'))
        self.assertEqual(url.removeprefix('file://'), self.root.resolve().as_posix() + '/skia-binaries-{key}.tar.gz')
        self.assertEqual('file://' + PureWindowsPath(r'D:\a\_temp\skia').as_posix(), 'file://D:/a/_temp/skia')


class PackTests(Fixture):
    def test_archive_unpacks_to_skia_binaries_and_is_named_by_download_key(self):
        staging = self.root / 'staging'
        self.export(staging)
        archive = cache.pack(staging, self.root, self.vcs)
        self.assertEqual(archive.name, f"skia-binaries-{'b' * 20}-x86_64-pc-windows-msvc-gl-pdf-textlayout.tar.gz")
        with tarfile.open(archive) as contents:
            self.assertEqual(sorted(contents.getnames()), ['skia-binaries', 'skia-binaries/key.txt', 'skia-binaries/skia.lib'])
        self.assertFalse(staging.exists())


class BuildTests(Fixture):
    def runner(self, export=True, status=0):
        calls = []

        def run(command, env):
            calls.append(env.get('BUILD_ARTIFACTSTAGINGDIRECTORY'))
            if export and env.get('BUILD_ARTIFACTSTAGINGDIRECTORY'):
                self.export(env['BUILD_ARTIFACTSTAGINGDIRECTORY'])
            return SimpleNamespace(returncode=status)
        return run, calls

    def test_empty_cache_exports_packs_and_reports(self):
        directory = self.root / 'cache'
        directory.mkdir()
        output = self.root / 'output'
        run, calls = self.runner()
        with patch('sys.stdout', io.StringIO()):
            self.assertEqual(cache.build(directory, ['cargo'], {'GITHUB_OUTPUT': str(output)}, run, self.vcs), 0)
        self.assertTrue(calls[0])
        self.assertEqual(len(cache.archives(directory)), 1)
        self.assertEqual(output.read_text(), 'skia-exported=true\n')

    def test_existing_archive_is_reused_without_exporting(self):
        directory = self.root / 'cache'
        directory.mkdir()
        (directory / 'skia-binaries-x.tar.gz').write_bytes(b'')
        run, calls = self.runner()
        self.assertEqual(cache.build(directory, ['cargo'], {}, run, self.vcs), 0)
        self.assertEqual(calls, [None])

    def test_missing_directory_means_a_plain_build(self):
        run, calls = self.runner()
        self.assertEqual(cache.build(self.root / 'absent', ['cargo'], {}, run, self.vcs), 0)
        self.assertEqual(calls, [None])

    def test_failed_build_exports_nothing(self):
        directory = self.root / 'cache'
        directory.mkdir()
        run, _ = self.runner(status=101)
        self.assertEqual(cache.build(directory, ['cargo'], {}, run, self.vcs), 101)
        self.assertEqual(cache.archives(directory), [])

    def test_build_without_skia_reports_nothing(self):
        directory = self.root / 'cache'
        directory.mkdir()
        output = self.root / 'output'
        run, _ = self.runner(export=False)
        with patch('sys.stdout', io.StringIO()):
            self.assertEqual(cache.build(directory, ['cargo'], {'GITHUB_OUTPUT': str(output)}, run, self.vcs), 0)
        self.assertFalse(output.exists())


class CommandTests(Fixture):
    def test_prepare_creates_directory_and_sets_the_url(self):
        environment = self.root / 'env'
        with patch('sys.stdout', io.StringIO()):
            self.assertEqual(cache.main(['prepare', str(self.root / 'cache'), '--github-env', str(environment)]), 0)
        self.assertTrue((self.root / 'cache').is_dir())
        self.assertTrue(environment.read_text().startswith('SKIA_BINARIES_URL=file://'))

    def test_build_requires_a_command(self):
        with patch('sys.stderr', io.StringIO()), self.assertRaises(SystemExit):
            cache.main(['build', str(self.root), '--'])

    def test_build_passes_the_command_through(self):
        with patch.object(cache, 'build', return_value=7) as build:
            self.assertEqual(cache.main(['build', 'dir', '--', 'cargo', 'build']), 7)
        build.assert_called_once_with('dir', ['cargo', 'build'])


if __name__ == '__main__':
    unittest.main()
