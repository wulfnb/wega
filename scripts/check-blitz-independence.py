#!/usr/bin/env python3
"""Fail closed when the selected production dependency graph contains Blitz.

Default scope is the actual browser worker, never just the native rendering flag.
--package can check an explicitly named component; component success is not
browser runtime independence. Cargo errors are errors, never successful skips.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--package', default='wega-engine')
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    command = ['cargo', 'tree', '--manifest-path', 'engines/Cargo.toml',
               '--locked', '--offline', '-p', args.package, '-e', 'normal,build',
               '--prefix', 'depth']
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    packages = sorted(set(re.findall(r'^\d+([^ ]+) v', result.stdout, re.MULTILINE)))
    forbidden = [name for name in packages if name.startswith('blitz-')
                 or name == 'wega-adapter-blitz']
    passed = result.returncode == 0 and bool(packages) and not forbidden
    report = {'package': args.package, 'scope': 'browser-runtime' if args.package == 'wega-engine' else 'component-only',
              'command': command, 'cargo_exit_code': result.returncode,
              'independent': passed, 'forbidden': forbidden, 'packages': packages,
              'tree_sha256': hashlib.sha256(result.stdout.encode()).hexdigest(),
              'stderr': result.stderr,
              'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()}
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + '\n')
    output.with_suffix('.tree.txt').write_text(result.stdout)
    print(json.dumps({key: report[key] for key in ['package', 'scope', 'independent', 'forbidden']}, indent=2))
    return 0 if passed else 1


if __name__ == '__main__':
    sys.exit(main())
