"""Drive real muted audio/video decoding through the browser's W3C HTTP endpoint."""
import base64
import json
import os
from pathlib import Path
import subprocess
import socket
import sys
import tempfile
import time
import urllib.request
import urllib.error
import struct
import zlib

def png_pixel_reader(png):
    """Read the engine's noninterlaced RGBA8 PNG screenshots without extra SDKs."""
    assert png.startswith(b'\x89PNG\r\n\x1a\n')
    offset=8; compressed=bytearray()
    while offset<len(png):
        size=struct.unpack_from('>I',png,offset)[0];kind=png[offset+4:offset+8];data=png[offset+8:offset+8+size];offset+=size+12
        if kind==b'IHDR':
            width,height,depth,color,_,_,interlace=struct.unpack('>IIBBBBB',data)
            assert (depth,color,interlace)==(8,6,0)
        if kind==b'IDAT':compressed.extend(data)
    data=zlib.decompress(compressed);stride=width*4;rows=[];previous=bytearray(stride)
    for y in range(height):
        start=y*(stride+1);mode=data[start];row=bytearray(data[start+1:start+1+stride])
        assert 0<=mode<=4
        for x in range(stride):
            a=row[x-4] if x>=4 else 0;b=previous[x];c=previous[x-4] if x>=4 else 0
            if mode==1:predict=a
            elif mode==2:predict=b
            elif mode==3:predict=(a+b)//2
            elif mode==4:
                p=a+b-c;pa,pb,pc=abs(p-a),abs(p-b),abs(p-c)
                predict=a if pa<=pb and pa<=pc else b if pb<=pc else c
            else:predict=0
            row[x]=(row[x]+predict)&255
        rows.append(row);previous=row
    return width,height,lambda x,y: tuple(rows[y][x*4:x*4+4])

binary = str(Path(sys.argv[1] if len(sys.argv) > 1 else 'target/debug/wega').resolve())
profile = Path(tempfile.mkdtemp(prefix='wega-media-', dir=os.environ.get('WEGA_TEST_OUTPUT')))
with socket.socket() as listener:
    listener.bind(('127.0.0.1', 0))
    port = listener.getsockname()[1]
base = f'http://127.0.0.1:{port}'
def request(method,path,body=None):
    data = None if body is None else json.dumps(body).encode()
    req=urllib.request.Request(base+path,data=data,method=method,headers={'Content-Type':'application/json'})
    try:
        with urllib.request.urlopen(req,timeout=36) as response:
            return json.load(response)['value']
    except urllib.error.HTTPError as error:
        raise AssertionError(error.read().decode()) from error
with (profile/'browser.log').open('w') as log:
    process=subprocess.Popen([binary,'--engine','wega','--profile-dir',str(profile),'--webdriver-port',str(port),'wega://newtab'],stdout=log,stderr=log)
    try:
        deadline=time.monotonic()+30
        while True:
            try:
                if request('GET','/status')['ready']:
                    break
                if time.monotonic() > deadline:
                    raise RuntimeError('Browser did not become ready')
                time.sleep(.2)
            except OSError:
                if time.monotonic()>deadline or process.poll() is not None:raise RuntimeError((profile/'browser.log').read_text())
                time.sleep(.2)
        session=request('POST','/session',{'capabilities':{'alwaysMatch':{'browserName':'wega'}}})['sessionId']
        root=f'/session/{session}'
        request('POST',root+'/url',{'url':os.environ.get('WEGA_FIXTURE_URL', 'http://127.0.0.1:8767') + '/media.html'})
        deadline=time.monotonic()+25
        while True:
            result=request('POST',root+'/execute/sync',{'script':'return window.mediaReport','args':[]})
            assert not result.get('videoError') and not result.get('audioError'),result
            if result.get('videoTime',0)>.5 and result.get('audioTime',0)>.5 and result.get('videoWidth')==320:break
            if time.monotonic()>deadline:raise AssertionError(result)
            time.sleep(.2)
        (profile/'media-report.json').write_text(json.dumps(result,indent=2))
        request('POST',root+'/execute/sync',{'script':'video.pause();audio.pause();if(navigator.mediaSession)navigator.mediaSession.playbackState="paused";return true','args':[]})
        (profile/'media-playback.png').write_bytes(base64.b64decode(request('GET',root+'/screenshot')))
        request('POST',root+'/url',{'url':os.environ.get('WEGA_FIXTURE_URL','http://127.0.0.1:8767')+'/compatibility.html'})
        deadline=time.monotonic()+20
        while True:
            result=request('POST',root+'/execute/sync',{'script':'''return {style:!!document.querySelector('[data-wega-mask-fallback]'),image:getComputedStyle(document.getElementById('icon')).backgroundImage,panels:document.querySelectorAll('[data-wega-media-message]').length,links:document.querySelectorAll('[data-wega-media-message] a[download]').length,width:innerWidth,height:innerHeight}''','args':[]})
            if result['style'] and result['panels']==1 and result['links']==1:break
            if time.monotonic()>deadline:raise AssertionError(result)
            time.sleep(.2)
        assert '/mask-fixture.svg' in result['image'],result
        # Wait for actual SVG image decoding/paint, not just a CSS declaration.
        until=time.monotonic()+10
        while True:
            png=base64.b64decode(request('GET',root+'/screenshot'))
            width,height,pixel=png_pixel_reader(png);sx=width/result['width'];sy=height/result['height']
            corner=pixel(round(5*sx),round(5*sy));center=pixel(round(40*sx),round(40*sy))
            if min(corner[:3])>240 and max(center[:3])<30:break
            if time.monotonic()>until:raise AssertionError({'corner':corner,'center':center,'dimensions':[width,height]})
            time.sleep(.2)
        (profile/'compatibility.png').write_bytes(png)
        (profile/'compatibility-report.json').write_text(json.dumps({'page':result,'icon_corner':corner,'icon_center':center},indent=2))
        request('DELETE',root)
        print(f'MEDIA TEST PASSED: actual muted WebM/WAV decode, advancing clocks, 320×180 frames; per-element failure/download feedback and SVG-mask fallback pixels (white corners, black diamond). Artifacts: {profile}',flush=True)
    finally:
        process.terminate()
        try:process.wait(timeout=10)
        except subprocess.TimeoutExpired:process.kill();process.wait()
