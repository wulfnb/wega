#!/usr/bin/env python3
"""Verify every vendored engine source against a pinned archive plus exact patches."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / 'engines/vendor'


def verify(cache=None):
    manifest = json.loads((VENDOR / 'sources.json').read_text())
    with tempfile.TemporaryDirectory(prefix='wega-vendor-') as scratch:
        scratch = Path(scratch)
        for name, spec in manifest.items():
            archive_name = f'{name}-{spec["version"]}.crate'
            archive = (cache / archive_name).read_bytes() if cache else urllib.request.urlopen(
                f'https://static.crates.io/crates/{name}/{archive_name}', timeout=60).read()
            if hashlib.sha256(archive).hexdigest() != spec['sha256']:
                raise RuntimeError(f'{archive_name}: archive checksum mismatch')
            package = scratch / archive_name
            package.write_bytes(archive)
            with tarfile.open(package) as source:
                source.extractall(scratch, filter='data')
            pristine = scratch / f'{name}-{spec["version"]}'
            for patch in spec['patches']:
                subprocess.run(['patch', '--batch', '--forward', '-p1', '-d', str(pristine),
                                '-i', str(VENDOR / patch)], check=True, capture_output=True)
            ignored = {'.cargo-ok', '.cargo_vcs_info.json', *spec.get('extra_licenses', [])}
            def contents(directory):
                return {str(path.relative_to(directory)): path.read_bytes()
                        for path in directory.rglob('*') if path.is_file()
                        and path.name not in ignored}
            expected, actual = contents(pristine), contents(VENDOR / spec['directory'])
            different = sorted(key for key in expected.keys() | actual.keys()
                               if expected.get(key) != actual.get(key))
            if different:
                raise RuntimeError(f'{name}: undocumented vendor changes: {different}')
            print(f'{name} {spec["version"]}: pinned archive and documented patches match', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, help='Use a local Cargo archive cache without network')
    verify(parser.parse_args().cache)
