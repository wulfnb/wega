"""Coverage gates are release controls: partial, rounded or missing data must fail."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import sys
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from coverage_sources import fingerprint

def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), ROOT / 'scripts' / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

check = load('check-coverage')
accounting = load('coverage-inventory')
desktop = load('desktop-coverage')

class PortableWrapperTests(unittest.TestCase):
    def test_wrapper_preserves_arguments_flags_and_compiler_exit_status(self):
        import os
        import shutil
        import subprocess
        if not shutil.which('rustc'):
            self.skipTest('Rust toolchain is required for compiler-wrapper contracts')
        with tempfile.TemporaryDirectory() as directory:
            wrapper = Path(directory) / ('wrapper.exe' if os.name == 'nt' else 'wrapper')
            subprocess.run(['rustc', str(ROOT / 'scripts/coverage-rustc.rs'), '--edition=2024', '-o', str(wrapper)], check=True)
            for branch in ['0','1']:
                env = dict(os.environ, WEGA_COVERAGE_BRANCH=branch)
                output = subprocess.check_output([str(wrapper), sys.executable, '-c', 'import json,sys; print(json.dumps(sys.argv[1:]))', 'literal argument with spaces', '$not_a_shell'], env=env, text=True)
                args = json.loads(output)
                self.assertEqual(args[:2], ['literal argument with spaces', '$not_a_shell'])
                self.assertIn('instrument-coverage', args)
                self.assertEqual('coverage-options=branch' in args, branch == '1')
                if sys.platform == 'darwin':
                    self.assertTrue(any('__llvm_prf_cnts,0x4000' in arg for arg in args))
            self.assertEqual(subprocess.run([str(wrapper), sys.executable, '-c', 'import sys;sys.exit(17)']).returncode,17)
            self.assertNotEqual(subprocess.run([str(wrapper)], capture_output=True).returncode,0)
            self.assertNotEqual(subprocess.run([str(wrapper), str(Path(directory)/'missing')], capture_output=True).returncode,0)

class DesktopVerificationTests(unittest.TestCase):
    def test_failed_native_suite_remains_a_failure_after_coverage_export(self):
        measurement = {'tests_passed': False, 'failures': ['native smoke (exit 101)', 'native logs: /test/run']}
        with contextlib.redirect_stderr(io.StringIO()) as diagnostic:
            self.assertEqual(desktop.verification_status(measurement), 1)
        self.assertIn('native smoke (exit 101)', diagnostic.getvalue())
        self.assertIn('/test/run', diagnostic.getvalue())
        with contextlib.redirect_stderr(io.StringIO()) as diagnostic:
            self.assertEqual(desktop.verification_status({'tests_passed': False}), 1)
        self.assertIn('inspect test and native logs', diagnostic.getvalue())
        with contextlib.redirect_stderr(io.StringIO()) as diagnostic:
            self.assertEqual(desktop.verification_status({'tests_passed': True}), 0)
        self.assertEqual(diagnostic.getvalue(), '')


class CoverageGateTests(unittest.TestCase):
    def reports(self, covered=10, count=10):
        return {'data': [{'totals': {'lines': {'count': count, 'covered': covered}}}]}, {'files': {'docker/auth-service.py': {'summary': {'num_statements': 10, 'covered_lines': 10}}}}

    def test_every_line_is_required_even_when_a_percentage_rounds_to_100(self):
        baseline = {'rust_core_line_percent': 100, 'python_gateway_line_percent': 100}
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertTrue(check.enforce(baseline, *self.reports()))
            self.assertFalse(check.enforce(baseline, *self.reports(999999, 1000000)))
            with self.assertRaises(ValueError):
                check.enforce(baseline, *self.reports(0, 0))
            baseline['rust_core_line_percent'] = 80
            self.assertTrue(check.enforce(baseline, *self.reports(8, 10)))

    def test_missing_or_corrupt_reports_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            self.assertEqual(check.main(root), 1)
            (root / 'coverage-baseline.json').write_text('invalid JSON')
            self.assertEqual(check.main(root), 1)
            (root / 'coverage-baseline.json').write_text(json.dumps({'rust_core_line_percent': 100, 'python_gateway_line_percent': 100}))
            (root / 'target/coverage/rust').mkdir(parents=True)
            (root / 'target/coverage/python').mkdir()
            rust, python = self.reports()
            (root / 'target/coverage/rust/coverage.json').write_text(json.dumps(rust))
            (root / 'target/coverage/python/coverage.json').write_text(json.dumps(python))
            self.assertEqual(check.main(root), 0)
            rust['data'][0]['totals']['lines']['covered'] = 9
            (root / 'target/coverage/rust/coverage.json').write_text(json.dumps(rust))
            self.assertEqual(check.main(root), 1)

    def test_unmeasured_file_and_missing_platform_cannot_disappear(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'src').mkdir()
            (root / 'src/covered.rs').write_text('fn covered() {}')
            (root / 'src/missing.js').write_text('console.log("not tested")')
            (root / 'src/example_tests.rs').write_text('fn test() {}')
            (root / 'src/brand.rs').write_text('const NAME: &str = "Wega";')
            policy = {'include': ['src/*'], 'test_only': ['src/*_tests.rs'], 'declaration_only': ['src/brand.rs'], 'required_builds': ['test:desktop'], 'target_line_percent': 100}
            rust = {'data': [{'files': [{'filename': str(root / 'src/covered.rs'), 'summary': {'lines': {'count': 1, 'covered': 1}}}]}]}
            report = accounting.inventory(root, policy, [(rust, {'platform': 'host: test', 'core_only': False})], {})
            self.assertFalse(report['complete'])
            self.assertIn('src/missing.js', [item['file'] for item in report['files'] if item['complete'] is False])
            (root / 'src/missing.js').unlink()
            self.assertTrue(accounting.inventory(root, policy, [(rust, {'platform': 'host: test'})], {})['complete'])
            self.assertFalse(accounting.inventory(root, policy, [(rust, {'platform': 'host: other'})], {})['complete'])
            (root / 'src/brand.rs').write_text('fn now_executable() {}')
            self.assertFalse(accounting.inventory(root, policy, [(rust, {'platform': 'host: test'})], {})['complete'])

    def test_vendored_third_party_sources_are_not_first_party(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'engines/wega').mkdir(parents=True)
            (root / 'engines/vendor/deno/deno_core').mkdir(parents=True)
            (root / 'engines/wega/lib.rs').write_text('fn covered() {}')
            (root / 'engines/vendor/deno/deno_core/lib.rs').write_text('fn upstream() {}')
            policy = {'include': ['engines/**/*.rs'], 'test_only': [], 'third_party': ['engines/vendor/**'], 'declaration_only': [], 'required_builds': ['test:desktop'], 'target_line_percent': 100}
            rust = {'data': [{'files': [{'filename': str(root / 'engines/wega/lib.rs'), 'summary': {'lines': {'count': 1, 'covered': 1}}}]}]}
            report = accounting.inventory(root, policy, [(rust, {'platform': 'host: test'})], {})
            self.assertTrue(report['complete'])
            self.assertEqual([item['file'] for item in report['files']], ['engines/wega/lib.rs'])
            policy['third_party'] = []
            self.assertFalse(accounting.inventory(root, policy, [(rust, {'platform': 'host: test'})], {})['complete'])

    def test_shell_counters_require_passing_contracts_and_matching_sources(self):
        import hashlib
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / 'start.sh'
            path.write_text('echo startup\n')
            policy = {'include': ['*.sh'], 'test_only': [], 'declaration_only': [], 'required_builds': [], 'target_line_percent': 100}
            shell = {'files': {'start.sh': {'lines': {'count': 1, 'covered': 1}, 'uncovered_lines': []}}, 'metadata': {'tests_passed': True, 'source_hashes': {'start.sh': hashlib.sha256(path.read_bytes()).hexdigest()}}}
            self.assertTrue(accounting.inventory(root, policy, [], {}, shell=shell)['complete'])
            shell['metadata']['tests_passed'] = False
            self.assertFalse(accounting.inventory(root, policy, [], {}, shell=shell)['complete'])
            shell['metadata']['tests_passed'] = True
            path.write_text('echo changed\n')
            stale = accounting.inventory(root, policy, [], {}, shell=shell)
            self.assertFalse(stale['complete'])
            self.assertEqual(stale['files'][0]['measurements'], [])
            shell['metadata']['source_hashes'].clear()
            self.assertFalse(accounting.inventory(root, policy, [], {}, shell=shell)['complete'])

    def test_empty_platform_report_never_satisfies_a_required_build(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            (root/'src').mkdir()
            (root/'src/covered.rs').write_text('fn covered() {}')
            policy={'include':['src/*'], 'test_only':[], 'declaration_only':[], 'required_builds':['test:desktop'], 'target_line_percent':100}
            result=accounting.inventory(root, policy, [({'data':[{'files':[]}]}, {'platform':'host: test'})], {})
            self.assertFalse(result['complete'])
            self.assertEqual(result['missing_builds'], ['test:desktop'])
            self.assertIn('test: empty Rust measurement', result['failed_builds'])

    def test_source_fingerprint_detects_edits_additions_and_removals(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            (root/'src').mkdir()
            for name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'):
                (root/name).write_text(name)
            source=root/'src/lib.rs'
            source.write_text('fn original() {}')
            original=fingerprint(root)
            self.assertEqual(original, fingerprint(root))
            source.write_text('fn changed() {}')
            self.assertNotEqual(original, fingerprint(root))
            source.write_text('fn original() {}')
            extra=root/'src/new.rs'
            extra.write_text('fn new() {}')
            self.assertNotEqual(original, fingerprint(root))
            extra.unlink()
            self.assertEqual(original, fingerprint(root))

    def test_inventory_rejects_full_counters_for_changed_or_unverifiable_sources(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            root=Path(directory)
            (root/'src').mkdir()
            source=root/'src/covered.rs'
            source.write_text('fn covered() {}')
            for name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'):
                (root/name).write_text(name)
            (root/'coverage-policy.json').write_text(json.dumps({'include':['src/*'], 'test_only':[], 'declaration_only':[], 'required_builds':['test:desktop'], 'target_line_percent':100}))
            output=root/'target/coverage/desktop'
            output.mkdir(parents=True)
            (output/'coverage.json').write_text(json.dumps({'data':[{'files':[{'filename':str(source), 'summary':{'lines':{'count':1,'covered':1}}}]}]}))
            metadata={'platform':'host: test', 'source_root':str(root), 'tests_passed':True}
            manifest=output/'measurement.json'
            manifest.write_text(json.dumps(metadata))
            with patch.object(accounting, 'ROOT', root):
                self.assertEqual(accounting.main(['--require-complete']), 1)
                metadata['source_fingerprint']=fingerprint(root)
                manifest.write_text(json.dumps(metadata))
                self.assertEqual(accounting.main(['--require-complete']), 0)
                source.write_text('fn newly_uncovered() {}')
                self.assertEqual(accounting.main(['--require-complete']), 1)

    def test_lcov_line_counters_are_read_exactly(self):
        report = accounting.parse_lcov('TN:\nSF:docker/viewer/login.js\nLH:24\nLF:24\nBRH:8\nBRF:9\nend_of_record\n')
        self.assertEqual(report['docker/viewer/login.js']['lines'], {'count': 24, 'covered': 24})
        self.assertEqual(report['docker/viewer/login.js']['branches']['covered'], 8)
        self.assertEqual(accounting.percent(None), 'unmeasured')
        self.assertEqual(accounting.percent({'count': 0, 'covered': 0}), 'no counters')
        self.assertIsNone(accounting.relative('/outside/workspace', Path('/tmp/project')))
        self.assertEqual(accounting.relative('C:/work/wega/src/profile.rs', 'C:/work/wega'), 'src/profile.rs')
        self.assertIsNone(accounting.relative('C:/outside/profile.rs', 'C:/work/wega'))


@unittest.skipUnless(sys.platform == 'darwin', 'Continuous mmap coverage is validated on Darwin')
class ContinuousCounterTests(unittest.TestCase):
    def test_terminated_process_retains_nonzero_profile_counters(self):
        import os
        import subprocess
        with tempfile.TemporaryDirectory(prefix='wega-counter-test-') as directory:
            root = Path(directory)
            source = root / 'probe.rs'
            source.write_text('fn main() { println!("ready"); std::thread::sleep(std::time::Duration::from_secs(30)); }')
            binary = root / 'probe'
            subprocess.run([str(ROOT / 'scripts/coverage-rustc.sh'), 'rustc', str(source), '-o', str(binary)], check=True, capture_output=True)
            environment = os.environ.copy()
            environment['LLVM_PROFILE_FILE'] = str(root / 'probe-%p%c.profraw')
            process = subprocess.Popen([str(binary)], env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                self.assertEqual(process.stdout.readline().strip(), 'ready')
            finally:
                process.terminate()
            _, errors = process.communicate(timeout=5)
            self.assertNotIn('LLVM Profile Error', errors)
            sysroot = Path(subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip())
            host = next(line.split(': ', 1)[1] for line in subprocess.check_output(['rustc', '-vV'], text=True).splitlines() if line.startswith('host:'))
            profiler = sysroot / 'lib/rustlib' / host / 'bin/llvm-profdata'
            result = subprocess.check_output([str(profiler), 'show', '--all-functions', '--counts', str(next(root.glob('*.profraw')))], text=True)
            self.assertIn('Function count: 1', result)

class SourceFingerprintTests(unittest.TestCase):
    def test_vendor_generator_and_embedded_script_changes_invalidate_measurement(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'src/lib.rs', 'src/reader.js',
                         'vendor/bindings/mem.rs', 'vendor/bindings/codegen.py']:
                file = root / name
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_text('original')
            before = fingerprint(root)
            cache = root / 'vendor/bindings/__pycache__/generated.py'
            cache.parent.mkdir()
            cache.write_text('generated cache')
            self.assertEqual(fingerprint(root), before)
            for name in ['src/reader.js', 'vendor/bindings/mem.rs', 'vendor/bindings/codegen.py']:
                file = root / name
                file.write_text('changed')
                self.assertNotEqual(fingerprint(root), before, name)
                file.write_text('original')


if __name__ == '__main__':
    unittest.main()
