import os,subprocess,time,pathlib,json,re,sys,hashlib
ROOT=pathlib.Path('/Users/noufalbabu/dev/personal/wega');OUT=pathlib.Path('/private/tmp/wega-blitz-final-validation');OUT.mkdir(exist_ok=True)
engine=['cargo','test','--manifest-path','engines/Cargo.toml','-p','wega-engine','--offline']
app='/private/tmp/WegaBlitzCampaign-'+subprocess.check_output(['git','rev-parse','--short','HEAD'],cwd=ROOT,text=True).strip()+'.app';helper=app+'/Contents/Helpers/WegaContent.app/Contents/MacOS/wega-engine'
tasks=[
('engine-workspace',['cargo','test','--manifest-path','engines/Cargo.toml','--workspace','--offline','--','--test-threads=4'],{'RUST_MIN_STACK':'8388608'},900),
('root-tests',['cargo','test','--offline'],{},600),
('engine-clippy',['cargo','clippy','--manifest-path','engines/Cargo.toml','--workspace','--all-targets','--offline','--','-D','warnings'],{},600),
('root-clippy',['cargo','clippy','--workspace','--all-targets','--offline','--','-D','warnings'],{},600),
('engine-fmt',['cargo','fmt','--manifest-path','engines/Cargo.toml','--all','--','--check'],{},60),
('root-fmt',['cargo','fmt','--all','--','--check'],{},60),
('diff-check',['git','diff','--check'],{},30),
('native-security',engine+sum([['--test',x] for x in ['native_browser_behavior','native_stylesheets','native_images','iframes','same_origin_dom','iframe_wpt','isolate_safety','security','inherited_broker','native_resource_redirects','native_accessibility']],[])+['--','--test-threads=4'],{'WEGA_RENDER_BACKEND':'native','WEGA_TEST_BROKER':'1','RUST_MIN_STACK':'8388608'},300),
('worker-build',['cargo','build','--manifest-path','engines/Cargo.toml','-p','wega-engine','--bin','wega-engine','--no-default-features','--offline'],{},300),
('sandbox-bundle',['engines/target/debug/wega-content-bundle','engines/target/debug/wega-engine',app],{'WEGA_CODESIGN_IDENTITY':'-'},60),
('codesign-verify',['codesign','--verify','--strict','--verbose=2',app+'/Contents/Helpers/WegaContent.app'],{},30),
('required-sandbox',engine+['--test','inherited_broker','--','--ignored','--exact','a_required_worker_runs_jit_storage_cookies_and_frames_through_grants','--nocapture'],{'WEGA_RENDER_BACKEND':'native','WEGA_CONTENT_HELPER':helper},180),
('existing-inspector-sandbox',engine+['--test','inspector_sandbox','--','--ignored','--nocapture'],{'WEGA_CONTENT_HELPER':helper},180),
('chrome-wpt',['python3','scripts/native-compat-gate.py','--chrome-bin','/Applications/Google Chrome.app/Contents/MacOS/Google Chrome','--output',str(OUT/'chrome')],{},600),
('public-pages',engine+['--test','native_public_compat','--','--nocapture'],{'WEGA_PUBLIC_COMPAT':'1','WEGA_TEST_BROKER':'1','WEGA_RENDER_BACKEND':'native','WEGA_NATIVE_EVIDENCE_DIR':str(OUT/'public')},120),
('independent-build',['cargo','build','--manifest-path','engines/Cargo.toml','-p','wega-native','--no-default-features','--locked','--offline'],{},300),
('independent-graph',['python3','scripts/check-blitz-independence.py','--package','wega-native','--output',str(OUT/'independent-graph.json')],{},60),
('production-graph',['python3','scripts/check-blitz-independence.py','--output',str(OUT/'production-graph.json')],{},60),
('release-comparable',['/usr/bin/time','-l','cargo','test','--release','--manifest-path','engines/Cargo.toml','-p','wega-engine','--offline','--test','native_benchmark','--test','native_stage_profile','--','--nocapture','--test-threads=1'],{'WEGA_NATIVE_EVIDENCE_DIR':str(OUT/'release')},900),
('release-independent',['/usr/bin/time','-l','cargo','test','--release','--manifest-path','engines/Cargo.toml','-p','wega-native','--offline','--test','benchmark','--','--nocapture','--test-threads=1'],{'WEGA_NATIVE_EVIDENCE_DIR':str(OUT/'release-independent')},900),
]
rows=json.loads((OUT/'results.json').read_text()) if (OUT/'results.json').exists() else []
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
for name,command,environment,timeout in tasks:
 if any(r['name']==name and r['exit_code']==0 and r['head']==head for r in rows):continue
 row={'name':name,'head':head,'command':command,'environment':environment,'active':True,'log':str(OUT/(name+'.log'))}
 (OUT/'active.json').write_text(json.dumps(row,indent=2)+'\n');print('START',name,flush=True);start=time.monotonic()
 with open(row['log'],'w') as log:
  try:r=subprocess.run(command,cwd=ROOT,env=dict(os.environ,**environment),stdout=log,stderr=subprocess.STDOUT,timeout=timeout);code=r.returncode
  except subprocess.TimeoutExpired:code=124
 row.update(active=False,exit_code=code,seconds=round(time.monotonic()-start,3))
 data=pathlib.Path(row['log']).read_text(errors='replace');row['log_sha256']=hashlib.sha256(data.encode()).hexdigest()
 counts=re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',data)
 row['cargo_test_totals']={k:sum(int(c[i]) for c in counts) for i,k in enumerate(['passed','failed','ignored'])}
 rows=[r for r in rows if r['name']!=name]+[row];(OUT/'results.json').write_text(json.dumps(rows,indent=2)+'\n');print('DONE',name,code,row['seconds'],row['cargo_test_totals'],flush=True)
(OUT/'active.json').write_text(json.dumps({'active':False,'head':head,'complete':True})+'\n')
