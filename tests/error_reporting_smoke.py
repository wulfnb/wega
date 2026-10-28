#!/usr/bin/env python3
"""Native engines -> durable outbox -> optional real collector container."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import threading
import time


class Fixture(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/bad.css':
            content_type = 'text/css'
            body = b'p{color:invalid-color;no-such-property:42}'
        else:
            content_type = 'text/html'
            body = b'''<!doctype html><title>Error reporting fixture</title>
<link rel=stylesheet href=/bad.css><p></div>Diagnostic fixture
<script>console.warn('native-warning');console.error('native-console');
setTimeout(()=>{throw Error('native-timer')},50);throw Error('native-uncaught')</script>'''
        self.send_response(200)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path('target/debug/wega'))
    parser.add_argument('--output', type=Path, default=Path('target/error-reporting/native'))
    parser.add_argument('--collector-image', help='Also deliver queued reports to a disposable Docker collector')
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    container = None
    endpoint = None
    results = []

    def docker(*command):
        return subprocess.check_output(['docker', *command], text=True).strip()

    try:
        with tempfile.TemporaryDirectory(prefix='wega-native-collector-') as data:
            if args.collector_image:
                Path(data).chmod(0o777)
                container = 'wega-native-collector-' + secrets.token_hex(6)
                docker('run', '-d', '--name', container, '--read-only', '--cap-drop=ALL',
                       '--security-opt=no-new-privileges', '--memory=128m',
                       '-p', '127.0.0.1::7090', '-v', f'{data}:/data', args.collector_image)
                port = json.loads(docker('inspect', container))[0]['NetworkSettings']['Ports']['7090/tcp'][0]['HostPort']
                endpoint = f'http://127.0.0.1:{port}/v1/errors'
                deadline = time.monotonic() + 15
                while subprocess.run(['docker', 'exec', container, '/wega-error-collector', 'healthcheck'],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
                    assert time.monotonic() < deadline, 'collector health deadline'
                    time.sleep(0.2)
            for engine in ['wega', 'servo']:
                profile = Path(tempfile.mkdtemp(prefix=engine+'-', dir=output))
                outbox = profile / 'logs/error-outbox'

                def capture(name):
                    environment = os.environ.copy()
                    environment.pop('WEGA_ERROR_REPORT_URL', None)
                    command = [str(args.binary.resolve()), '--engine', engine, '--profile-dir', str(profile),
                               '--capture', str(profile / f'{name}.png'), f'http://127.0.0.1:{server.server_port}/']
                    with (profile / f'{name}.log').open('w') as log:
                        result = subprocess.run(command, env=environment, stdout=log, stderr=log, timeout=75)
                    assert result.returncode == 0, (engine, name, profile)
                    report = json.loads((profile / f'{name}.json').read_text())
                    assert report['engine'] == engine, report

                capture('queued')
                reports = [json.loads(p.read_text()) for p in outbox.glob('*.json')]
                categories = {r['diagnostic']['category'] for r in reports}
                assert {'html', 'css', 'javascript'} <= categories, (engine, categories, profile)
                assert any(r['diagnostic']['category'] == 'css'
                           and (r['diagnostic']['url'] or '').endswith('/bad.css')
                           and r['diagnostic']['line'] == 1 for r in reports), (engine, 'CSS URL/line', profile)
                for message in ['native-warning', 'native-console', 'native-uncaught', 'native-timer']:
                    assert any(message in r['diagnostic']['message'] for r in reports), (engine, message, profile)
                entry = {'engine': engine, 'profile': str(profile), 'queued': len(reports),
                         'categories': sorted(categories)}
                if endpoint:
                    public = json.loads((profile / 'error-reporting/enrollment.json').read_text())
                    docker('exec', container, '/wega-error-collector', 'enroll', public['client_id'], public['public_key'])
                    (profile / 'error-reporting.json').write_text(json.dumps({'endpoint': endpoint}))
                    capture('delivered')
                    rows = [json.loads(line) for line in docker('exec', container, '/wega-error-collector', 'export', '10000').splitlines()]
                    accepted = [r for r in rows if r['client_id'] == public['client_id']]
                    ids = {r['report']['id'] for r in accepted}
                    assert {r['id'] for r in reports} <= ids, (engine, 'queued IDs not delivered')
                    entry['accepted'] = len(accepted)
                    # Shutdown itself can emit final engine warnings after the
                    # last network attempt. Those stay durable for the next launch.
                    entry['pending_at_shutdown'] = len(list(outbox.glob('*.json')))
                results.append(entry)
                print(json.dumps(entry), flush=True)
        (output / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
    finally:
        if container:
            subprocess.run(['docker', 'rm', '-f', container], stdout=subprocess.DEVNULL, check=False)
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    main()
