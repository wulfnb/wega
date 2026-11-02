#!/usr/bin/env python3
"""Reuse Wega's own MSVC Skia build across CI runs.

On MSVC the vendored skia-bindings patch builds Skia from source (about ten
minutes; see engines/vendor/README.md). rust-cache cannot keep that output:
vendored crates are path dependencies, which Cargo rebuilds from a fresh
checkout. rust-skia can export a finished build (BUILD_ARTIFACTSTAGINGDIRECTORY)
and install one from a file:// SKIA_BINARIES_URL. Archives are named with
rust-skia's own key (crate commit, target, features, CRT and debug), so a build
only ever installs binaries that were exported for exactly its configuration;
anything else falls back to the normal source build.

  prepare DIR       create DIR and point SKIA_BINARIES_URL at it (--github-env)
  build DIR -- CMD  run CMD; if DIR exists without archives, export and pack the
                    Skia build CMD performs, and report skia-exported=true
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[1]
VCS_INFO = ROOT / 'engines/vendor/blitz/skia-bindings/.cargo_vcs_info.json'
ARCHIVE = 'skia-binaries'


def url_template(directory):
    # rust-skia strips "file://" and reads the remainder, so Windows drive paths
    # must not gain a third slash ("file://D:/...").
    return 'file://' + Path(directory).resolve().as_posix() + f'/{ARCHIVE}-{{key}}.tar.gz'


def download_key(export_key, vcs_info=VCS_INFO):
    """Exports are keyed by the enclosing repository's commit; downloads by the crate's."""
    crate_hash = json.loads(Path(vcs_info).read_text())['git']['sha1'][:20]
    _, rest = export_key.split('-', 1)
    return f'{crate_hash}-{rest}'


def archives(directory):
    return sorted(Path(directory).glob(f'{ARCHIVE}-*.tar.gz'))


def pack(staging, directory, vcs_info=VCS_INFO):
    exported = Path(staging) / ARCHIVE
    key = download_key((exported / 'key.txt').read_text().strip(), vcs_info)
    destination = Path(directory) / f'{ARCHIVE}-{key}.tar.gz'
    with tarfile.open(destination, 'w:gz') as archive:
        archive.add(exported, arcname=ARCHIVE)
    shutil.rmtree(staging)
    return destination


def prepare(directory, github_env):
    Path(directory).mkdir(parents=True, exist_ok=True)
    found = archives(directory)
    print(f'MSVC Skia cache: {len(found)} archive(s) in {directory}')
    with open(github_env, 'a', encoding='utf-8') as output:
        output.write(f'SKIA_BINARIES_URL={url_template(directory)}\n')


def build(directory, command, environ=os.environ, run=subprocess.run, vcs_info=VCS_INFO):
    directory = Path(directory)
    environment = dict(environ)
    staging = directory / 'staging'
    export = directory.is_dir() and not archives(directory)
    if export:
        shutil.rmtree(staging, ignore_errors=True)
        environment['BUILD_ARTIFACTSTAGINGDIRECTORY'] = str(staging.resolve())
    status = run(command, env=environment).returncode
    if status or not export:
        return status
    if not (staging / ARCHIVE / 'key.txt').is_file():
        print('MSVC Skia cache: this build did not compile Skia; nothing to export')
        return 0
    archive = pack(staging, directory, vcs_info)
    print(f'MSVC Skia cache: exported {archive.name} ({archive.stat().st_size // 1048576} MiB)')
    if environ.get('GITHUB_OUTPUT'):
        with open(environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
            output.write('skia-exported=true\n')
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='action', required=True)
    prepare_parser = commands.add_parser('prepare')
    prepare_parser.add_argument('directory')
    prepare_parser.add_argument('--github-env', required=True)
    build_parser = commands.add_parser('build')
    build_parser.add_argument('directory')
    build_parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    if args.action == 'prepare':
        prepare(args.directory, args.github_env)
        return 0
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('build needs a command after --')
    return build(args.directory, command)


if __name__ == '__main__':
    sys.exit(main())
