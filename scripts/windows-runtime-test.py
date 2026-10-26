#!/usr/bin/env python3
"""Real packaged Windows media and running-profile URL/file handoff regressions."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer

ROOT=Path(__file__).resolve().parents[1]


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True);args=parser.parse_args()
    if sys.platform!='win32': parser.error('Requires actual Windows')
    binary=Path(args.binary).resolve()
    output=ROOT/'target/windows-runtime-tests';output.mkdir(parents=True,exist_ok=True)
    work=Path(tempfile.mkdtemp(prefix='run-',dir=output))
    spec=importlib.util.spec_from_file_location('fixture',ROOT/'tests/network_server.py')
    fixture=importlib.util.module_from_spec(spec);spec.loader.exec_module(fixture)
    server=ThreadingHTTPServer(('127.0.0.1',0),fixture.Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    env=os.environ.copy()
    sdk=env.get('GSTREAMER_1_0_ROOT_MSVC_X86_64','')
    env['PATH']=os.pathsep.join(path for path in env['PATH'].split(os.pathsep) if not sdk or not path.lower().startswith(sdk.lower()))
    for key in list(env):
        if key.startswith(('GSTREAMER_','GST_PLUGIN','SERVO_GSTREAMER')):env.pop(key)
    env.update(WEGA_FIXTURE_URL=f'http://127.0.0.1:{server.server_port}',WEGA_TEST_OUTPUT=str(work))
    try:
        subprocess.run([sys.executable,str(ROOT/'tests/media_smoke.py'),str(binary)],env=env,check=True,timeout=120)
        capture=work/'wega-engine.png'
        subprocess.run([str(binary),'--engine','wega','--profile-dir',str(work/'wega-engine-profile'),'--capture',str(capture),env['WEGA_FIXTURE_URL']+'/wega-engine.html'],env=env,check=True,timeout=65)
        report=json.loads(capture.with_suffix('.json').read_text())
        assert report['engine']=='wega' and report['servoInitialized'] is False and report['title']=='Wega engine ready',report
        with socket.socket() as listener:
            listener.bind(('127.0.0.1',0));port=listener.getsockname()[1]
        profile=work/'profile';profile.mkdir()
        local=work/'some page & café.html';local.write_text('<title>Local Windows handoff</title><p>Local bytes</p>',encoding='utf8')
        def request(method,path,body=None):
            req=urllib.request.Request(f'http://127.0.0.1:{port}'+path,data=None if body is None else json.dumps(body).encode(),method=method,headers={'Content-Type':'application/json'})
            with urllib.request.urlopen(req,timeout=15) as response:return json.load(response)['value']
        with (work/'handoff.log').open('w') as log:
            primary=subprocess.Popen([str(binary),'--profile-dir',str(profile),'--webdriver-port',str(port),'about:blank'],env=env,stdout=log,stderr=log)
            try:
                until=time.monotonic()+35
                while True:
                    try:
                        if request('GET','/status')['ready']:break
                    except OSError:pass
                    if primary.poll() is not None or time.monotonic()>until:raise AssertionError('Primary browser failed to start')
                    time.sleep(.2)
                sid=request('POST','/session',{'capabilities':{}})['sessionId'];route='/session/'+sid
                for address,title in [(env['WEGA_FIXTURE_URL']+'/second.html','Second Servo document'),(str(local),'Local Windows handoff')]:
                    handles=request('GET',route+'/window/handles')
                    second=subprocess.run([str(binary),'--profile-dir',str(profile),address],env=env,capture_output=True,text=True,timeout=15)
                    assert second.returncode==0,second.stderr
                    until=time.monotonic()+20
                    while True:
                        current=request('GET',route+'/window/handles')
                        if len(current)==len(handles)+1:
                            request('POST',route+'/window',{'handle':current[-1]})
                            # A new tab answers document.title with a JavaScript
                            # error (HTTP 500) until its first document exists.
                            try:
                                if request('GET',route+'/title')==title:break
                            except urllib.error.HTTPError as error:
                                if error.code!=500:raise
                        if time.monotonic()>until:raise AssertionError('URL/file handoff did not load expected content')
                        time.sleep(.2)
                    assert primary.poll() is None,'Primary process exited'
                (work/'result.json').write_text(json.dumps({'passed':True,'pid':primary.pid,'media':True,'web_and_file_handoff':True},indent=2))
            finally:
                primary.terminate()
                try:primary.wait(timeout=8)
                except subprocess.TimeoutExpired:primary.kill();primary.wait()
        print('PASS: packaged media decode and same-profile web/Unicode-file handoff without SDK PATH; '+str(work))
    finally:server.shutdown();server.server_close()


if __name__=='__main__':main()
