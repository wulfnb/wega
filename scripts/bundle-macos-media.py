#!/usr/bin/env python3
"""Copy the selected GStreamer plugins and Mach-O dependency closure into our app.

Only copies from the SDK/Homebrew; never edits or re-signs installed libraries.
Refuses GPL/patent-encumbered codec libraries (FFmpeg, x264, ...) and writes a
license notice for every bundled library with its Homebrew version and source.
"""
from collections import deque
from pathlib import Path
import ast
import json
import os
import re
import shutil
import subprocess
import sys

# FFmpeg (gst-libav), x264/x265 and FAAC are GPL or patent-encumbered. macOS
# decodes through Apple's system codecs (gstapplemedia) instead; see packaging/media/gstreamer_plugin_lists/README.md.
FORBIDDEN=re.compile(r'lib(x264|x265|av|swscale|swresample|postproc|faac|gstlibav)',re.IGNORECASE)
REPOSITORY='https://github.com/wulfnb/wega'

def homebrew_formula(source):
    match=re.search(r'/Cellar/([^/]+)/([^/]+)/',str(source))
    return match.groups() if match else None

def write_notices(path,manifest):
    """Record each library's origin, license and source location for LGPL compliance."""
    formulas=sorted({entry['formula'] for entry in manifest if 'formula' in entry})
    info={}
    if formulas:
        for formula in json.loads(subprocess.check_output(['brew','info','--json=v2',*formulas],text=True))['formulae']:
            info[formula['name']]=formula
    lines=['Wega for macOS: bundled media libraries','',
        'Contents/MacOS/lib contains these unmodified GStreamer plugins and their',
        'dependencies, copied from Homebrew. GStreamer, GLib and several others are',
        'licensed under the GNU LGPL; each is a separate dynamic library that you may',
        'replace with a compatible modified build (afterwards re-sign the app ad hoc:',
        'codesign --force --deep --sign - Wega.app). No FFmpeg, gst-libav, x264, x265',
        'or other GPL codec library is included; video/audio decoding uses Apple\'s',
        'system codecs through the gstapplemedia plugin.','']
    for entry in manifest:
        formula=info.get(entry.get('formula'),{})
        lines.append(entry['file'])
        if 'formula' in entry:
            lines.append(f"  Homebrew formula: {entry['formula']} {entry['version']}")
            lines.append(f"  License: {formula.get('license') or 'see upstream project'}")
            if formula.get('homepage'):lines.append(f"  Project: {formula['homepage']}")
            if formula.get('ruby_source_path'):lines.append(f"  Build recipe: https://github.com/Homebrew/homebrew-core/blob/HEAD/{formula['ruby_source_path']}")
            source_url=((formula.get('urls') or {}).get('stable') or {}).get('url')
            if source_url:lines.append(f"  Source (current formula release): {source_url}")
        else:
            lines.append(f"  Copied from: {entry['source']} (license: see upstream project)")
        lines.append('')
    lines+=['Written offer for source code','',
        'For at least three years after a build is distributed, anyone may obtain the',
        'complete corresponding source code of the LGPL-licensed libraries above, at',
        'the exact versions listed, for no more than the cost of providing it. Request',
        f'it by opening an issue at {REPOSITORY}/issues. Homebrew',
        'keeps the build recipe for every version in its formula history.','']
    path.write_text('\n'.join(lines))

def bundle(app, root=None):
    root=Path(root) if root is not None else Path(__file__).resolve().parents[1]
    app=Path(app).resolve()
    binary=app/'Contents/MacOS/wega'
    output=binary.parent/'lib'
    output.mkdir(parents=True,exist_ok=True)
    override=os.environ.get('WEGA_GSTREAMER_PLUGIN_DIR')
    plugin_dir=Path(override or subprocess.check_output(['pkg-config','--variable=pluginsdir','gstreamer-1.0'],text=True).strip())
    plugins=[]
    for name in ('common','macos'):
        content=(root/f'packaging/media/gstreamer_plugin_lists/{name}.rs.in').read_text()
        plugins.extend(ast.literal_eval(re.sub(r'//[^\n]*','',content)))
    queue=deque()
    seen={}
    manifest=[]
    def add(source):
        source=Path(source).resolve()
        if not source.is_file():raise RuntimeError(f'Missing media dependency: {source}')
        name=source.name
        if FORBIDDEN.match(name):raise RuntimeError(f'Refusing to bundle GPL/patent-encumbered codec library {name} from {source}')
        if name in seen:
            if seen[name]!=source:raise RuntimeError(f'Library name collision: {name}')
            return output/name
        seen[name]=source
        destination=output/name
        temporary=destination.with_name(destination.name+'.new')
        shutil.copy2(source,temporary)
        temporary.chmod(0o755)
        temporary.replace(destination)
        queue.append((source,destination))
        entry={'file':name,'source':str(source)}
        formula=homebrew_formula(source)
        if formula:entry['formula'],entry['version']=formula
        manifest.append(entry)
        return destination
    for plugin in plugins:add(plugin_dir/f'lib{plugin}.dylib')
    queue.append((binary,binary))
    while queue:
        original,destination=queue.popleft()
        dependencies=subprocess.check_output(['otool','-L',str(original)],text=True).splitlines()[1:]
        edits=[]
        for entry in dependencies:
            dependency=entry.strip().split(' (',1)[0]
            if dependency==str(original):continue
            if dependency.startswith(('/System/','/usr/lib/')):continue
            path=Path(dependency)
            if dependency.startswith('@loader_path/'):
                path=original.parent/dependency.removeprefix('@loader_path/')
            elif dependency.startswith('@rpath/'):
                candidate=plugin_dir.parent/dependency.removeprefix('@rpath/')
                if not candidate.exists():raise RuntimeError(f'Unresolved rpath {dependency} in {original}')
                path=candidate
            elif not path.is_absolute():raise RuntimeError(f'Unresolved dependency {dependency} in {original}')
            if path.resolve()==original.resolve():continue
            copied=add(path)
            target=('@loader_path/lib/' if destination==binary else '@loader_path/')+copied.name
            edits.extend(['-change',dependency,target])
        if destination!=binary:
            edits[0:0]=['-id','@rpath/'+destination.name]
        if edits:subprocess.run(['install_name_tool',*edits,str(destination)],check=True)
    for library in output.glob('*.dylib'):
        subprocess.run(['codesign','--force','--sign','-','--timestamp=none',str(library)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    # Rewriting dependencies invalidates the executable's existing ad-hoc signature.
    (app/'Contents/Resources').mkdir(exist_ok=True)
    (app/'Contents/Resources/media-libraries.json').write_text(json.dumps(manifest,indent=2))
    write_notices(app/'Contents/Resources/MEDIA-LIBRARIES.txt',manifest)
    # Seal the complete bundle only after every resource has been written.
    subprocess.run(['codesign','--force','--sign','-','--timestamp=none',str(app)],check=True)
    subprocess.run(['codesign','--verify','--deep','--strict',str(app)],check=True)
    print(f'Bundled {len(plugins)} GStreamer plugins and {len(seen)} native libraries.')


def main(args=None):
    args = sys.argv[1:] if args is None else args
    if len(args) != 1:
        raise SystemExit('Usage: scripts/bundle-macos-media.py APP')
    bundle(args[0])


if __name__ == '__main__':
    main()
