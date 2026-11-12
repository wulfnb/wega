"""Release helpers operate on a temporary app and source SDK, never installed libs."""
import contextlib
import importlib.util
import io
import json
import os
import plistlib
from pathlib import Path
import runpy
import shlex
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]

def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), ROOT / 'scripts' / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

credentials = load('web-credentials')
media = load('bundle-macos-media')
preview = load('sync-preview-release')


class PreviewReleaseTests(unittest.TestCase):
    def test_only_preview_moves_and_only_known_obsolete_assets_are_removed(self):
        assets = [{'id': index, 'name': name} for index, name in enumerate(sorted(preview.OBSOLETE), 1)]
        assets += [{'id': 99, 'name': 'Wega-Windows-x64.zip'}, {'id': 100, 'name': 'SHA256SUMS.txt'}]
        sha = 'a' * 40
        with patch.object(preview.subprocess, 'run') as run, patch.object(
            preview.subprocess, 'check_output', return_value=json.dumps({'assets': assets})
        ):
            preview.main(['--repository', 'wulfnb/wega', '--tag', 'test-preview', '--sha', sha])
        self.assertEqual(json.loads(run.call_args_list[0].kwargs['input']), {'sha': sha, 'force': True})
        self.assertEqual(run.call_args_list[0].args[0][4], 'repos/wulfnb/wega/git/refs/tags/test-preview')
        self.assertEqual([call.args[0][-1] for call in run.call_args_list[1:]],
                         [f'repos/wulfnb/wega/releases/assets/{index}' for index in range(1, 5)])
        for call in run.call_args_list:
            self.assertTrue(call.kwargs['check'])

    def test_immutable_tags_and_invalid_inputs_never_mutate_github(self):
        with patch.object(preview.subprocess, 'run') as run:
            for tag in ['v0.1.0', 'test-preview-' + 'a' * 12, 'custom-preview']:
                preview.synchronize('wulfnb/wega', tag, 'a' * 40)
            for repository, sha in [('bad/repo/path', 'a' * 40), ('wulfnb/wega', 'short')]:
                with self.assertRaises(ValueError):
                    preview.synchronize(repository, 'test-preview', sha)
            run.assert_not_called()

    def test_tag_update_failure_prevents_cleanup(self):
        with patch.object(preview.subprocess, 'run', side_effect=preview.subprocess.CalledProcessError(1, 'gh')):
            with patch.object(preview.subprocess, 'check_output') as read:
                with self.assertRaises(preview.subprocess.CalledProcessError):
                    preview.synchronize('wulfnb/wega', 'test-preview', 'a' * 40)
                read.assert_not_called()

    def test_cli_entrypoint(self):
        args = ['sync-preview-release.py', '--repository', 'wulfnb/wega', '--tag', 'v0.1.0', '--sha', 'a' * 40]
        with patch.object(sys, 'argv', args):
            runpy.run_path(preview.__file__, run_name='__main__')

class CredentialToolTests(unittest.TestCase):
    def test_rewrite_is_private_and_does_not_print_secrets(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()) as output:
            root = Path(directory)
            (root / '.env').write_text("OTHER=keep\nWEGA_WEB_USER=old\n")
            values = {'WEGA_WEB_USER': 'enc:v1:user', 'WEGA_WEB_PASSWORD': 'enc:v1:secret'}
            with patch.object(credentials.subprocess, 'check_output', return_value=json.dumps(values)) as run:
                credentials.main(['encrypt-env'], root)
                self.assertEqual(run.call_args.kwargs['cwd'], root)
            content = (root / '.env').read_text()
            self.assertIn('OTHER=keep', content)
            self.assertIn("WEGA_WEB_USER='enc:v1:user'", content)
            self.assertIn("WEGA_WEB_PASSWORD='enc:v1:secret'", content)
            self.assertNotIn('secret', output.getvalue())
            self.assertEqual((root / '.env').stat().st_mode & 0o777, 0o600)
        with patch.object(credentials.subprocess, 'check_output', return_value='explicit bootstrap output'), contextlib.redirect_stdout(io.StringIO()) as output:
            credentials.main(['show'])
            self.assertEqual(output.getvalue(), 'explicit bootstrap output')
        with patch.object(sys, 'argv', ['web-credentials.py']), self.assertRaises(SystemExit):
            runpy.run_path(credentials.__file__, run_name='__main__')

class MediaPackagerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.sdk = self.root / 'sdk/lib/gstreamer-1.0'
        self.sdk.mkdir(parents=True)
        self.app = self.root / 'Wega.app'
        self.binary = self.app / 'Contents/MacOS/wega'
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(b'original executable')
        lists = self.root / 'packaging/media/gstreamer_plugin_lists'
        lists.mkdir(parents=True)
        (lists / 'common.rs.in').write_text('["alpha"] // comment\n')
        (lists / 'macos.rs.in').write_text('["beta"]\n')
        for name in ['libalpha.dylib', 'libbeta.dylib', 'libhelper.dylib']:
            (self.sdk / name).write_bytes(b'source ' + name.encode())
        (self.sdk.parent / 'libcommon.dylib').write_bytes(b'common source')
        self.dependencies = {}
        self.formulae = {}
        self.calls = []

    def run_bundle(self, override=True):
        def check_output(command, **_):
            if command[0] == 'pkg-config':
                self.assertFalse(override)
                return str(self.sdk)
            if command[0] == 'brew':
                self.assertEqual(command[:3], ['brew', 'info', '--json=v2'])
                return json.dumps({'formulae': [self.formulae[name] for name in command[3:]]})
            self.assertEqual(command[0], 'otool')
            original = command[-1]
            deps = self.dependencies.get(original, [])
            return original + ':\n' + ''.join('  ' + dependency + ' (compatibility version 1.0.0)\n' for dependency in deps)
        with patch.dict(os.environ, {'WEGA_GSTREAMER_PLUGIN_DIR': str(self.sdk)} if override else {}, clear=True), patch.object(media.subprocess, 'check_output', side_effect=check_output), patch.object(media.subprocess, 'run', side_effect=lambda command, **_: self.calls.append(command)), contextlib.redirect_stdout(io.StringIO()):
            media.bundle(self.app, self.root)

    def test_dependency_closure_is_copied_rewritten_and_signed_only_inside_app(self):
        alpha = str(self.sdk / 'libalpha.dylib')
        beta = str(self.sdk / 'libbeta.dylib')
        common = str(self.sdk.parent / 'libcommon.dylib')
        self.dependencies[alpha] = [alpha, '/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation', '/usr/lib/libSystem.B.dylib', '@loader_path/libalpha.dylib', '@loader_path/libhelper.dylib', '@rpath/libcommon.dylib']
        self.dependencies[beta] = [common]
        self.dependencies[str(self.binary)] = [common]
        self.run_bundle(override=False)
        self.assertEqual((self.sdk / 'libalpha.dylib').read_bytes(), b'source libalpha.dylib')
        self.assertEqual(self.binary.read_bytes(), b'original executable')
        manifest = json.loads((self.app / 'Contents/Resources/media-libraries.json').read_text())
        self.assertEqual(len(manifest), 4)
        self.assertTrue(any('@loader_path/lib/libcommon.dylib' in command for command in self.calls))
        self.assertTrue(any(command[:2] == ['codesign', '--verify'] for command in self.calls))
        for command in self.calls:
            self.assertTrue(Path(command[-1]).is_relative_to(self.app))
        self.run_bundle()  # Override does not invoke pkg-config; existing copied files are safely replaced.

    def test_homebrew_libraries_get_license_and_source_notices(self):
        cellar = self.root / 'Cellar'
        glib = cellar / 'glib/2.90.1/lib/libglib-2.0.0.dylib'
        intl = cellar / 'gettext/1.0/lib/libintl.8.dylib'
        for library in [glib, intl]:
            library.parent.mkdir(parents=True)
            library.write_bytes(b'homebrew ' + library.name.encode())
        self.formulae['glib'] = {'name': 'glib', 'license': 'LGPL-2.1-or-later', 'homepage': 'https://docs.gtk.org/glib/', 'ruby_source_path': 'Formula/g/glib.rb', 'urls': {'stable': {'url': 'https://download.gnome.org/sources/glib/2.90/glib-2.90.1.tar.xz'}}}
        self.formulae['gettext'] = {'name': 'gettext', 'license': None, 'urls': {}}
        self.dependencies[str(self.sdk / 'libalpha.dylib')] = [str(glib), str(intl)]
        self.run_bundle()
        manifest = json.loads((self.app / 'Contents/Resources/media-libraries.json').read_text())
        self.assertIn({'file': glib.name, 'source': str(glib), 'formula': 'glib', 'version': '2.90.1'}, manifest)
        notices = (self.app / 'Contents/Resources/MEDIA-LIBRARIES.txt').read_text()
        self.assertIn('Homebrew formula: glib 2.90.1', notices)
        self.assertIn('License: LGPL-2.1-or-later', notices)
        self.assertIn('https://github.com/Homebrew/homebrew-core/blob/HEAD/Formula/g/glib.rb', notices)
        self.assertIn('glib-2.90.1.tar.xz', notices)
        self.assertIn('Homebrew formula: gettext 1.0\n  License: see upstream project', notices)
        self.assertIn('Copied from: ' + str(self.sdk / 'libalpha.dylib'), notices)
        self.assertIn('Written offer', notices)
        self.assertIn('https://github.com/wulfnb/wega/issues', notices)

    def test_gpl_and_patent_encumbered_codecs_are_refused_before_copying(self):
        for name in ['libx264.164.dylib', 'libx265.215.dylib', 'libavcodec.62.dylib', 'libavformat.62.dylib', 'libswscale.9.dylib', 'libswresample.6.dylib', 'libpostproc.59.dylib', 'libfaac.0.dylib', 'libgstlibav.dylib']:
            codec = self.root / 'codecs' / name
            codec.parent.mkdir(exist_ok=True)
            codec.write_bytes(b'codec')
            self.dependencies[str(self.sdk / 'libalpha.dylib')] = [str(codec)]
            self.calls.clear()
            with self.subTest(name=name), self.assertRaisesRegex(RuntimeError, 'Refusing to bundle GPL/patent-encumbered codec library ' + name.replace('.', r'\.')):
                self.run_bundle()
            self.assertFalse((self.app / 'Contents/MacOS/lib' / name).exists())
            self.assertFalse(any(command[0] == 'codesign' for command in self.calls))
        # A listed gst-libav plugin is refused even before its dependencies are read.
        self.dependencies.clear()
        (self.sdk / 'libgstlibav.dylib').write_bytes(b'plugin')
        (self.root / 'packaging/media/gstreamer_plugin_lists/macos.rs.in').write_text('["beta", "gstlibav"]\n')
        with self.assertRaisesRegex(RuntimeError, 'libgstlibav'):
            self.run_bundle()

    def test_shipped_plugin_lists_use_system_codecs_without_gst_libav(self):
        plugins = {}
        for name in ['common', 'macos']:
            content = (ROOT / f'packaging/media/gstreamer_plugin_lists/{name}.rs.in').read_text()
            plugins[name] = media.ast.literal_eval(media.re.sub(r'//[^\n]*', '', content))
        self.assertNotIn('gstlibav', plugins['common'] + plugins['macos'])
        self.assertIn('gstapplemedia', plugins['macos'])
        self.assertFalse(any(media.FORBIDDEN.match(f'lib{plugin}.dylib') for plugin in plugins['common'] + plugins['macos']))

    def test_missing_collision_and_unresolved_dependencies_fail_before_signing(self):
        other = self.root / 'other/libalpha.dylib'
        other.parent.mkdir()
        other.write_bytes(b'other source')
        for dependency, message in [(str(other), 'collision'), ('@rpath/missing.dylib', 'Unresolved rpath'), ('relative.dylib', 'Unresolved dependency')]:
            self.dependencies[str(self.sdk / 'libalpha.dylib')] = [dependency]
            with self.subTest(dependency=dependency), self.assertRaisesRegex(RuntimeError, message):
                self.run_bundle()
        (self.sdk / 'libalpha.dylib').unlink()
        with self.assertRaisesRegex(RuntimeError, 'Missing media dependency'):
            self.run_bundle()
        with patch.object(sys, 'argv', ['bundle-macos-media.py']), self.assertRaises(SystemExit):
            runpy.run_path(media.__file__, run_name='__main__')
        with patch.object(media, 'bundle') as bundle:
            media.main([str(self.app)])
            bundle.assert_called_once_with(str(self.app))

class DockerBuildInputsTests(unittest.TestCase):
    def test_package_build_script_is_copied_before_cargo_build(self):
        manifest = tomllib.loads((ROOT / 'Cargo.toml').read_text())
        build_script = manifest['package']['build']
        self.assertTrue((ROOT / build_script).is_file())
        before_build = (ROOT / 'Dockerfile').read_text().split('    cargo build --locked', 1)[0]
        sources = []
        for line in before_build.splitlines():
            if line.startswith('COPY ') and '--from=' not in line:
                sources.extend(shlex.split(line)[1:-1])
        self.assertIn(build_script, sources,
                      'Fresh Docker builds need the package build script, not just Cargo.toml and src/')


class MacBundleTypesTests(unittest.TestCase):
    def test_bundle_declares_web_urls_and_html_viewing_without_claiming_ownership(self):
        script = (ROOT / 'scripts/package-macos.sh').read_text()
        xml = script.split("<<'PLIST'\n", 1)[1].split('\nPLIST', 1)[0]
        info = plistlib.loads(xml.encode())
        self.assertEqual(info['CFBundleURLTypes'][0]['CFBundleURLSchemes'], ['http', 'https'])
        self.assertEqual(info['CFBundleURLTypes'][0]['CFBundleTypeRole'], 'Viewer')
        document = info['CFBundleDocumentTypes'][0]
        self.assertEqual(document['CFBundleTypeRole'], 'Viewer')
        self.assertEqual(document['LSHandlerRank'], 'Alternate')
        self.assertEqual(document['LSItemContentTypes'], ['public.html', 'public.xhtml'])


if __name__ == '__main__':
    unittest.main()
