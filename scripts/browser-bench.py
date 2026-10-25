#!/usr/bin/env python3
"""Isolated two-engine compatibility probes and process-tree resource measurements.

Standard library only. RSS is summed resident working set, not unique physical
memory: shared pages may be counted twice. No VM-heap figures are used as RSS.
"""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import platform
import signal
import socket
import statistics
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
PROBES = {
    "article": None,
    "dom": "var e=document.createElement('p');e.setAttribute('id','made');e.textContent='changed';document.body.appendChild(e);check(document.getElementById('made').textContent==='changed');",
    "timer": "setTimeout(function(){check(true);},100);",
    "events": "var b=document.getElementById('button');b.addEventListener('click',function(){check(true);});b.click();",
    "forms": "var f=new FormData(document.getElementById('form'));check(f.get('draft')==='unsaved text');",
    "storage": "localStorage.setItem('bench','saved');check(localStorage.getItem('bench')==='saved');localStorage.removeItem('bench');",
    "fetch": "fetch('/payload').then(function(r){return r.text();}).then(function(t){check(t==='network ok');}).catch(function(){check(false);});",
}
# These are bounded API probes, not claims of whole-site compatibility.
REQUIRED = {"servo": set(PROBES), "wega": {"article", "dom", "timer"}}
LIVE_SITES = ["https://example.com/", "https://en.wikipedia.org/wiki/Rust_(programming_language)",
              "https://developer.mozilla.org/en-US/", "https://github.com/", "https://news.ycombinator.com/"]


def fixture(name):
    script = PROBES[name]
    title = f"PASS:{name}" if script is None else f"PENDING:{name}"
    body = "".join(f"<p>Paragraph {i}: deterministic article text and <a href='/article'>a link</a>.</p>" for i in range(100))
    test = "" if script is None else (
        "<script>function check(ok){document.title=(ok?'PASS:':'FAIL:')+" + json.dumps(name) + ";}"
        "document.addEventListener('DOMContentLoaded',function(){try{" + script +
        "}catch(e){document.title='FAIL:' + " + json.dumps(name) + ";}});</script>")
    return (f"<!doctype html><meta charset='utf-8'><title>{title}</title>"
            "<style>body{max-width:800px;margin:24px auto;font:16px sans-serif}h1{color:#14765a}</style>"
            "<h1>Reference article</h1><button id='button'>Test event</button>"
            "<form id='form'><input name='draft' value='unsaved text'></form>" + body + test).encode()


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        name = self.path.split('?', 1)[0].strip('/')
        if name in PROBES:
            body, kind = fixture(name), 'text/html; charset=utf-8'
        elif name == 'payload':
            body, kind = b'network ok', 'text/plain'
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header('Content-Type', kind)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


def cpu_seconds(value):
    """Parse ps [[days-]hours:]minutes:seconds without locale assumptions."""
    days, sep, rest = value.partition('-')
    parts = [float(x) for x in (rest if sep else value).split(':')]
    total = 0.0
    for part in parts:
        total = total * 60 + part
    return total + (float(days) * 86400 if sep else 0)


def process_snapshot():
    if sys.platform.startswith('linux'):
        rows = {}
        ticks, page_size = os.sysconf('SC_CLK_TCK'), os.sysconf('SC_PAGE_SIZE')
        for path in Path('/proc').glob('[0-9]*/stat'):
            try:
                # comm can contain spaces and parentheses; split at its final ')'.
                fields = path.read_text().rsplit(')', 1)[1].split()
                rows[int(path.parent.name)] = (int(fields[1]), int(fields[21]) * page_size,
                                               (int(fields[11]) + int(fields[12])) / ticks)
            except (OSError, ValueError, IndexError):
                continue  # An unrelated process may exit during this snapshot.
        return rows
    if os.name == 'nt':
        command = ["powershell", "-NoProfile", "-Command",
                   "@(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,WorkingSetSize,KernelModeTime,UserModeTime) | ConvertTo-Json -Compress"]
        rows = json.loads(subprocess.check_output(command, text=True, timeout=15))
        return {int(p['ProcessId']): (int(p['ParentProcessId']), int(p['WorkingSetSize'] or 0),
                (int(p['KernelModeTime'] or 0) + int(p['UserModeTime'] or 0)) / 1e7) for p in rows}
    output = subprocess.check_output(['ps', '-axo', 'pid=,ppid=,rss=,time='], text=True, timeout=10)
    rows = [line.split() for line in output.splitlines() if line.strip()]
    return {int(pid): (int(parent), int(rss) * 1024, cpu_seconds(cpu)) for pid, parent, rss, cpu in rows}


def tree_sample(root, rows):
    members = {root} if root in rows else set()
    while True:
        expanded = members | {pid for pid, (parent, _, _) in rows.items() if parent in members}
        if expanded == members:
            break
        members = expanded
    return {'pids': sorted(members), 'rss_bytes': sum(rows[pid][1] for pid in members),
            'cpu_by_pid': {str(pid): rows[pid][2] for pid in members}}


class Sampler:
    def __init__(self, process):
        self.process = process
        self.start = time.monotonic()
        self.phase = 'startup'
        self.samples = []
        self.errors = []
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.run, daemon=True)

    def run(self):
        while not self.stop.is_set() and self.process.poll() is None:
            try:
                sample = tree_sample(self.process.pid, process_snapshot())
                if not sample['pids']:
                    raise RuntimeError('browser process missing from sample')
                sample.update(seconds=time.monotonic() - self.start, phase=self.phase)
                self.samples.append(sample)
            except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
                self.errors.append(str(error))
            self.stop.wait(0.25)

    def finish(self):
        self.stop.set()
        self.thread.join(timeout=20)
        return {'samples': self.samples, 'sampling_errors': self.errors,
                'peak_tree_rss_bytes': max((s['rss_bytes'] for s in self.samples), default=None)}


def terminate(process):
    if os.name == 'nt':
        if process.poll() is None:
            subprocess.run(['taskkill', '/F', '/T', '/PID', str(process.pid)], capture_output=True, timeout=20)
    else:
        # Also collect descendants left behind if the root crashed/exited first.
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        if os.name != 'nt':
            os.killpg(process.pid, signal.SIGKILL)
        else:
            process.kill()
        process.wait(timeout=10)


def request(base, method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base + path, data, {'Content-Type': 'application/json'}, method=method)
    with urllib.request.urlopen(req, timeout=3) as response:
        value = json.load(response)['value']
    if isinstance(value, dict) and 'error' in value:
        raise RuntimeError(str(value))
    return value


def wait_until(operation, timeout=40):
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        try:
            value = operation()
            if value:
                return value
        except (OSError, ValueError, RuntimeError) as error:
            last_error = str(error)
        time.sleep(0.1)
    raise TimeoutError(f'Browser readiness timed out: {last_error}')


def resource_run(binary, engine, url, tabs, idle, folder):
    folder.mkdir(parents=True)
    profile = folder / 'profile'
    profile.mkdir()
    # Existing profiles have no defaults dependency; serde fills unspecified settings.
    (profile / 'browser.json').write_text(json.dumps({'urls': [url] * tabs, 'active': 0,
        'policy': {'discard_after_minutes': 0}, 'engines': [engine.capitalize()] * tabs}))
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    base = f'http://127.0.0.1:{port}'
    command = [str(binary), '--engine', engine, '--profile-dir', str(profile), '--webdriver-port', str(port)]
    result = {'engine': engine, 'tabs': tabs, 'url': url, 'command': command, 'status': 'error'}
    with (folder / 'browser.log').open('w') as log:
        process = subprocess.Popen(command, stdout=log, stderr=log, start_new_session=os.name != 'nt')
        sampler = Sampler(process)
        sampler.thread.start()
        try:
            wait_until(lambda: request(base, 'GET', '/status')['ready'])
            session = request(base, 'POST', '/session', {'capabilities': {}})['sessionId']
            path = '/session/' + session
            handles = request(base, 'GET', path + '/window/handles')
            if len(handles) != tabs:
                raise RuntimeError(f'Expected {tabs} restored tabs, got {len(handles)}')
            for handle in handles:
                request(base, 'POST', path + '/window', {'handle': handle})
                wait_until(lambda: request(base, 'GET', '/status')['ready'])
                wait_until(lambda: request(base, 'GET', path + '/title') == 'PASS:article')
            result['all_tabs_ready_seconds'] = time.monotonic() - sampler.start
            sampler.phase = 'settle'
            time.sleep(2)
            sampler.phase = 'idle'
            deadline = time.monotonic() + idle
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError('Browser exited during idle measurement')
                time.sleep(0.1)
            result['status'] = 'measured'
        except (OSError, ValueError, RuntimeError, TimeoutError) as error:
            result['error'] = str(error)
        finally:
            result.update(sampler.finish())
            terminate(process)
    samples = [s for s in result['samples'] if s['phase'] == 'idle']
    if len(samples) >= 2:
        first, last = samples[0], samples[-1]
        # Count deltas for every observed PID, retaining terminated workers' last counters.
        observed = {}
        for sample in samples:
            for pid, cpu in sample['cpu_by_pid'].items():
                observed.setdefault(pid, [cpu, cpu])[1] = cpu
        result['idle_tree_cpu_percent'] = 100 * sum(max(0, end - start) for start, end in observed.values()) / (last['seconds'] - first['seconds'])
        result['idle_median_tree_rss_bytes'] = statistics.median(s['rss_bytes'] for s in samples)
    else:
        result['status'] = 'error'
        result['error'] = result.get('error', 'Insufficient idle samples')
    if result['sampling_errors']:
        result['status'] = 'error'
    return result


def compatibility_run(binary, engine, name, url, folder, required):
    folder.mkdir(parents=True)
    image = folder / 'page.png'
    command = [str(binary), '--engine', engine, '--profile-dir', str(folder / 'profile'), '--capture', str(image), url]
    result = {'engine': engine, 'case': name, 'url': url, 'required': required, 'status': 'error'}
    with (folder / 'browser.log').open('w') as log:
        process = subprocess.Popen(command, stdout=log, stderr=log, start_new_session=os.name != 'nt')
        try:
            status = process.wait(timeout=75)
            report = json.loads(image.with_suffix('.json').read_text())
            valid_png = image.read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
            result.update(exit_code=status, report=report)
            if status == 0 and valid_png:
                result['status'] = ('captured_unverified' if name == 'live' else
                                    'pass' if report.get('title') == f'PASS:{name}' else 'unsupported_or_failed')
        except (OSError, ValueError, subprocess.TimeoutExpired) as error:
            result['error'] = str(error)
        finally:
            terminate(process)
    return result


def write_summary(report, output):
    lines = ['# Browser baseline', '', report['label'], '', report['scope'], '',
             'Local probes exercise bounded APIs. Live captures require inspection and do not certify site functionality.', '',
             '| Engine | Probe | Result |', '| --- | --- | --- |']
    lines.extend(f'| {row["engine"]} | {row["case"]} | {row["status"]} |' for row in report['compatibility'])
    lines += ['', '| Engine | Tabs | Runs | Median idle RSS (MiB) | Max sampled RSS (MiB) | Median idle CPU (%) |',
              '| --- | --- | --- | --- | --- | --- |']
    groups = sorted({(r['engine'], r['tabs']) for r in report['resources']})
    for engine, tabs in groups:
        rows = [r for r in report['resources'] if (r['engine'], r['tabs']) == (engine, tabs) and r['status'] == 'measured']
        if rows:
            rss = statistics.median(r['idle_median_tree_rss_bytes'] for r in rows) / 1048576
            peak = max(r['peak_tree_rss_bytes'] for r in rows) / 1048576
            cpu = statistics.median(r['idle_tree_cpu_percent'] for r in rows)
            lines.append(f'| {engine} | {tabs} | {len(rows)} | {rss:.1f} | {peak:.1f} | {cpu:.1f} |')
    lines += ['', f'Complete: {report["complete"]}. Binary unchanged: {report["binary_unchanged"]}. Worker unchanged: {report["worker_unchanged"]}.',
              '', 'See results.json for hashes, startup readiness, raw process samples, failures and individual runs.', '']
    (output / 'summary.md').write_text('\n'.join(lines))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/wega')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/browser-bench')
    parser.add_argument('--engine', nargs='+', choices=['servo', 'wega'], default=['servo', 'wega'])
    parser.add_argument('--mode', choices=['all', 'compatibility', 'resources'], default='all')
    parser.add_argument('--label', default='unspecified build', help='Build provenance, for example published optimized release or local debug')
    parser.add_argument('--tabs', nargs='+', type=int, default=[1, 5, 10])
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--idle-seconds', type=float, default=10)
    parser.add_argument('--live', action='store_true', help='Capture reference public sites; load-only evidence, not functional certification')
    parser.add_argument('--live-url', action='append', default=[], help='Use a specific reference site (repeatable) instead of the default live list')
    args = parser.parse_args()
    if args.repeats < 1 or args.idle_seconds < 2 or any(n < 1 or n > 100 for n in args.tabs):
        parser.error('Require repeats >= 1, idle >= 2 seconds and 1..100 tabs')
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f'Missing browser: {binary}')
    # Never merge reports from different binaries, fixtures or invocations.
    args.output.mkdir(parents=True, exist_ok=False)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_port}'
    report = {'schema': 1, 'platform': platform.platform(), 'machine': platform.machine(),
              'label': args.label, 'sample_interval_seconds': 0.25,
              'idle_requested_seconds': args.idle_seconds,
              'fixture_sha256': hashlib.sha256(b''.join(fixture(name) for name in PROBES)).hexdigest(),
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'scope': 'sum of browser and descendant RSS; shared pages may be counted multiple times; CPU 100% = one core; fresh profile, OS caches uncontrolled',
              'complete': False, 'compatibility': [], 'resources': []}
    try:
        report['source_commit'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
        report['working_tree'] = subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True)
    except (OSError, subprocess.SubprocessError):
        report['source_commit'] = os.environ.get('GITHUB_SHA')
        report['working_tree'] = 'unavailable; use binary and fixture hashes'
    helper = binary.with_name('wega-engine.exe' if os.name == 'nt' else 'wega-engine')
    helper = Path(os.environ.get('WEGA_ENGINE_BINARY', str(helper)))
    report['worker_sha256'] = hashlib.sha256(helper.read_bytes()).hexdigest() if helper.is_file() else None
    try:
        if args.mode in ('all', 'compatibility'):
            for engine in args.engine:
                for name in PROBES:
                    result = compatibility_run(binary, engine, name, base + '/' + name,
                        args.output / f'compat-{engine}-{name}', name in REQUIRED[engine])
                    report['compatibility'].append(result)
                    print(f'{engine} {name}: {result["status"]}', flush=True)
                    (args.output / 'results.json').write_text(json.dumps(report, indent=2))
                if args.live or args.live_url:
                    for index, url in enumerate(args.live_url or LIVE_SITES):
                        report['compatibility'].append(compatibility_run(binary, engine, 'live', url,
                            args.output / f'live-{engine}-{index}', False))
        if args.mode in ('all', 'resources'):
            for repeat in range(args.repeats):
                # Alternate order to reduce systematic warm-cache bias.
                for engine in args.engine[::1 if repeat % 2 == 0 else -1]:
                    for tabs in args.tabs:
                        result = resource_run(binary, engine, base + '/article', tabs, args.idle_seconds,
                            args.output / f'resources-{repeat}-{engine}-{tabs}')
                        result['repeat'] = repeat
                        report['resources'].append(result)
                        print(f'{engine} {tabs} tabs repeat {repeat}: {result["status"]}', flush=True)
                        (args.output / 'results.json').write_text(json.dumps(report, indent=2))
        report['complete'] = True
    finally:
        server.shutdown()
        server.server_close()
        report['binary_unchanged'] = hashlib.sha256(binary.read_bytes()).hexdigest() == report['binary_sha256']
        report['worker_unchanged'] = (hashlib.sha256(helper.read_bytes()).hexdigest() if helper.is_file() else None) == report['worker_sha256']
        (args.output / 'results.json').write_text(json.dumps(report, indent=2) + '\n')
        write_summary(report, args.output)
    failed = any(r['required'] and r['status'] != 'pass' for r in report['compatibility'])
    failed |= any(r['status'] != 'measured' for r in report['resources'])
    failed |= not report['binary_unchanged'] or not report['worker_unchanged']
    return int(failed)


if __name__ == '__main__':
    raise SystemExit(main())
