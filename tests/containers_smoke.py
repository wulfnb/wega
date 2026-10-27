#!/usr/bin/env python3
"""Real browser isolation, persistence and live container-window reuse.

All browser profiles and fixture servers are disposable. No personal profile is
read or changed. The capture mode exits through the normal engine shutdown path.
"""
import argparse
import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import urllib.parse
import urllib.request


class Fixture(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        query = urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query)
        account = query.get('account', [None])[0]
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Cache-Control', 'no-store')
        if account:
            self.send_header('Set-Cookie', f'account={account}; Path=/; Max-Age=3600; SameSite=Lax')
            self.send_header('Set-Cookie', f'httpAccount={account}; HttpOnly; Path=/; Max-Age=3600; SameSite=Lax')
        self.end_headers()
        self.wfile.write(('''<!doctype html><meta charset="utf-8"><h1>Container account fixture</h1><script>
const account = %s;
if (account) { localStorage.setItem('account', account); sessionStorage.setItem('account', account); }
document.title = JSON.stringify({cookie:document.cookie, local:localStorage.getItem('account'), session:sessionStorage.getItem('account'), serverCookie:%s});
</script>''' % (json.dumps(account), json.dumps(self.headers.get('Cookie', '')))).encode())


def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def request(base, method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base + path, data=data, method=method, headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(req, timeout=40) as response:
        return json.load(response)['value']


def wait(predicate, process=None):
    deadline = time.monotonic() + 35
    while True:
        try:
            result = predicate()
            if result:
                return result
        except (OSError, ValueError, AssertionError):
            pass
        if time.monotonic() > deadline or (process and process.poll() is not None):
            raise AssertionError('Native container check timed out')
        time.sleep(0.1)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', default='target/debug/wega')
    parser.add_argument('--output', default='target/containers-validation')
    parser.add_argument('--engines', nargs='+', choices=['servo', 'wega'], default=['servo', 'wega'])
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    run = Path(tempfile.mkdtemp(prefix='run-', dir=output))
    server = ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    origin = f'http://127.0.0.1:{server.server_port}/'
    evidence = {'run': str(run), 'complete': False, 'checks': []}

    def capture(root, identity, engine, suffix, account=None):
        image = run / f'{engine}-{identity}-{suffix}.png'
        url = origin + (f'?account={account}' if account else '')
        command = [binary, '--profile-dir', str(root), '--container', identity, '--engine', engine, '--capture', str(image), url]
        env = os.environ.copy()
        env.pop('GST_REGISTRY_1_0', None)
        with image.with_suffix('.log').open('w') as log:
            subprocess.run(command, stdout=log, stderr=log, timeout=80, check=True, env=env)
        assert image.read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
        report = json.loads(image.with_suffix('.json').read_text())
        assert report['engine'] == engine, report
        return json.loads(report['title'])

    try:
        for engine in args.engines:
            root = run / f'{engine}-root'
            root.mkdir()
            parent = b'{"urls":["wega://newtab"],"active":0,"history":[]}'
            (root / 'browser.json').write_bytes(parent)
            for identity in ['personal', 'work', 'shopping']:
                profile = root / 'containers' / identity
                profile.mkdir(parents=True)
                (profile / 'browser.json').write_text(json.dumps({'urls': [origin], 'policy': {'wega_fallback': False}}))
            for identity, account in [('personal', 'alice'), ('work', 'bob')]:
                initial = capture(root, identity, engine, 'set', account)
                assert initial['local'] == account and initial['session'] == account, initial
                assert f'account={account}' in initial['cookie'], initial
                assert 'httpAccount=' not in initial['cookie'], initial
                assert initial['serverCookie'] == '', initial
            for identity, account in [('personal', 'alice'), ('work', 'bob')]:
                restored = capture(root, identity, engine, 'restored')
                assert restored['local'] == account, restored
                assert restored['session'] is None, restored
                assert f'account={account}' in restored['cookie'], restored
                assert f'httpAccount={account}' in restored['serverCookie'], restored
                assert ('bob' if account == 'alice' else 'alice') not in json.dumps(restored), restored
            empty = capture(root, 'shopping', engine, 'empty')
            assert empty == {'cookie': '', 'local': None, 'session': None, 'serverCookie': ''}, empty
            assert (root / 'browser.json').read_bytes() == parent
            assert not (root / 'engine').exists() and not (root / 'wega-storage').exists()
            evidence['checks'].append(f'{engine}: isolated regular/HttpOnly cookies, persistent localStorage, fresh sessionStorage, empty third identity, unchanged parent profile')
            print(evidence['checks'][-1], flush=True)

        # Two simultaneously running Servo containers, then a second launch into
        # Work must add a tab to its existing window and retain its account.
        if 'servo' in args.engines:
            root = run / 'servo-root'
            processes = []
            try:
                sessions = {}
                for identity in ['personal', 'work']:
                    base = f'http://127.0.0.1:{port()}'
                    log = (run / f'live-{identity}.log').open('w')
                    process = subprocess.Popen([binary, '--profile-dir', str(root), '--container', identity, '--webdriver-port', base.rsplit(':', 1)[1], origin], stdout=log, stderr=log)
                    log.close()
                    processes.append(process)
                    wait(lambda: request(base, 'GET', '/status')['ready'], process)
                    sid = request(base, 'POST', '/session', {'capabilities': {}})['sessionId']
                    path = f'/session/{sid}'
                    wait(lambda: json.loads(request(base, 'GET', path + '/title')).get('local'), process)
                    sessions[identity] = (base, path)
                for identity, expected in [('personal', 'alice'), ('work', 'bob')]:
                    base, path = sessions[identity]
                    state = json.loads(request(base, 'GET', path + '/title'))
                    assert state['local'] == expected and f'httpAccount={expected}' in state['serverCookie'], state
                base, path = sessions['work']
                before = request(base, 'GET', path + '/window/handles')
                subprocess.run([binary, '--profile-dir', str(root), '--container', 'work', origin + '?copy=1'], timeout=15, check=True, capture_output=True)
                wait(lambda: len(request(base, 'GET', path + '/window/handles')) == len(before) + 1)
                title = wait(lambda: json.loads(request(base, 'GET', path + '/title')))
                assert title['local'] == 'bob' and 'httpAccount=bob' in title['serverCookie'], title
                personal = json.loads(request(sessions['personal'][0], 'GET', sessions['personal'][1] + '/title'))
                assert personal['local'] == 'alice' and 'httpAccount=alice' in personal['serverCookie'], personal
                screenshot = base64.b64decode(request(base, 'GET', path + '/screenshot'))
                assert screenshot.startswith(b'\x89PNG\r\n\x1a\n')
                (run / 'live-work.png').write_bytes(screenshot)
                evidence['checks'].append('simultaneous Servo accounts and authenticated existing-window URL handoff')
                print(evidence['checks'][-1], flush=True)
            finally:
                for process in processes:
                    process.terminate()
                for process in processes:
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
        evidence['complete'] = True
    finally:
        server.shutdown()
        server.server_close()
        (run / 'results.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(f'CONTAINERS PASSED: {run}', flush=True)


if __name__ == '__main__':
    main()
