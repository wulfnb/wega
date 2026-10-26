#!/usr/bin/env python3
"""Account for every first-party code file; never drop unmeasured code from the gate."""
import argparse
import fnmatch
import hashlib
import json
from pathlib import Path, PureWindowsPath
import sys
from coverage_sources import fingerprint

ROOT = Path(__file__).resolve().parents[1]


def relative(path, root=ROOT):
    candidate = Path(path)
    if PureWindowsPath(path).is_absolute():
        try:
            return PureWindowsPath(path).relative_to(PureWindowsPath(root)).as_posix()
        except ValueError:
            return None
    try:
        return candidate.resolve().relative_to(root.resolve()).as_posix()
    except ValueError:
        return None


def inventory(root, policy, rust_reports, python_report, javascript=None, shell=None):
    measured = {}
    builds = set()
    failed_builds = []
    for report, metadata in rust_reports:
        if metadata.get('source_error'):
            failed_builds.append(metadata['source_error'])
            continue  # Counters from different sources cannot describe this checkout.
        if metadata.get('tests_passed') is False:
            failed_builds.append(metadata.get('platform', 'unknown'))
        platform = next((line.split(': ', 1)[1] for line in metadata.get('platform', '').splitlines() if line.startswith('host:')), 'unknown')
        valid_files = [item for data in report.get('data', []) for item in data.get('files', []) if item.get('summary', {}).get('lines', {}).get('count', 0) > 0]
        if not valid_files:
            failed_builds.append(platform + ': empty Rust measurement')
        else:
            builds.add(platform + (':core' if metadata.get('core_only') else ':desktop'))
        for data in report['data']:
            for item in data['files']:
                name = relative(item['filename'], Path(metadata.get('source_root', root)))
                if name is None:
                    continue
                summary = item['summary']
                # Keep build measurements separate. Taking the best percentage
                # from two incomplete reports would manufacture a merged result.
                measured.setdefault(name, []).append({'build': platform, 'lines': summary['lines'], 'branches': summary.get('branches') if metadata.get('branches') else None, 'uncovered_lines': metadata.get('line_details', {}).get(name, [])})
    for name, record in python_report.get('files', {}).items():
        name = relative(root / name, root) if not Path(name).is_absolute() else relative(name, root)
        if name is None:
            continue
        total = record['summary']
        measured.setdefault(name, []).append({'build': 'python', 'lines': {'count': total['num_statements'], 'covered': total['covered_lines']}, 'branches': {'count': total['num_branches'], 'covered': total['covered_branches']}, 'uncovered_lines': record.get('missing_lines', [])})
    for name, sample in (javascript or {}).items():
        measured.setdefault(name, []).append({'build': 'node', **sample})
    if shell:
        metadata = shell.get('metadata', {})
        if metadata.get('tests_passed') is not True:
            failed_builds.append('shell contracts failed or unverifiable')
        for name, sample in shell.get('files', {}).items():
            path = root / name
            expected = metadata.get('source_hashes', {}).get(name)
            if not path.is_file() or not expected or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
                failed_builds.append(name + ': stale or unverifiable shell sources')
                continue
            measured.setdefault(name, []).append({'build': 'linux-shell-contracts', **sample})
    entries = []
    files = sorted({path for pattern in policy['include'] for path in root.glob(pattern) if path.is_file()})
    for path in files:
        name = path.relative_to(root).as_posix()
        # Vendored upstream sources inside an included tree are not first-party code.
        if any(fnmatch.fnmatch(name, pattern) for pattern in policy.get('third_party', [])):
            continue
        if any(fnmatch.fnmatch(name, pattern) for pattern in policy['test_only']):
            category = 'test-driver'
        elif name in policy['declaration_only'] and not any('fn ' in line or 'impl ' in line for line in path.read_text().splitlines() if not line.lstrip().startswith('//')):
            category = 'declarations'
        else:
            category = 'first-party'
        samples = measured.get(name, [])
        complete = bool(samples) and all(sample['lines']['count'] > 0 and sample['lines']['covered'] == sample['lines']['count'] for sample in samples)
        entries.append({'file': name, 'category': category, 'measurements': samples, 'complete': complete if category == 'first-party' else None})
    missing = sorted(set(policy['required_builds']) - builds)
    return {'target_lines': policy['target_line_percent'], 'missing_builds': missing, 'failed_builds': failed_builds, 'files': entries, 'complete': bool(entries) and not missing and not failed_builds and all(item['complete'] for item in entries if item['category'] == 'first-party')}


def parse_lcov(text):
    result = {}
    name = None
    counters = {}
    uncovered = []
    for line in text.splitlines():
        if line.startswith('SF:'):
            name = line[3:].replace('\\', '/')
            counters = {}
            uncovered = []
        elif line.startswith('DA:'):
            number, count, *_ = line[3:].split(',')
            if int(count) == 0:
                uncovered.append(int(number))
        elif line.startswith(('LF:', 'LH:', 'BRF:', 'BRH:')):
            key, value = line.split(':', 1)
            counters[key] = int(value)
        elif line == 'end_of_record' and name:
            if 'LF' in counters and 'LH' in counters:
                result[name] = {'lines': {'count': counters['LF'], 'covered': counters['LH']}, 'branches': {'count': counters.get('BRF', 0), 'covered': counters.get('BRH', 0)}, 'uncovered_lines': uncovered}
            name = None
    return result


def percent(counter):
    if counter is None:
        return 'unmeasured'
    if not counter['count']:
        return 'no counters'
    return f"{100 * counter['covered'] / counter['count']:.2f}%"


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument('--require-complete', action='store_true')
    args = parser.parse_args(argv)
    policy = json.loads((ROOT / 'coverage-policy.json').read_text())
    rust = []
    directories = [ROOT / 'target/coverage/desktop', ROOT / 'target/coverage/core-branches']
    directories += sorted((ROOT / 'target/coverage/builds').glob('*'))
    for directory in directories:
        if (directory / 'measurement.json').exists() and (directory / 'coverage.json').exists():
            metadata = json.loads((directory / 'measurement.json').read_text())
            if metadata.get('source_fingerprint') != fingerprint(ROOT):
                metadata['source_error'] = str(directory) + ': stale or unverifiable Rust sources'
            details = parse_lcov((directory / 'lcov.info').read_text()) if (directory / 'lcov.info').exists() else {}
            metadata['line_details'] = {relative(name, Path(metadata.get('source_root', ROOT))): record['uncovered_lines'] for name, record in details.items()}
            rust.append((json.loads((directory / 'coverage.json').read_text()), metadata))
    python_path = ROOT / 'target/coverage/python/coverage.json'
    javascript_path = ROOT / 'target/coverage/javascript/lcov.info'
    javascript = parse_lcov(javascript_path.read_text()) if javascript_path.exists() else {}
    shell_directory = ROOT / 'target/coverage/shell'
    shell = None
    if (shell_directory / 'lcov.info').exists():
        try:
            metadata = json.loads((shell_directory / 'measurement.json').read_text())
        except (OSError, ValueError):
            metadata = {}
        shell = {'files': parse_lcov((shell_directory / 'lcov.info').read_text()), 'metadata': metadata}
    report = inventory(ROOT, policy, rust, json.loads(python_path.read_text()) if python_path.exists() else {}, javascript, shell)
    destination = ROOT / 'target/coverage'
    destination.mkdir(parents=True, exist_ok=True)
    (destination / 'inventory.json').write_text(json.dumps(report, indent=2) + '\n')
    lines = ['# First-party coverage inventory', '', 'Target: 100% lines. Branch coverage is informational. Unmeasured code prevents completion.', '', '| File | Classification | Lines | Branches (informational) | Uncovered lines |', '| --- | --- | --- | --- | --- |']
    for item in report['files']:
        samples = item['measurements']
        lines.append(f"| `{item['file']}` | {item['category']} | {' / '.join(percent(sample['lines']) for sample in samples) if samples else 'unmeasured'} | {' / '.join(percent(sample['branches']) for sample in samples) if samples else 'unmeasured'} | {', '.join(map(str, sorted({number for sample in samples for number in sample.get('uncovered_lines', [])}))) if samples else 'unmeasured'} |")
    lines += ['', 'Missing build configurations: ' + ', '.join(report['missing_builds']), '', 'Complete: ' + str(report['complete']), '']
    (destination / 'inventory.md').write_text('\n'.join(lines))
    uncovered = [item['file'] for item in report['files'] if item['category'] == 'first-party' and not item['complete']]
    print(f"{len(uncovered)} first-party files below the complete target or unmeasured; {len(report['missing_builds'])} build configurations missing")
    print(destination / 'inventory.md')
    return int(args.require_complete and not report['complete'])


if __name__ == '__main__':
    sys.exit(main())
