"""Pull-request scoping may skip work, but never a check a change can break."""
import importlib.util
import io
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('ci_scope', ROOT / 'scripts/ci-scope.py')
scope = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scope)

ALL = set(scope.JOBS)


def pr(*paths):
    return scope.select(paths, 'pull_request')


class SelectTests(unittest.TestCase):
    def test_every_non_pull_request_event_runs_everything(self):
        for event in ('push', 'workflow_dispatch', 'merge_group', 'schedule', ''):
            self.assertEqual(scope.select(['docs/a.md'], event), ALL)

    def test_documentation_and_spike_changes_select_nothing(self):
        self.assertEqual(pr('README.md', 'docs/step-08.md', 'docs/validation/x.md'), set())
        self.assertEqual(pr('engines/v8-spike/src/main.rs', '.github/workflows/v8-engine.yml'), set())

    def test_engine_changes_run_engine_and_desktop_suites_only(self):
        expected = {'lint', 'engine-validation', 'desktop-macos', 'desktop-windows'}
        self.assertEqual(pr('engines/wega/src/lib.rs'), expected)
        self.assertEqual(pr('engines/Cargo.lock'), expected)
        self.assertEqual(pr('scripts/build-wega-engine.py'), expected)
        self.assertEqual(pr('.github/workflows/standalone-engine-validation.yml'), {'lint', 'engine-validation'})

    def test_collector_and_site_changes_stay_local(self):
        self.assertEqual(pr('error-collector/src/main.rs'), {'lint', 'error-collector'})
        self.assertEqual(pr('site/index.html'), {'lint'})

    def test_app_changes_skip_only_engine_and_collector(self):
        for path in ('src/main.rs', 'vendor/servo-script-bindings/mem.rs', 'assets/icons/a.png', 'tests/test_auth.py', 'build.rs'):
            self.assertEqual(pr(path), ALL - {'engine-validation', 'error-collector'}, path)

    def test_docker_changes_run_the_linux_integration(self):
        expected = {'lint', 'linux-binaries', 'docker-integration'}
        for path in ('Dockerfile', 'docker/viewer/app.js', 'compose.quality.yaml', 'compose.yaml'):
            self.assertEqual(pr(path), expected, path)

    def test_shared_and_unknown_paths_fail_safe_to_everything(self):
        for path in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo/audit.toml', '.github/workflows/ci.yml',
                     'scripts/coverage.sh', 'packaging/windows/wega.iss', 'engines-new/x.rs', 'docs/diagram.svg'):
            self.assertEqual(pr(path), ALL, path)

    def test_mixed_changes_take_the_union(self):
        self.assertEqual(pr('error-collector/a.rs', 'engines/wega/a.rs'),
                         {'lint', 'error-collector', 'engine-validation', 'desktop-macos', 'desktop-windows'})

    def test_outputs_name_every_job_and_full(self):
        lines = scope.outputs({'lint'})
        self.assertIn('lint=true', lines)
        self.assertIn('core-matrix=false', lines)
        self.assertIn('full=false', lines)
        self.assertIn('full=true', scope.outputs(ALL))
        self.assertEqual(len(lines), len(scope.JOBS) + 1)


def needs(selected, results, scope_result='success'):
    value = {'changes': {'result': scope_result, 'outputs': {job: str(job in selected).lower() for job in scope.JOBS}}}
    value.update({job: {'result': results.get(job, 'skipped')} for job in scope.JOBS})
    return value


class GateTests(unittest.TestCase):
    def test_selected_jobs_must_succeed(self):
        self.assertEqual(scope.gate(needs({'lint'}, {'lint': 'success'})), [])
        self.assertEqual(len(scope.gate(needs({'lint'}, {'lint': 'failure'}))), 1)

    def test_a_selected_job_may_not_be_skipped(self):
        # A skip caused by a failed or cancelled dependency is still a failure.
        failures = scope.gate(needs({'lint', 'error-collector'}, {'lint': 'success'}))
        self.assertEqual(len(failures), 1)
        self.assertIn('error-collector: skipped', failures[0])

    def test_unselected_jobs_may_be_skipped_but_not_fail(self):
        self.assertEqual(scope.gate(needs(set(), {})), [])
        self.assertEqual(len(scope.gate(needs(set(), {'core-matrix': 'cancelled'}))), 1)

    def test_failed_scope_selection_fails_the_gate(self):
        self.assertEqual(scope.gate(needs(ALL, {}, scope_result='failure')), ['changes: failure'])
        self.assertEqual(scope.gate({}), ['changes: None'])


class CommandTests(unittest.TestCase):
    def test_select_reads_paths_and_writes_outputs(self):
        stdout = io.StringIO()
        code = scope.main(['select'], {'GITHUB_EVENT_NAME': 'pull_request'}, io.StringIO('site/a.js\n\n'), stdout)
        self.assertEqual(code, 0)
        self.assertIn('lint=true', stdout.getvalue().splitlines())
        self.assertIn('full=false', stdout.getvalue().splitlines())

    def test_gate_exit_code_follows_failures(self):
        for results, expected in (({'lint': 'success'}, 0), ({'lint': 'failure'}, 1)):
            stdout = io.StringIO()
            code = scope.main(['gate'], {'NEEDS': json.dumps(needs({'lint'}, results))}, io.StringIO(), stdout)
            self.assertEqual(code, expected)
            self.assertIn('CI gate:', stdout.getvalue())


if __name__ == '__main__':
    unittest.main()
