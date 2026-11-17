#!/usr/bin/env python3
"""Run the mandatory native compatibility gate; a missing Chrome is a failure."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--chrome-bin', required=True)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    chrome = Path(args.chrome_bin).resolve()
    if not chrome.is_file():
        parser.error(f'Chrome does not exist: {chrome}')
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, WEGA_CHROME_BIN=str(chrome), WEGA_NATIVE_PINNED='1',
               WEGA_TEST_BROKER='1', WEGA_RENDER_BACKEND='native',
               WEGA_NATIVE_EVIDENCE_DIR=str(output))
    command = ['cargo', 'test', '--manifest-path', 'engines/Cargo.toml',
               '-p', 'wega-engine', '--offline']
    for suite in ['native_chrome_compatibility', 'native_graphics_acceptance',
                  'native_forms_editing', 'native_css_layout_acceptance',
                  'native_css_at_rules_acceptance', 'native_browser_behavior',
                  'wpt', 'iframe_wpt', 'native_visual_corpus']:
        command.extend(['--test', suite])
    command.extend(['--', '--test-threads=4', '--nocapture'])
    started = time.monotonic()
    with (output / 'gate.log').open('w') as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log,
                                stderr=subprocess.STDOUT, timeout=600)
    report = {'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'status': subprocess.check_output(['git', 'status', '--short'], cwd=ROOT, text=True),
              'command': command, 'exit_code': result.returncode,
              'seconds': round(time.monotonic() - started, 3),
              'thresholds': {'channel': 8, 'pixel_fraction': 0.01, 'geometry_px': 0.5}}
    (output / 'gate.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    return result.returncode


if __name__ == '__main__':
    raise SystemExit(main())
