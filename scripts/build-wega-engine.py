#!/usr/bin/env python3
"""Build the independent Blitz worker and atomically install it beside Wega.

Servo pins ICU 2.1, while Blitz beta.4 needs ICU 2.3. Keep both lockfiles and
target directories independent; neither engine changes the other's dependencies.
"""
import argparse
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess

ROOT=Path(__file__).resolve().parents[1]


def build(profile='release',target=None,jobs=None):
    base=Path(os.environ.get('CARGO_TARGET_DIR',ROOT/'target')).resolve()
    worker_target=base/'wega-engine'
    target=target or os.environ.get('CARGO_BUILD_TARGET')
    command=['cargo','build','--locked','--manifest-path',str(ROOT/'engines/Cargo.toml'),
             '--package','wega-engine','--bin','wega-engine','--target-dir',str(worker_target),'--profile',profile]
    if target:command+=['--target',target]
    if jobs:command+=['--jobs',str(jobs)]
    environment=None
    if os.environ.get('CI'):
        spec=importlib.util.spec_from_file_location("v8_prebuilt",ROOT/"scripts/v8_prebuilt.py")
        helper=importlib.util.module_from_spec(spec)
        spec.loader.exec_module(helper)
        environment=helper.prepare(target)
    subprocess.run(command,cwd=ROOT,check=True,env=environment)
    name='wega-engine.exe' if (target and 'windows' in target) or (not target and os.name=='nt') else 'wega-engine'
    folder='debug' if profile=='dev' else profile
    source=worker_target / target / folder / name if target else worker_target / folder / name
    destination=base / target / folder / name if target else base / folder / name
    if not source.is_file():raise FileNotFoundError(f'Worker build did not produce {source}')
    destination.parent.mkdir(parents=True,exist_ok=True)
    temporary=destination.with_suffix('.new')
    try:
        shutil.copy2(source,temporary)
        temporary.chmod(0o755)
        temporary.replace(destination)
    finally:temporary.unlink(missing_ok=True)
    print('Installed Wega engine: '+str(destination))
    return destination


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile',choices=['dev','release'],default='release')
    parser.add_argument('--debug',dest='profile',action='store_const',const='dev')
    parser.add_argument('--target')
    parser.add_argument('--jobs',type=int,default=int(os.environ.get('CARGO_BUILD_JOBS','4')))
    args=parser.parse_args()
    if args.jobs<1:parser.error('--jobs must be positive')
    build(args.profile,args.target,args.jobs)
