#!/usr/bin/env python3
"""Select the CI jobs a pull request needs, and gate on exactly those.

Only pull requests are scoped. Every push, merge-queue, tag and CD invocation
runs the complete suite, so a release never depends on this mapping. A path no
rule recognises selects everything: an unknown file can only cost time, never
skip a check that should have run.
"""
import argparse
import json
import os
import re
import sys

JOBS = (
    'lint',
    'engine-validation',
    'error-collector',
    'core-matrix',
    'desktop-macos',
    'desktop-windows',
    'linux-binaries',
    'docker-integration',
)

DESKTOP = ('desktop-macos', 'desktop-windows')
LINUX = ('linux-binaries', 'docker-integration')

# First match wins. Lint always accompanies product jobs because they need it.
RULES = (
    # Markdown documentation, and the V8 spike that v8-engine.yml validates on its own.
    (r'^(docs/.*|[^/]+)\.md$', ()),
    (r'^engines/v8-spike/', ()),
    (r'^\.github/workflows/v8-engine\.yml$', ()),
    # The website's scripts are measured by the JavaScript coverage in lint.
    (r'^site/', ('lint',)),
    (r'^error-collector/', ('lint', 'error-collector')),
    # The desktop app bundles wega-engine and links engines/platform and
    # engines/network; both desktop suites run the packaged engine.
    (r'^engines/', ('lint', 'engine-validation') + DESKTOP),
    (r'^scripts/(build-wega-engine|v8_prebuilt|verify-engine-vendor)\.py$', ('lint', 'engine-validation') + DESKTOP),
    (r'^\.github/workflows/standalone-engine-validation\.yml$', ('lint', 'engine-validation')),
    (r'^(src|vendor|assets|tests)/', ('lint', 'core-matrix') + DESKTOP + LINUX),
    (r'^build\.rs$', ('lint', 'core-matrix') + DESKTOP + LINUX),
    (r'^(Dockerfile|docker/|compose[^/]*\.ya?ml$)', ('lint',) + LINUX),
)


def jobs_for(path):
    for pattern, jobs in RULES:
        if re.search(pattern, path):
            return set(jobs)
    return set(JOBS)


def select(paths, event):
    if event != 'pull_request':
        return set(JOBS)
    selected = set()
    for path in paths:
        selected |= jobs_for(path)
    return selected


def outputs(selected):
    lines = [f"{job}={str(job in selected).lower()}" for job in JOBS]
    lines.append(f"full={str(selected == set(JOBS)).lower()}")
    return lines


def gate(needs):
    """Return failures: a job failed, or skipped although its scope selected it."""
    scope = needs.get('changes', {})
    if scope.get('result') != 'success':
        return [f"changes: {scope.get('result')}"]
    selected = scope.get('outputs', {})
    failures = []
    for job in JOBS:
        result = needs.get(job, {}).get('result')
        if result == 'success':
            continue
        if result == 'skipped' and selected.get(job) != 'true':
            continue
        failures.append(f"{job}: {result} (selected={selected.get(job)})")
    return failures


def main(argv=None, environ=os.environ, stdin=sys.stdin, stdout=sys.stdout):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('select', 'gate'))
    args = parser.parse_args(argv)
    if args.command == 'select':
        paths = [line.strip() for line in stdin if line.strip()]
        selected = select(paths, environ.get('GITHUB_EVENT_NAME', ''))
        for line in outputs(selected):
            print(line, file=stdout)
        return 0
    failures = gate(json.loads(environ['NEEDS']))
    for failure in failures:
        print(failure, file=stdout)
    print('CI gate: ' + ('FAILED' if failures else 'all selected checks passed'), file=stdout)
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
