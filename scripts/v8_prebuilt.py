"""Use upstream V8 downloads first; pin and verify the repository mirror on failure."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
VERSION = '150.4.0'
TAG = f'deno-v8-mirror-{VERSION}'


def prepare(target=None, environment=None, opener=urllib.request.urlopen):
    environment = dict(os.environ if environment is None else environment)
    if environment.get('RUSTY_V8_MIRROR') or environment.get('V8_FROM_SOURCE'):
        return environment
    if not target:
        details = subprocess.check_output(['rustc', '-vV'], text=True)
        target = next(line.removeprefix('host: ') for line in details.splitlines() if line.startswith('host: '))
    checksums = {}
    for line in (ROOT / 'engines/vendor/deno/V8-PREBUILT-SHA256SUMS').read_text().splitlines():
        checksum, name = line.split()
        if name.endswith(f'{target}.a.gz') or name.endswith(f'{target}.lib.gz') or name.endswith(f'{target}.rs'):
            checksums[name] = checksum
    if len(checksums) != 2:
        raise ValueError(f'No pinned V8 prebuilt pair for {target}; use V8_FROM_SOURCE=1')
    upstream = f'https://github.com/denoland/rusty_v8/releases/download/v{VERSION}'
    try:
        for name in checksums:
            with opener(urllib.request.Request(f'{upstream}/{name}', method='HEAD'), timeout=30):
                pass
        return environment
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        print(f'V8 upstream download unavailable ({error}); verifying repository mirror.', flush=True)
    repository = environment.get('GITHUB_REPOSITORY', 'wulfnb/wega')
    mirror = f'https://github.com/{repository}/releases/download/{TAG}'
    directory = Path(tempfile.mkdtemp(prefix='wega-v8-mirror-'))
    assets = directory / f'v{VERSION}'
    assets.mkdir()
    for name, expected in checksums.items():
        path = assets / name
        digest = hashlib.sha256()
        with opener(f'{mirror}/{name}', timeout=120) as source, path.open('wb') as output:
            while chunk := source.read(1024 * 1024):
                digest.update(chunk)
                output.write(chunk)
        if digest.hexdigest() != expected:
            raise ValueError(f'V8 mirror checksum mismatch: {name}')
    environment['RUSTY_V8_MIRROR'] = str(directory)
    return environment


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target')
    parser.add_argument('--github-env', type=Path)
    args = parser.parse_args()
    result = prepare(args.target)
    if args.github_env and not os.environ.get('RUSTY_V8_MIRROR') and result.get('RUSTY_V8_MIRROR'):
        with args.github_env.open('a') as output:
            output.write(f"RUSTY_V8_MIRROR={result['RUSTY_V8_MIRROR']}\n")
