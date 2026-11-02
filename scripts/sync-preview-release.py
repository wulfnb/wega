#!/usr/bin/env python3
"""Keep the moving preview's source tag and assets aligned after publication."""
import argparse
import json
import re
import subprocess


OBSOLETE = {
    'Lotse-Linux-x86_64.tar.gz',
    'Lotse-macOS-arm64.dmg',
    'Lotse-macOS-arm64.zip',
    'Lotse-Windows-x64.zip',
}


def synchronize(repository, tag, sha):
    if tag != 'test-preview':
        return  # Version and per-commit tags must never move.
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('Invalid GitHub repository')
    if not re.fullmatch(r'[0-9a-f]{40}', sha):
        raise ValueError('Expected a complete commit SHA')
    prefix = f'repos/{repository}'
    # Updating target_commitish on a release does not move an existing Git tag.
    subprocess.run(
        ['gh', 'api', '--method', 'PATCH', f'{prefix}/git/refs/tags/{tag}', '--input', '-'],
        input=json.dumps({'sha': sha, 'force': True}), text=True, check=True,
    )
    release = json.loads(subprocess.check_output(
        ['gh', 'api', f'{prefix}/releases/tags/{tag}'], text=True,
    ))
    for asset in release['assets']:
        if asset['name'] in OBSOLETE:
            subprocess.run(
                ['gh', 'api', '--method', 'DELETE', f"{prefix}/releases/assets/{int(asset['id'])}"],
                check=True,
            )


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository', required=True)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--sha', required=True)
    args = parser.parse_args(argv)
    synchronize(args.repository, args.tag, args.sha)


if __name__ == '__main__':
    main()
