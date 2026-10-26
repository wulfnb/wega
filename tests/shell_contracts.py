#!/usr/bin/env python3
"""Linux-container contracts for executable shell adapters, collected by kcov.

Run in the disposable test-tools image, never on a host: entrypoint tests write
/config and /run. Packaging uses a private checkout and explicit fake compiler /
media-packager ports; actual cp/chmod/tar and generated launchers are exercised.
"""
import argparse
import grp
import hashlib
import json
import os
import pwd
from pathlib import Path
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if not Path('/.dockerenv').exists() or os.geteuid() != 0:
        raise SystemExit('Run only as root in a disposable Docker test container')
    source, output = args.source.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for name in ['lcov.info', 'measurement.json']:
        (output / name).unlink(missing_ok=True)
    raw = output / 'raw'
    raw.mkdir(exist_ok=True)
    collection = Path(tempfile.mkdtemp(prefix='run-', dir=raw))
    with tempfile.TemporaryDirectory(prefix='wega-shell-') as temporary:
        work = Path(temporary)
        checkout = work / 'checkout'
        for name in ['docker', 'scripts', 'assets']:
            shutil.copytree(source / name, checkout / name)
        for name in ['LICENSE', 'NOTICE', 'about.toml', 'about.hbs']:
            shutil.copy2(source / name, checkout / name)
        fake = work / 'bin'
        fake.mkdir()
        calls = work / 'calls.jsonl'
        def program(name, body):
            path = fake / name
            path.write_text('#!/usr/bin/python3\nimport os,sys,json\nfrom pathlib import Path\n' + body)
            path.chmod(0o755)
        log = f"with open({str(calls)!r},'a') as f: f.write(json.dumps([Path(sys.argv[0]).name,sys.argv[1:]])+'\\n')\n"
        # cargo-about is a port too: `--version` probes it, `generate` writes the notices.
        program('cargo', log + "if sys.argv[1:3] == ['about', '--version'] and os.environ.get('FAKE_NO_CARGO_ABOUT'): sys.exit(1)\n"
                "if sys.argv[1:3] == ['about', 'generate'] and not os.environ.get('FAKE_EMPTY_NOTICES') and not (os.environ.get('FAKE_EMPTY_WORKER_NOTICES') and 'engines/wega/Cargo.toml' in ' '.join(sys.argv)): Path(sys.argv[sys.argv.index('--output-file') + 1]).write_text('<html>notices</html>')\n")
        program('git', "print('0123456789abcdef0123456789abcdef01234567')\n")
        program('python3', log + "if 'scripts/build-wega-engine.py' in sys.argv:\n for folder in ['debug','release']:\n  p=Path.cwd()/'target'/folder/'wega-engine';p.parent.mkdir(parents=True,exist_ok=True);p.write_text('engine worker');p.chmod(0o755)\n")
        program('codesign', log)
        program('strip', log)
        program('uname', "print('Darwin')\n")
        program('supervisord', log + "assert not any(k in os.environ for k in ['BROWSER_USER','BROWSER_PASSWORD','BROWSER_PASSWORD_FILE'])\n")
        program('supervisorctl', "print('\\n'.join(f'worker{i} RUNNING pid 1' for i in range(6)))\n")
        program('curl', "print('401' if '/auth/account' in sys.argv[-1] else '')\n")
        program('xdpyinfo', "p=Path('/tmp/wega-display-attempts');n=int(p.read_text()) if p.exists() else 0;p.write_text(str(n+1));sys.exit(0 if n else 1)\n")
        program('wega', log + f"with open({str(work / 'wega-env.jsonl')!r},'a') as f: f.write(json.dumps([os.environ.get('LP_NUM_THREADS'),os.environ.get('GALLIUM_DRIVER')])+'\\n')\n")
        base_env = os.environ.copy()
        base_env['PATH'] = str(fake) + ':' + base_env['PATH']
        runs = []
        def run(name, arguments=(), *, env=None, success=True):
            directory = collection / f'case-{len(runs):02}'
            command = ['kcov', f'--include-path={checkout}', str(directory), str(checkout / name), *arguments]
            result = subprocess.run(command, env=env or base_env, capture_output=True, text=True, timeout=90)
            assert (result.returncode == 0) == success, (name, arguments, result.returncode, result.stderr[-2000:])
            runs.append(directory)
            return result
        # Linux packaging: real filesystem/archive operations, isolated fake binary.
        run('scripts/package-linux.sh', [str(work / 'missing')], success=False)
        binary = checkout / 'target/release/wega'
        binary.parent.mkdir(parents=True)
        binary.write_text('#!/bin/sh\nprintf "launcher:%s\\n" "$1"\n')
        binary.chmod(0o755)
        run('scripts/package-linux.sh', success=False)
        worker=binary.parent/'wega-engine';worker.write_text('engine worker');worker.chmod(0o755)
        run('scripts/package-linux.sh')
        package = checkout / 'dist/wega-linux-x86_64'
        assert (package / 'libexec/wega-engine').read_text()=='engine worker'
        assert (package / 'share/applications/wega.desktop').read_text().startswith('[Desktop Entry]')
        assert subprocess.check_output([str(package / 'bin/wega'), 'literal argument'], text=True).strip() == 'launcher:literal argument'
        assert (package / 'LICENSE').read_bytes() == (checkout / 'LICENSE').read_bytes()
        assert (package / 'THIRD-PARTY-NOTICES.html').read_text() == '<html>notices</html>'
        assert 'source commit: 0123456789abcdef0123456789abcdef01234567' in (package / 'NOTICE').read_text()
        shutil.move(checkout / 'assets/icons/browser-icon-256.png', work / 'icon.png')
        run('scripts/package-linux.sh', [str(binary)])
        # Third-party notices: usage, missing tool, empty output and explicit commit.
        notices = work / 'notices'
        run('scripts/third-party-notices.sh', success=False)
        run('scripts/third-party-notices.sh', ['--features'], success=False)
        run('scripts/third-party-notices.sh', [str(notices)], env=dict(base_env, FAKE_NO_CARGO_ABOUT='1'), success=False)
        run('scripts/third-party-notices.sh', [str(notices)], env=dict(base_env, FAKE_EMPTY_NOTICES='1'), success=False)
        run('scripts/third-party-notices.sh', [str(notices), '--features', 'media'], env=dict(base_env, WEGA_SOURCE_COMMIT='feedface'))
        engine_manifest=checkout/'engines/wega/Cargo.toml';engine_manifest.parent.mkdir(parents=True);engine_manifest.write_text('[package]')
        run('scripts/third-party-notices.sh',[str(notices)],env=dict(base_env,WEGA_SOURCE_COMMIT='feedface'))
        assert (notices/'WEGA-ENGINE-THIRD-PARTY-NOTICES.html').is_file()
        run('scripts/third-party-notices.sh',[str(work/'empty-worker-notices')],env=dict(base_env,FAKE_EMPTY_WORKER_NOTICES='1'),success=False)
        notice = (notices / 'NOTICE').read_text()
        assert notice.startswith((checkout / 'NOTICE').read_text())
        assert 'source commit: feedface\nCorresponding source: https://github.com/wulfnb/wega/tree/feedface' in notice
        commands = [json.loads(line) for line in calls.read_text().splitlines()]
        assert any(name == 'cargo' and arguments[:5] == ['about', 'generate', '--locked', '--fail', '--config'] and arguments[-5:-3] == ['--features', 'media'] for name, arguments in commands)
        # Docker runtime-prebuilt binaries: usage, quick notices and full notices.
        run('scripts/build-linux-binaries.sh', [str(work / 'docker-out')], success=False)
        prebuilt = work / 'docker-out'
        run('scripts/build-linux-binaries.sh', [str(prebuilt), 'test-harness'])
        assert (prebuilt / 'wega').read_bytes() == binary.read_bytes()
        assert (prebuilt / 'wega-engine').read_text() == 'engine worker'
        assert (prebuilt / 'notices/NOTICE').read_bytes() == (checkout / 'NOTICE').read_bytes()
        run('scripts/build-linux-binaries.sh', [str(prebuilt), 'desktop', '--notices'], env=dict(base_env, BUILD_JOBS='2'))
        assert (prebuilt / 'notices/THIRD-PARTY-NOTICES.html').read_text() == '<html>notices</html>'
        commands = [json.loads(line) for line in calls.read_text().splitlines()]
        assert ['cargo', ['build', '--locked', '--features', 'test-harness', '--release', '--jobs', '4']] in commands
        assert ['cargo', ['build', '--locked', '--features', 'desktop', '--release', '--jobs', '2']] in commands
        assert ['strip', [str(prebuilt / 'wega'), str(prebuilt / 'wega-engine')]] in commands
        # macOS packager contracts substitute only external compiler/media ports.
        linux_env = dict(base_env, PATH='/usr/bin:/bin')
        run('scripts/package-macos.sh', env=linux_env, success=False)
        run('scripts/package-macos.sh', ['--invalid'], success=False)
        run('scripts/package-macos.sh', ['--no-media'])
        debug = checkout / 'target/debug/wega'
        debug.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(binary, debug)
        run('scripts/package-macos.sh', ['--debug', '--media'])
        assert (checkout / 'target/Wega.app/Contents/MacOS/wega').read_bytes() == debug.read_bytes()
        assert 'io.github.wulfnb.wega' in (checkout / 'target/Wega.app/Contents/Info.plist').read_text()
        assert (checkout / 'target/Wega.app/Contents/Resources/THIRD-PARTY-NOTICES.html').is_file()
        commands = [json.loads(line) for line in calls.read_text().splitlines()]
        assert ['cargo', ['build', '--locked', '--release', '--no-default-features', '--features', 'brotli-streams']] in commands
        assert ['cargo', ['build', '--locked', '--features', 'media']] in commands
        assert any(name == 'python3' and arguments[0] == 'scripts/bundle-macos-media.py' for name, arguments in commands)
        # The gateway bootstrap is already tested against real encryption/storage.
        # Here its process boundary is recorded, while shell credential scrubbing,
        # directory modes and real self-signed TLS generation are checked.
        env = dict(base_env, BROWSER_RESOLUTION='1280x800', BROWSER_USER='fixture-user', BROWSER_PASSWORD='fixture-secret', BROWSER_PASSWORD_FILE='/tmp/fixture-password')
        Path('/config/auth').mkdir(parents=True, exist_ok=True)
        # Reproduce the browser-owned, private tmpfs used by compose.quality.yaml.
        Path('/config').chmod(0o700)
        run('docker/entrypoint.sh', env=env)
        assert (Path('/config').stat().st_mode & 0o777) == 0o711
        for private in ['auth', 'browser', 'Downloads', '.cache']:
            assert (Path('/config', private).stat().st_mode & 0o777) == 0o700
        subprocess.run(['/usr/sbin/gosu', 'wega-auth', '/bin/sh', '-c',
                        'test -x /config && test -r /config/auth && '
                        'test ! -r /config/browser && test ! -r /config/Downloads'], check=True)
        assert (Path('/config/tls/server.key').stat().st_mode & 0o777) == 0o600
        assert (Path('/run/wega/browser').stat().st_mode & 0o777) == 0o700
        run('docker/entrypoint.sh', ['/bin/sh', '-c', 'test -z "${BROWSER_USER+x}${BROWSER_PASSWORD+x}${BROWSER_PASSWORD_FILE+x}"'], env=env)
        for resolution in ['', 'bad', '800x600x2', 'x600', '800x', '799x600', '800x599', '3841x600', '800x2161']:
            run('docker/entrypoint.sh', env=dict(env, BROWSER_RESOLUTION=resolution), success=False)
        run('docker/entrypoint.sh', ['healthcheck'], env=env)
        # TLS lifecycle with real openssl: unchanged settings keep the certificate,
        # SAN changes / expiry / legacy RSA regenerate it, user files win.
        tls = Path('/config/tls')
        crt, key, marker = tls / 'server.crt', tls / 'server.key', tls / 'generated.conf'
        default_settings = 'v2 ec-p256 365d DNS:localhost,IP:127.0.0.1'
        def openssl(*arguments):
            return subprocess.run(['openssl', *arguments], check=True, capture_output=True, text=True).stdout
        def certificate(key_path, crt_path, subject, *, days=365, algorithm=('-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:prime256v1')):
            openssl('req', '-x509', *algorithm, '-nodes', '-days', str(days), '-subj', subject, '-keyout', str(key_path), '-out', str(crt_path))
        def subject():
            return openssl('x509', '-noout', '-subject', '-in', str(crt)).strip()
        assert marker.read_text() == default_settings + '\n' and subject() == 'subject=CN=Wega'
        generated = crt.read_bytes()
        run('docker/entrypoint.sh', ['renew-tls'], env=env)
        assert crt.read_bytes() == generated, 'unchanged valid certificate must be kept'
        for sans in ['bad/name', 'DNS:', 'host_name', 'IP:10.0.0.1;rm']:
            result = run('docker/entrypoint.sh', ['renew-tls'], env=dict(env, BROWSER_TLS_SANS=sans), success=False)
            assert 'BROWSER_TLS_SANS: invalid entry' in result.stderr, result.stderr
            assert crt.read_bytes() == generated and marker.read_text() == default_settings + '\n'
        sans_env = dict(env, BROWSER_TLS_SANS=' wega.lan ,,DNS:proxy.example,10.0.0.5,::1,\t*.example.com')
        run('docker/entrypoint.sh', ['renew-tls'], env=sans_env)
        assert marker.read_text() == default_settings + ',DNS:wega.lan,DNS:proxy.example,IP:10.0.0.5,IP:::1,DNS:*.example.com\n'
        names = openssl('x509', '-noout', '-ext', 'subjectAltName', '-in', str(crt))
        for name in ['DNS:localhost', 'DNS:wega.lan', 'DNS:proxy.example', 'IP Address:10.0.0.5', 'DNS:*.example.com']:
            assert name in names, names
        assert crt.read_bytes() != generated and (key.stat().st_mode & 0o777) == 0o600
        assert key.stat().st_uid == pwd.getpwnam('browser').pw_uid and not list(tls.glob('.*.new'))
        generated = crt.read_bytes()
        run('docker/entrypoint.sh', ['renew-tls'], env=sans_env)
        assert crt.read_bytes() == generated, 'same SAN settings must keep the certificate'
        # Expiring within 30 days (matching settings) is renewed by renew-tls.
        certificate(key, crt, '/CN=Wega', days=1)
        marker.write_text(default_settings + '\n')
        expiring = crt.read_bytes()
        run('docker/entrypoint.sh', ['renew-tls'], env=env)
        assert crt.read_bytes() != expiring and marker.read_text() == default_settings + '\n'
        openssl('x509', '-checkend', '2592000', '-noout', '-in', str(crt))
        # Legacy self-signed RSA localhost certificate (no marker) is migrated.
        marker.unlink()
        certificate(key, crt, '/CN=localhost', days=3650, algorithm=('-newkey', 'rsa:2048'))
        run('docker/entrypoint.sh', env=env)
        assert subject() == 'subject=CN=Wega' and marker.read_text() == default_settings + '\n'
        assert 'prime256v1' in openssl('pkey', '-in', str(key), '-text_pub', '-noout'), 'legacy RSA key must be replaced by EC P-256'
        # Mounted certificate files: both required and readable, copied, marker dropped.
        mounted = work / 'mounted-tls'
        mounted.mkdir()
        certificate(mounted / 'key.pem', mounted / 'cert.pem', '/CN=mounted.example')
        for partial in [dict(WEGA_TLS_CERT_FILE=str(mounted / 'cert.pem')), dict(WEGA_TLS_KEY_FILE=str(mounted / 'key.pem')),
                        dict(WEGA_TLS_CERT_FILE=str(mounted / 'cert.pem'), WEGA_TLS_KEY_FILE=str(mounted / 'missing.pem'))]:
            result = run('docker/entrypoint.sh', ['renew-tls'], env=dict(env, **partial), success=False)
            assert 'WEGA_TLS_CERT_FILE and WEGA_TLS_KEY_FILE must both name readable files' in result.stderr, result.stderr
        assert marker.exists() and subject() == 'subject=CN=Wega'
        mounted_env = dict(env, WEGA_TLS_CERT_FILE=str(mounted / 'cert.pem'), WEGA_TLS_KEY_FILE=str(mounted / 'key.pem'))
        run('docker/entrypoint.sh', ['renew-tls'], env=mounted_env)
        assert crt.read_bytes() == (mounted / 'cert.pem').read_bytes() and key.read_bytes() == (mounted / 'key.pem').read_bytes()
        assert (crt.stat().st_mode & 0o777) == 0o644 and (key.stat().st_mode & 0o777) == 0o600 and not marker.exists()
        assert crt.stat().st_uid == pwd.getpwnam('browser').pw_uid
        os.utime(crt, ns=(1, 1)); os.utime(key, ns=(1, 1))
        run('docker/entrypoint.sh', ['renew-tls'], env=mounted_env)
        assert (crt.stat().st_mtime_ns, key.stat().st_mtime_ns) == (1, 1), 'identical mounted files must not be reinstalled'
        # A certificate the user placed in the volume (no marker, not legacy) is never replaced,
        # not even when SAN settings are requested.
        placed = crt.read_bytes()
        run('docker/entrypoint.sh', ['renew-tls'], env=sans_env)
        assert crt.read_bytes() == placed and not marker.exists() and subject() == 'subject=CN=mounted.example'
        # Trusted reverse proxies render the nginx real_ip include; invalid values abort.
        realip = Path('/run/wega/browser/nginx-realip.conf')
        run('docker/entrypoint.sh', env=dict(env, WEGA_TRUSTED_PROXIES='10.0.0.0/8, unix:,2001:DB8::/32,192.168.1.1'))
        assert realip.read_text() == ('set_real_ip_from 10.0.0.0/8;\nset_real_ip_from unix:;\nset_real_ip_from 2001:DB8::/32;\n'
                                      'set_real_ip_from 192.168.1.1;\nreal_ip_header X-Forwarded-For;\nreal_ip_recursive on;\n')
        assert (realip.stat().st_mode & 0o777) == 0o644 and not Path(str(realip) + '.new').exists()
        for proxies in ['proxy.example', '10.0.0.1,unix:/tmp/x', '10.0.0.1;']:
            result = run('docker/entrypoint.sh', env=dict(env, WEGA_TRUSTED_PROXIES=proxies), success=False)
            assert 'WEGA_TRUSTED_PROXIES: invalid address' in result.stderr, result.stderr
            assert not realip.exists(), 'a rejected proxy list must not leave a stale include active'
        run('docker/entrypoint.sh', env=dict(env, WEGA_TRUSTED_PROXIES='10.0.0.1'))
        assert realip.exists()
        run('docker/entrypoint.sh', env=env)
        assert not realip.exists(), 'unsetting WEGA_TRUSTED_PROXIES must remove the include'
        # An optional plain-HTTP port renders a listen include; invalid ports abort.
        listener = Path('/run/wega/browser/server-http.conf')
        result = run('docker/entrypoint.sh', env=dict(env, WEGA_HTTP_PORT='7080'))
        assert listener.read_text() == 'listen 7080;\n' and (listener.stat().st_mode & 0o777) == 0o644
        assert not Path(str(listener) + '.new').exists()
        assert 'expose it only to a TLS-terminating reverse proxy' in result.stderr, result.stderr
        assert 'without WEGA_TRUSTED_PROXIES' in result.stderr, result.stderr
        result = run('docker/entrypoint.sh', env=dict(env, WEGA_HTTP_PORT='7080', WEGA_TRUSTED_PROXIES='10.0.0.0/24'))
        assert 'without WEGA_TRUSTED_PROXIES' not in result.stderr, result.stderr
        invalid, out_of_range = 'WEGA_HTTP_PORT: invalid port', 'WEGA_HTTP_PORT must be 1024-65535'
        for port, message in [('http', invalid), ('-1', invalid), ('07080', invalid), ('80', out_of_range), ('1023', out_of_range),
                              ('7443', out_of_range), ('65536', out_of_range), ('99999999999999999999', out_of_range)]:
            result = run('docker/entrypoint.sh', env=dict(env, WEGA_HTTP_PORT=port), success=False)
            assert message in result.stderr, (port, result.stderr)
            assert not listener.exists(), 'a rejected port must not leave a stale listener active'
        run('docker/entrypoint.sh', env=dict(env, WEGA_HTTP_PORT='65535'))
        assert listener.read_text() == 'listen 65535;\n'
        run('docker/entrypoint.sh', env=env)
        assert not listener.exists(), 'unsetting WEGA_HTTP_PORT must remove the listener'
        # Legacy uploads move from the credential directory; existing names win.
        legacy = Path('/config/auth/uploads')
        (legacy / 'folder').mkdir(parents=True)
        (legacy / 'a.txt').write_text('legacy a')
        (legacy / 'dup.txt').write_text('legacy dup')
        (legacy / 'folder/inner.txt').write_text('inner')
        Path('/config/uploads').mkdir(parents=True, exist_ok=True)
        Path('/config/uploads/dup.txt').write_text('current dup')
        run('docker/entrypoint.sh', env=env)
        uploads = Path('/config/uploads')
        assert not legacy.exists()
        assert (uploads / 'a.txt').read_text() == 'legacy a' and (uploads / 'dup.txt').read_text() == 'current dup'
        assert (uploads / 'folder/inner.txt').read_text() == 'inner'
        owner, group = pwd.getpwnam('wega-auth').pw_uid, grp.getgrnam('browser').gr_gid
        for path in [uploads, uploads / 'folder']:
            assert (path.stat().st_mode & 0o7777) == 0o2750 and (path.stat().st_uid, path.stat().st_gid) == (owner, group), path
        for path in [uploads / 'a.txt', uploads / 'dup.txt', uploads / 'folder/inner.txt']:
            assert (path.stat().st_mode & 0o7777) == 0o640 and (path.stat().st_uid, path.stat().st_gid) == (owner, group), path
        legacy.mkdir()
        run('docker/entrypoint.sh', env=env)
        assert not legacy.exists() and sorted(p.name for p in uploads.iterdir()) == ['a.txt', 'dup.txt', 'folder']
        Path('/config/browser').mkdir(parents=True, exist_ok=True)
        profile = Path('/config/browser/browser.json')
        profile.unlink(missing_ok=True)
        env = dict(base_env, DISPLAY=':99', BROWSER_START_URL='https://example.com/with literal space')
        run('docker/start-browser.sh', env=env)
        profile.write_text('{}')
        run('docker/start-browser.sh', env=env)
        run('docker/start-browser.sh', env=dict(env, WEGA_RENDER_THREADS='6', WEGA_GALLIUM_DRIVER='softpipe'))
        commands = [json.loads(line) for line in calls.read_text().splitlines()]
        assert ['wega', ['--profile-dir', '/config/browser', '--', env['BROWSER_START_URL']]] in commands
        assert ['wega', ['--profile-dir', '/config/browser']] in commands
        # llvmpipe rasterizer threads are bounded by default and overridable.
        renderers = [json.loads(line) for line in (work / 'wega-env.jsonl').read_text().splitlines()]
        assert renderers == [['2', 'llvmpipe'], ['2', 'llvmpipe'], ['6', 'softpipe']], renderers
        # Merge exact collector counters, never infer hits from source text.
        merged = collection / 'merged'
        subprocess.run(['kcov', '--merge', str(merged), *map(str, runs)], check=True, capture_output=True, timeout=60)
        samples = {}
        for report in merged.rglob('cobertura.xml'):
            for item in ET.parse(report).findall('.//class'):
                path = Path(item.attrib['filename'])
                if not path.is_absolute():
                    path = checkout / path
                try:
                    name = path.relative_to(checkout).as_posix()
                except ValueError:
                    continue
                if name not in ['docker/entrypoint.sh', 'docker/start-browser.sh', 'scripts/package-linux.sh', 'scripts/package-macos.sh', 'scripts/third-party-notices.sh']:
                    continue
                counters = samples.setdefault(name, {})
                for line in item.findall('./lines/line'):
                    number, hits = int(line.attrib['number']), int(line.attrib['hits'])
                    counters[number] = max(counters.get(number, 0), hits)
        assert len(samples) == 5, f'Incomplete shell collector output: {list(samples)}'
        lcov = []
        hashes = {}
        for name, counters in sorted(samples.items()):
            hashes[name] = hashlib.sha256((source / name).read_bytes()).hexdigest()
            lcov += [f'SF:{name}', *[f'DA:{line},{hits}' for line, hits in sorted(counters.items())], f'LF:{len(counters)}', f'LH:{sum(hit > 0 for hit in counters.values())}', 'end_of_record']
            print(f'{name}: {sum(hit > 0 for hit in counters.values())}/{len(counters)} lines; missing {[line for line, hit in counters.items() if hit == 0]}')
        (output / 'lcov.info').write_text('\n'.join(lcov) + '\n')
        (output / 'measurement.json').write_text(json.dumps({'source_hashes': hashes, 'tests_passed': True, 'collector': subprocess.check_output(['kcov', '--version'], text=True).strip(), 'scope': 'shell process-boundary contracts in disposable Linux container'}, indent=2) + '\n')
        print('Shell process-boundary contracts passed')

if __name__ == '__main__':
    main()
