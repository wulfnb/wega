"""Protect the benchmark against misleading process scope and CPU accounting."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('browser_bench', Path(__file__).resolve().parents[1] / 'scripts/browser-bench.py')
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class MeasurementTests(unittest.TestCase):
    def test_descendants_include_grandchildren_and_exclude_unrelated_processes(self):
        rows = {10: (1, 1000, 2.0), 11: (10, 2000, 3.0),
                12: (11, 4000, 4.0), 99: (1, 999999, 50.0)}
        result = bench.tree_sample(10, rows)
        self.assertEqual(result['pids'], [10, 11, 12])
        self.assertEqual(result['rss_bytes'], 7000)
        self.assertEqual(sum(result['cpu_by_pid'].values()), 9.0)
        self.assertEqual(bench.tree_sample(999, rows)['rss_bytes'], 0)

    def test_ps_units_and_day_cpu_values(self):
        self.assertEqual(bench.cpu_seconds('02:03.50'), 123.5)
        self.assertEqual(bench.cpu_seconds('01:02:03'), 3723)
        self.assertEqual(bench.cpu_seconds('2-01:02:03'), 176523)
        with patch.object(bench.sys, 'platform', 'darwin'), patch.object(bench.os, 'name', 'posix'), patch.object(bench.subprocess, 'check_output', return_value=' 10 1 1024 00:02.50\n'):
            self.assertEqual(bench.process_snapshot(), {10: (1, 1048576, 2.5)})

    def test_required_cases_do_not_claim_experimental_form_support(self):
        self.assertEqual(bench.REQUIRED['servo'], set(bench.PROBES))
        self.assertNotIn('forms', bench.REQUIRED['wega'])
        self.assertIn(b'FormData', bench.fixture('forms'))
        self.assertIn(b'PENDING:fetch', bench.fixture('fetch'))


if __name__ == '__main__':
    unittest.main()
