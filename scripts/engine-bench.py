#!/usr/bin/env python3
"""Repeatable engine benchmarks: runs the ignored `BENCH` tests of wega-engine in the release
profile, several times, and records the environment next to the numbers.

    python3 scripts/engine-bench.py --repeats 5 --output bench.json
    python3 scripts/engine-bench.py --repeats 5 --compare previous.json

Each repeat runs every benchmark function once; results are aggregated per metric as
median/min/max. The environment block (hardware, load average before and after, git revision,
profile) is part of the result because a number without it cannot be compared. Load matters:
record on a quiet machine and treat a run with a high load average as indicative only.
The compare mode prints per-metric change in the medians and flags changes larger than the
run-to-run spread of either side.
"""
import argparse
import json
import os
import platform
import re
import statistics
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ENGINES = os.path.join(ROOT, 'engines')
# (test target, ignored test function)
BENCHES = [
    ('hot_path_bench', 'hot_paths'),
    ('hot_path_bench', 'event_paths'),
    ('hot_path_bench', 'style_paths'),
    ('hot_path_bench', 'layout_paths'),
    ('hot_path_bench', 'render_paths'),
    ('render_bench', 'render_stages'),
    ('worker_bench', 'worker_paths'),
]
LINE = re.compile(r'^BENCH (?P<name>.+?): (?P<value>[\d.]+) (?P<unit>\S.*)$')


def parse(output):
    """Metric name -> (value, unit) from `BENCH name: value unit` lines."""
    metrics = {}
    for line in output.splitlines():
        match = LINE.match(line.strip())
        if match:
            metrics[match['name']] = (float(match['value']), match['unit'])
    return metrics


def aggregate(runs):
    """List of {name: (value, unit)} -> {name: {median, min, max, unit, samples}}."""
    names = {}
    for run in runs:
        for name, (value, unit) in run.items():
            names.setdefault(name, {'unit': unit, 'values': []})['values'].append(value)
    return {
        name: {
            'unit': entry['unit'],
            'samples': len(entry['values']),
            'median': statistics.median(entry['values']),
            'min': min(entry['values']),
            'max': max(entry['values']),
        }
        for name, entry in names.items()
    }


def compare(old, new):
    """Rows (name, old median, new median, percent, significant) for metrics in both."""
    rows = []
    for name in sorted(set(old) & set(new)):
        a, b = old[name], new[name]
        if a['median'] == 0:
            continue
        percent = 100.0 * (b['median'] / a['median'] - 1.0)
        # Significant only when the medians differ by more than either side's own spread.
        spread = max(a['max'] - a['min'], b['max'] - b['min'])
        rows.append((name, a['median'], b['median'], percent, abs(b['median'] - a['median']) > spread))
    return rows


def environment():
    def sysctl(key):
        try:
            return subprocess.check_output(['sysctl', '-n', key], text=True).strip()
        except (OSError, subprocess.CalledProcessError):
            return None

    def git(*args):
        try:
            return subprocess.check_output(['git', '-C', ROOT, *args], text=True).strip()
        except (OSError, subprocess.CalledProcessError):
            return None

    return {
        'platform': platform.platform(),
        'machine': platform.machine(),
        'cpu': sysctl('machdep.cpu.brand_string') or platform.processor(),
        'cores': os.cpu_count(),
        'memory_bytes': int(sysctl('hw.memsize') or 0) or None,
        'profile': 'release',
        'git_revision': git('rev-parse', '--short', 'HEAD'),
        'git_dirty': bool(git('status', '--porcelain')),
        'load_before': os.getloadavg(),
    }


def run_bench(target, function):
    command = [
        'cargo', 'test', '--release', '--offline', '-p', 'wega-engine', '--test', target,
        '--', '--ignored', '--exact', function, '--nocapture',
    ]
    result = subprocess.run(command, cwd=ENGINES, capture_output=True, text=True)
    if result.returncode != 0:
        sys.stderr.write(result.stdout[-2000:] + result.stderr[-2000:])
        raise SystemExit(f'benchmark {target}::{function} failed')
    return parse(result.stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--output')
    parser.add_argument('--compare', help='a previous --output file to compare against')
    args = parser.parse_args()
    env = environment()
    runs = []
    for repeat in range(args.repeats):
        merged = {}
        for target, function in BENCHES:
            merged.update(run_bench(target, function))
        runs.append(merged)
        print(f'repeat {repeat + 1}/{args.repeats}: {len(merged)} metrics', file=sys.stderr)
    env['load_after'] = os.getloadavg()
    result = {'environment': env, 'repeats': args.repeats, 'timestamp': int(time.time()), 'metrics': aggregate(runs)}
    if args.output:
        with open(args.output, 'w') as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
    if args.compare:
        with open(args.compare) as handle:
            old = json.load(handle)
        for name, a, b, percent, significant in compare(old['metrics'], result['metrics']):
            print(f"{'*' if significant else ' '} {name}: {a:.3f} -> {b:.3f} ({percent:+.1f}%)")
    else:
        for name, entry in sorted(result['metrics'].items()):
            print(f"{name}: {entry['median']:.3f} {entry['unit']} [{entry['min']:.3f}-{entry['max']:.3f}] n={entry['samples']}")


if __name__ == '__main__':
    main()
