#!/usr/bin/env python3
"""Exercise the static container over real HTTP. Requires Docker and cryptography."""
import argparse
import hashlib
import json
import secrets
import subprocess
import tempfile
import time
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, build_opener, ProxyHandler

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--image', default='wega-error-collector:local')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    name = 'wega-collector-test-' + secrets.token_hex(6)
    checks = []
    http = build_opener(ProxyHandler({}))

    def docker(*command):
        return subprocess.check_output(['docker', *command], text=True).strip()

    with tempfile.TemporaryDirectory(prefix=name) as directory:
        # Disposable fixture only; production storage is 0700, owned by UID 65532.
        Path(directory).chmod(0o777)
        try:
            docker('run', '-d', '--name', name, '--read-only', '--cap-drop=ALL',
                   '--security-opt=no-new-privileges', '--memory=128m', '--cpus=0.5',
                   '-p', '127.0.0.1::7090', '-v', f'{directory}:/data', args.image)
            port = json.loads(docker('inspect', name))[0]['NetworkSettings']['Ports']['7090/tcp'][0]['HostPort']
            base = 'http://127.0.0.1:' + port

            def request(body=None, headers=None, path='/v1/errors'):
                req = Request(base + path, data=body, headers=headers or {})
                try:
                    with http.open(req, timeout=5) as response:
                        return response.status
                except HTTPError as error:
                    return error.code

            def health():
                deadline = time.monotonic() + 10
                while True:
                    try:
                        if request(path='/healthz') == 200:
                            break
                    except OSError:
                        pass
                    if time.monotonic() > deadline:
                        raise AssertionError('collector did not become healthy')
                    time.sleep(0.1)
                docker('exec', name, '/wega-error-collector', 'healthcheck')

            def check(label, actual, expected):
                assert actual == expected, (label, actual, expected)
                checks.append({'check': label, 'result': actual})

            health()
            check('unsigned request', request(b'{}'), 401)
            key = Ed25519PrivateKey.generate()
            client = secrets.token_hex(16)

            def signed(body, nonce=None, timestamp=None, signing_key=None):
                nonce = nonce or secrets.token_hex(16)
                timestamp = str(timestamp or int(time.time()))
                canonical = '\n'.join(['wega-errors-v1', 'POST', '/v1/errors', client,
                                       timestamp, nonce, hashlib.sha256(body).hexdigest()])
                return {'Content-Type': 'application/json', 'X-Wega-Client': client,
                        'X-Wega-Timestamp': timestamp, 'X-Wega-Nonce': nonce,
                        'X-Wega-Signature': (signing_key or key).sign(canonical.encode()).hex()}

            bodies = []
            for category in ['html', 'css', 'javascript']:
                bodies.append(json.dumps({
                    'schema_version': 1, 'id': secrets.token_hex(16),
                    'occurred_at': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                    'application': 'Wega', 'version': 'container-test', 'platform': 'test',
                    'source': 'smoke', 'diagnostic': {
                        'category': category, 'severity': 'error', 'message': 'exception\nstack λ',
                        'url': 'https://example.com/', 'line': 1, 'column': 2, 'truncated': False,
                    }}, separators=(',', ':')).encode())
            check('unknown client', request(bodies[0], signed(bodies[0])), 403)
            docker('exec', name, '/wega-error-collector', 'enroll', client,
                   key.public_key().public_bytes_raw().hex())
            first_headers = signed(bodies[0])
            check('HTML acceptance', request(bodies[0], first_headers), 202)
            check('CSS acceptance', request(bodies[1], signed(bodies[1])), 202)
            check('JS acceptance', request(bodies[2], signed(bodies[2])), 202)
            check('replay', request(bodies[0], first_headers), 409)
            check('retry', request(bodies[0], signed(bodies[0])), 200)
            check('tampering', request(bodies[1], signed(bodies[0])), 403)
            check('forged key', request(bodies[0], signed(bodies[0], signing_key=Ed25519PrivateKey.generate())), 403)
            check('stale timestamp', request(bodies[0], signed(bodies[0], timestamp=int(time.time())-1000)), 401)
            changed = bodies[0].replace(b'exception', b'changed')
            check('report ID conflict', request(changed, signed(changed)), 409)
            docker('restart', name)
            port = json.loads(docker('inspect', name))[0]['NetworkSettings']['Ports']['7090/tcp'][0]['HostPort']
            base = 'http://127.0.0.1:' + port
            health()
            check('replay survives restart', request(bodies[0], first_headers), 409)
            check('retry survives restart', request(bodies[0], signed(bodies[0])), 200)
            exported = docker('exec', name, '/wega-error-collector', 'export', '100')
            rows = [json.loads(row) for row in exported.splitlines()]
            check('exactly three persistent reports', len(rows), 3)
            assert all(row['client_id'] == client for row in rows)
            assert {row['report']['diagnostic']['category'] for row in rows} == {'html', 'css', 'javascript'}
            docker('exec', name, '/wega-error-collector', 'revoke', client)
            check('revocation', request(bodies[0], signed(bodies[0])), 403)
            image = json.loads(docker('image', 'inspect', args.image))[0]
            result = {'image': image['Id'], 'architecture': image['Architecture'],
                      'image_bytes': image['Size'], 'checks': checks}
            if args.output:
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(json.dumps(result, indent=2) + '\n')
            print(json.dumps(result, indent=2))
        except BaseException:
            subprocess.run(['docker', 'logs', '--tail', '30', name], check=False)
            raise
        finally:
            subprocess.run(['docker', 'rm', '-f', name], stdout=subprocess.DEVNULL, check=False)


if __name__ == '__main__':
    main()
