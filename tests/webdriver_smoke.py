"""Real W3C HTTP smoke test against an isolated, short-lived native browser process."""
import base64
import json
import os
from pathlib import Path
import subprocess
import socket
import sys
import tempfile
import time
import urllib.error
import urllib.request

binary = str(Path(sys.argv[1] if len(sys.argv) > 1 else 'target/debug/wega').resolve())
profile = Path(tempfile.mkdtemp(prefix='wega-webdriver-', dir=os.environ.get('WEGA_TEST_OUTPUT')))
with socket.socket() as listener:
    listener.bind(('127.0.0.1', 0))
    port = listener.getsockname()[1]
base = f'http://127.0.0.1:{port}'

def request(method, path, body=None, expected=200):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base + path, data=data, method=method, headers={'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(req, timeout=36) as response:
            status, raw = response.status, response.read()
    except urllib.error.HTTPError as error:
        status, raw = error.code, error.read()
    result = json.loads(raw)
    assert status == expected, (method, path, status, result)
    return result['value']

with (profile / 'browser.log').open('w') as log:
    process = subprocess.Popen([binary, '--profile-dir', str(profile), '--webdriver-port', str(port), 'wega://newtab'], stdout=log, stderr=log)
    try:
        deadline = time.monotonic() + 20
        while True:
            try:
                assert request('GET', '/status')['ready']
                break
            except (OSError, AssertionError):
                if time.monotonic() > deadline or process.poll() is not None:
                    raise RuntimeError((profile / 'browser.log').read_text())
                time.sleep(0.2)
        assert request('POST', '/session', {'capabilities': {'alwaysMatch': {'browserName': 'firefox'}}}, 500)['error'] == 'session not created'
        session = request('POST', '/session', {'capabilities': {'alwaysMatch': {'browserName': 'wega'}}})
        assert session['capabilities']['browserName'] == 'wega'
        assert session['capabilities']['browserVersion'].startswith('0.1.0')
        sid = session['sessionId']
        root = f'/session/{sid}'
        assert request('POST', root + '/url', {'url': 'javascript://blocked'}, 400)['error'] == 'invalid argument'
        request('POST', root + '/url', {'url': 'wega://newtab'})
        assert request('GET', root + '/url') == 'wega://newtab'
        request('POST', root + '/url', {'url': os.environ.get('WEGA_FIXTURE_URL', 'http://127.0.0.1:8767') + '/capabilities.html'})
        title = request('GET', root + '/title')
        assert title == 'Capability fixture', title
        assert request('GET', root + '/url').endswith('/capabilities.html')
        assert 'needle' in request('GET', root + '/source')
        result = request('POST', root + '/execute/sync', {'script': 'return {sum:arguments[0]+arguments[1], title:document.title};', 'args': [20, 22]})
        assert result == {'sum': 42, 'title': 'Capability fixture'}, result
        def cache_result(script):
            request('POST', root + '/execute/sync', {'script': "globalThis.cacheProof=null;(async()=>{"+script+"})().then(value=>cacheProof={value},error=>cacheProof={error:String(error)});return true;", 'args': []})
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                result = request('POST', root + '/execute/sync', {'script': 'return globalThis.cacheProof;', 'args': []})
                if result is not None:
                    assert result == {'value': 'desktop-persisted'}, result
                    return
                time.sleep(0.05)
            raise TimeoutError('Desktop Cache API did not complete')
        cache_result("const cache=await caches.open('webdriver');await cache.put('/__cache-proof',new Response('desktop-persisted'));return await(await cache.match('/__cache-proof')).text();")
        # Leaving a website must detach its worker so native pages report their
        # own URL/title. Returning creates a new document with the same storage.
        request('POST', root + '/url', {'url': 'wega://newtab'})
        assert request('GET', root + '/url') == 'wega://newtab'
        assert request('GET', root + '/title') == 'New tab'
        request('POST', root + '/url', {'url': os.environ.get('WEGA_FIXTURE_URL', 'http://127.0.0.1:8767') + '/capabilities.html'})
        cache_result("return await(await caches.match('/__cache-proof')).text();")
        handle = request('GET', root + '/window')
        assert handle in request('GET', root + '/window/handles')
        request('POST', root + '/window', {'handle': handle})
        assert request('POST', root + '/window', {'handle': 'missing-tab'}, 404)['error'] == 'no such window'
        assert request('POST', root + '/execute/sync', {'script': 'return arguments[0]', 'args': [{'element-6066-11e4-a52e-4f735466cecf':'unsupported'}]}, 500)['error'] == 'unsupported operation'
        assert request('POST', root + '/execute/sync', {'script': 'throw new Error("test exception")', 'args': []}, 500)['error'] == 'javascript error'
        for script, expected in [('return undefined', None), ('return null', None), ('return true', True), ('return 42', 42), ('return "string"', 'string'), ('return [1,2]', [1,2])]:
            assert request('POST', root + '/execute/sync', {'script': script, 'args': []}) == expected
        fixture = os.environ.get('WEGA_FIXTURE_URL', 'http://127.0.0.1:8767')
        request('POST', root + '/url', {'url': fixture + '/second.html'})
        title = request('GET', root + '/title')
        assert title == 'Second Servo document', title
        print('WebDriver: back', flush=True)
        request('POST', root + '/back', {})
        title = request('GET', root + '/title')
        assert title == 'Capability fixture', title
        print('WebDriver: forward', flush=True)
        request('POST', root + '/forward', {})
        title = request('GET', root + '/title')
        assert title == 'Second Servo document', title
        print('WebDriver: refresh', flush=True)
        request('POST', root + '/refresh', {})
        title = request('GET', root + '/title')
        assert title == 'Second Servo document', title
        print('WebDriver: forward', flush=True)
        request('POST', root + '/forward', {})  # End-of-history is a successful no-op.
        assert request('GET', root + '/url').endswith('/second.html')
        png = base64.b64decode(request('GET', root + '/screenshot'))
        assert png.startswith(b'\x89PNG\r\n\x1a\n')
        (profile / 'webdriver-screenshot.png').write_bytes(png)
        assert request('POST', root + '/element', {'using': 'css selector', 'value': 'h1'}, 500)['error'] == 'unsupported operation'
        request('DELETE', root)
        print(f'WEBDRIVER TEST PASSED: W3C HTTP sessions, capability rejection, native-page transitions, persistent Cache API, engine navigation/back/forward/refresh, URL/title/source, JSON script arguments/results, window handles, viewport PNG, unsupported-command error and session deletion. Artifacts: {profile}', flush=True)
    finally:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
