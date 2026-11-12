#!/usr/bin/env python3
"""Bundle selected MSVC GStreamer plugins and their exact x64 PE import closure.

Exclude system DLLs and FFmpeg/GPL codec libraries. Fail on missing imports,
wrong architectures, and ambiguous SDK names instead of publishing a partial app.
"""
import argparse
from collections import deque
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct

ROOT = Path(__file__).resolve().parents[1]
SDK_VERSION = '1.28.8'
SDK_SHA256 = 'e4c41ee6ee9c6c32a45d198463ec08d79bac3bd5fa58adc93d13fe4a08abc9d9'
# These ship with developer tools, but are not guaranteed on a clean Windows PC.
REDISTRIBUTABLE = re.compile(r'^(?:vcruntime|msvcp|msvcr|concrt|vcomp)\d.*\.dll$', re.I)
FORBIDDEN = re.compile(r'^(?:lib)?(?:gstlibav|avcodec|avformat|avutil|swscale|swresample|postproc|x264|x265|faac)', re.I)


def imports(path):
    data = Path(path).read_bytes()
    def u16(offset): return struct.unpack_from('<H', data, offset)[0]
    def u32(offset): return struct.unpack_from('<I', data, offset)[0]
    if data[:2] != b'MZ': raise ValueError(f'Not a PE file: {path}')
    pe = u32(0x3c)
    if data[pe:pe+4] != b'PE\0\0' or u16(pe+4) != 0x8664:
        raise ValueError(f'Expected an x64 PE: {path}')
    optional = pe+24
    if u16(optional) != 0x20b: raise ValueError(f'Expected PE32+: {path}')
    sections = optional + u16(pe+20)
    def offset(rva):
        if rva < u32(optional+60): return rva
        for index in range(u16(pe+6)):
            row = sections+40*index
            size, start, raw_size, raw = struct.unpack_from('<IIII',data,row+8)
            if start <= rva < start+max(size,raw_size): return raw+rva-start
        raise ValueError(f'Invalid PE RVA in {path}: {rva:x}')
    def name(rva):
        start=offset(rva); end=data.find(b'\0',start,start+4096)
        if end<0: raise ValueError(f'Invalid PE import name: {path}')
        value=data[start:end].decode('ascii')
        if Path(value).name != value or '/' in value or '\\' in value:
            raise ValueError(f'Invalid import basename: {value}')
        return value
    names=set()
    for directory, size, name_offset in [(1,20,12),(13,32,4)]:
        if u32(optional+108) <= directory: continue
        rva, length=struct.unpack_from('<II',data,optional+112+8*directory)
        if not rva: continue
        start=offset(rva)
        for row in range(start,min(start+length,len(data))-size+1,size):
            if not any(data[row:row+size]): break
            value=u32(row+name_offset)
            if directory==13 and not (u32(row)&1):
                value-=struct.unpack_from('<Q',data,optional+24)[0]
            names.add(name(value))
    return sorted(names,key=str.casefold)


def plugins():
    names=[]
    for file in ['common.rs.in','windows.rs.in']:
        text=(ROOT/'packaging/media/gstreamer_plugin_lists'/file).read_text()
        names.extend(re.findall(r'^"([\w]+)"',text,re.M))
    return [name+'.dll' for name in names]


def bundle(sdk, destination, system, vc_runtime=None):
    sdk, destination, system=map(Path,[sdk,destination,system])
    provenance=json.loads((sdk/'WEGA-SDK-PROVENANCE.json').read_text(encoding='utf-8-sig'))
    if provenance.get('version') != SDK_VERSION or provenance.get('installer_sha256','').lower() != SDK_SHA256:
        raise ValueError('Use the pinned, verified SDK from scripts/install-windows-media.ps1')
    destination.mkdir(parents=True,exist_ok=True)
    index={}
    for directory in [sdk/'bin',sdk/'lib/gstreamer-1.0']:
        for path in directory.glob('*.dll'):
            key=path.name.casefold()
            if key in index: raise ValueError(f'Ambiguous SDK DLL: {path.name}')
            index[key]=path
    vc_runtime=Path(vc_runtime) if vc_runtime else None
    if vc_runtime:
        if not vc_runtime.is_dir() or vc_runtime.parent.name.lower() != 'x64' or not re.fullmatch(r'Microsoft\.VC[0-9]+\.CRT',vc_runtime.name,re.I):
            raise ValueError('Expected the release x64 Microsoft.VC*.CRT REDIST directory')
        for path in vc_runtime.glob('*.dll'):
            if REDISTRIBUTABLE.match(path.name): index[path.name.casefold()]=path
    executables=[destination/'wega.exe']
    if (destination/'wega-engine.exe').is_file(): executables.append(destination/'wega-engine.exe')
    queue=deque([*executables,*[index.get(name.casefold(),sdk/'missing'/name) for name in plugins()]])
    visited=set(); manifest=[]
    while queue:
        source=queue.popleft(); key=source.name.casefold()
        if key in visited: continue
        visited.add(key)
        if FORBIDDEN.match(source.name): raise ValueError(f'Excluded codec dependency: {source.name}')
        dependencies=imports(source) # Also validates that every bundled file is x64.
        if source.parent != destination:
            target=destination/source.name
            if target.exists() and target.read_bytes()!=source.read_bytes(): raise ValueError(f'DLL collision: {target.name}')
            shutil.copy2(source,target)
            entry={'file':source.name,'sha256':hashlib.sha256(source.read_bytes()).hexdigest()}
            if vc_runtime and source.parent == vc_runtime:
                entry.update(source='Microsoft Visual C++ REDIST',runtime_path=source.relative_to(vc_runtime.parent.parent).as_posix())
            else: entry.update(source='GStreamer SDK',sdk_path=source.relative_to(sdk).as_posix())
            manifest.append(entry)
        for name in dependencies:
            key=name.casefold()
            if FORBIDDEN.match(name): raise ValueError(f'Excluded codec dependency: {name}')
            if key.startswith(('api-ms-win-','ext-ms-win-')): continue
            # Prefer bundled/SDK DLLs even if this developer machine has another
            # copy in System32. VC runtimes are redistributables, not OS APIs.
            if key in index: queue.append(index[key])
            elif (destination/name).is_file(): queue.append(destination/name)
            elif not REDISTRIBUTABLE.match(name) and (system/name).is_file(): continue
            else: raise ValueError(f'Missing runtime dependency: {source.name} -> {name}')
    licenses=sdk/'share/licenses'
    if not licenses.is_dir(): raise ValueError('SDK license texts missing; refusing an incomplete distribution')
    shutil.copytree(licenses,destination/'media-licenses',dirs_exist_ok=True)
    if vc_runtime:
        (destination/'MICROSOFT-VC-RUNTIME-NOTICE.txt').write_text(
            'Microsoft Visual C++ release runtime DLLs\n\n'
            'Copyright Microsoft Corporation. These proprietary redistributable files are separate\n'
            'from Wega and GStreamer. Files come only from the Visual Studio x64 REDIST CRT folder;\n'
            'their source paths and hashes are recorded in MEDIA-MANIFEST.json.\n'
            'Redistribution and use are subject to the applicable Microsoft Software License Terms:\n'
            'https://visualstudio.microsoft.com/license-terms/\n'
            'https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files\n')
    (destination/'MEDIA-MANIFEST.json').write_text(json.dumps({'sdk_version':SDK_VERSION,'installer_sha256':SDK_SHA256,'source':provenance['source'],'files':manifest},indent=2)+'\n')
    (destination/'MEDIA-NOTICE.txt').write_text('Wega Windows media runtime\n\nSelected unmodified DLLs from the official GStreamer 1.28.8 MSVC SDK.\nRequired Microsoft CRT files come from Visual Studio REDIST; see MICROSOFT-VC-RUNTIME-NOTICE.txt when present.\nFile hashes and SDK paths are in MEDIA-MANIFEST.json; license texts are in media-licenses/.\nGStreamer and GLib are dynamically linked LGPL components. Compatible modified DLLs\nmay be substituted beside wega.exe. Source and build instructions:\nhttps://gstreamer.freedesktop.org/src/\nhttps://gitlab.freedesktop.org/gstreamer/cerbero\nNo FFmpeg, gst-libav, x264/x265 or FAAC library is bundled.\n')
    print(f'Bundled {len(manifest)} media DLLs with licenses and exact dependency manifest')


if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--sdk',default=os.environ.get('GSTREAMER_1_0_ROOT_MSVC_X86_64'),required=not os.environ.get('GSTREAMER_1_0_ROOT_MSVC_X86_64'))
    parser.add_argument('--destination',required=True)
    parser.add_argument('--vc-runtime',default=os.environ.get('WEGA_VC_RUNTIME_DIR'))
    args=parser.parse_args()
    bundle(args.sdk,args.destination,Path(os.environ['SystemRoot'])/'System32',args.vc_runtime)
