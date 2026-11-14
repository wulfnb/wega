"""The benchmark runner's parsing, aggregation and comparison are what make numbers comparable."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('engine_bench', Path(__file__).resolve().parents[1] / 'scripts/engine-bench.py')
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class ParsingTests(unittest.TestCase):
    def test_bench_lines_are_parsed_and_noise_is_ignored(self):
        output = 'running 1 test\nBENCH paint 800x600 unchanged page: 0.64 ms/frame\nBENCH a: b: 2.5 us/op\nBENCH spaced unit: 4.0 us per op\ntest result: ok\n'
        self.assertEqual(
            bench.parse(output),
            {'paint 800x600 unchanged page': (0.64, 'ms/frame'), 'a: b': (2.5, 'us/op'), 'spaced unit': (4.0, 'us per op')},
        )

    def test_aggregation_reports_median_and_spread(self):
        runs = [{'m': (3.0, 'us')}, {'m': (1.0, 'us')}, {'m': (2.0, 'us')}]
        entry = bench.aggregate(runs)['m']
        self.assertEqual((entry['median'], entry['min'], entry['max'], entry['samples']), (2.0, 1.0, 3.0, 3))

    def test_comparison_flags_only_changes_beyond_the_spread(self):
        old = {'a': {'median': 10.0, 'min': 9.8, 'max': 10.2}, 'b': {'median': 10.0, 'min': 9.0, 'max': 11.0}}
        new = {'a': {'median': 11.0, 'min': 10.9, 'max': 11.1}, 'b': {'median': 10.5, 'min': 9.5, 'max': 11.5}}
        rows = {name: (percent, significant) for name, _, _, percent, significant in bench.compare(old, new)}
        self.assertTrue(rows['a'][1], 'a 10% change with a 0.4 spread is real')
        self.assertFalse(rows['b'][1], 'a 5% change inside a 2.0 spread is noise')
        self.assertAlmostEqual(rows['a'][0], 10.0)


if __name__ == '__main__':
    unittest.main()
