#!/usr/bin/env python3
"""Run the real engine against isolated profiles, with bounded subprocess lifetimes."""
import argparse
import json
import os
import signal
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
from http.server import ThreadingHTTPServer
import importlib.util

root = Path(__file__).resolve().parents[1]

def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', default=str(root / 'target/debug/wega'))
    parser.add_argument('--output', default=str(root / 'target/native-tests'))
    parser.add_argument('--suite', choices=['all', 'smoke', 'webdriver', 'media', 'capture', 'engine'], nargs='+', default=['all'])
    args = parser.parse_args()
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    result_path = output / "results.json"
    result = json.loads(result_path.read_text()) if result_path.exists() else []
    current_failed = False
    for suite in ['smoke', 'webdriver', 'media', 'capture', 'engine']:
        if 'all' not in args.suite and suite not in args.suite:
            continue
        module = load(f'wega_{suite}_fixture', root / 'tests' / ('download_server.py' if suite == 'download' else 'network_server.py'))
        server = ThreadingHTTPServer(('127.0.0.1', 0), module.Handler)
        # Exercise reloads slower than the smoke driver's former 650ms wait.
        if suite == 'smoke':
            server.smoke_reload_delay = 1.2
        threading.Thread(target=server.serve_forever, daemon=True).start()
        fixture = {'smoke': 'interaction.html', 'theme': 'interaction.html', 'capabilities': 'capabilities.html', 'toolbox': 'toolbox.html', 'download': '', 'webdriver': '', 'media': '', 'capture': 'capabilities.html', 'engine': 'wega-engine.html'}[suite]
        # Keep artifacts, use a new profile on every invocation, never the user's profile.
        profile = Path(tempfile.mkdtemp(prefix=f'{suite}-', dir=output))
        command = [str(Path(args.binary).resolve()), f'--{suite}-test', '--profile-dir', str(profile), f'http://127.0.0.1:{server.server_port}/{fixture}']
        environment = os.environ.copy()
        environment['WEGA_TEST_ALLOW_LOOPBACK']='1'
        if suite in ('engine','smoke'):
            command.insert(1,'--engine');command.insert(2,'wega')
            helper=Path(args.binary).resolve().parent/('wega-engine.exe' if os.name=='nt' else 'wega-engine')
            if not helper.is_file():helper=root/'target/debug'/helper.name
            environment.setdefault('WEGA_ENGINE_BINARY',str(helper))

        if suite in ('webdriver', 'media'):
            environment.update(WEGA_FIXTURE_URL=f'http://127.0.0.1:{server.server_port}', WEGA_TEST_OUTPUT=str(output))
            command = [sys.executable, str(root / 'tests' / ('webdriver_smoke.py' if suite == 'webdriver' else 'media_smoke.py')), str(Path(args.binary).resolve())]
        if suite == 'capture':
            command = [str(Path(args.binary).resolve()), '--profile-dir', str(profile), '--capture', str(profile / 'page.png'), f'http://127.0.0.1:{server.server_port}/capabilities.html']
        started = time.monotonic()
        try:
            with (profile / 'browser.log').open('w') as log:
                process = subprocess.Popen(command, stdout=log, stderr=log, cwd=root, env=environment, start_new_session=os.name != 'nt')
                try:
                    status = process.wait(timeout=280)
                except subprocess.TimeoutExpired:
                    if os.name != 'nt':
                        os.killpg(process.pid, signal.SIGKILL)
                    else:
                        subprocess.run(['taskkill', '/F', '/T', '/PID', str(process.pid)], check=False)
                    process.wait()
                    status = 124
            if status == 0 and suite == 'capture':
                capture = profile / 'page.png'
                report = profile / 'page.json'
                if not capture.exists() or not capture.read_bytes().startswith(b'\x89PNG\r\n\x1a\n') or not report.exists() or json.loads(report.read_text()).get('title') != 'Capability fixture':
                    status = 1
                    with (profile / 'browser.log').open('a') as log:
                        log.write('Capture did not produce a valid PNG and expected document report\n')
        finally:
            server.shutdown()
            server.server_close()
        result = [item for item in result if item['suite'] != suite]
        current_failed = current_failed or bool(status)
        result.append({'suite': suite, 'exit_code': status, 'seconds': round(time.monotonic() - started, 2), 'profile': str(profile)})
        (output / 'results.json').write_text(json.dumps(result, indent=2) + '\n')
        print(f'{suite}: {"PASS" if status == 0 else "FAIL"} — {profile}', flush=True)
        if status:
            print((profile / 'browser.log').read_text(), file=sys.stderr)
    return int(current_failed)


if __name__ == '__main__':
    sys.exit(main())
