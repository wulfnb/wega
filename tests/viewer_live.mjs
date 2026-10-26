// Real authenticated noVNC rendering and local touch gestures in isolated Chrome.
// node tests/viewer_live.mjs IMAGE; CHROME_BINARY may select a local Chrome binary.
import {spawn,spawnSync} from 'node:child_process';
import {mkdirSync,mkdtempSync,readFileSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {randomBytes} from 'node:crypto';
import {join} from 'node:path';
import assert from 'node:assert/strict';
const run=(...args)=>{const r=spawnSync('docker',args,{encoding:'utf8'});if(r.status)throw Error(r.stderr);return r.stdout.trim();};
const base=join(process.cwd(),'target/viewer-live');mkdirSync(base,{recursive:true});const out=mkdtempSync(join(base,'run-'));const profile=mkdtempSync(join(tmpdir(),'wega-viewer-chrome-'));const name='wega-viewer-'+process.pid;const password=randomBytes(24).toString('hex');let chrome,ws;
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
try {
 run('run','--rm','-d','--name',name,'-p','127.0.0.1::7443','-e','BROWSER_USER=quality','-e','BROWSER_PASSWORD='+password,process.argv[2] || 'wega-quality:local');
 const port=run('port',name,'7443/tcp').split(':').at(-1);const origin='https://127.0.0.1:'+port;
 chrome=spawn(process.env.CHROME_BINARY || (process.platform==='darwin'?'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome':'google-chrome'),['--headless=new','--remote-debugging-port=0','--user-data-dir='+profile,'--ignore-certificate-errors','--window-size=1200,900','--no-first-run','about:blank'],{stdio:'ignore'});
 let devport;for(let n=0;n<100;n++){try{devport=readFileSync(join(profile,'DevToolsActivePort'),'utf8').split('\n')[0];break;}catch{}await sleep(200);}
 assert.ok(devport,'Chrome did not start');
 const pages=await(await fetch('http://127.0.0.1:'+devport+'/json/list')).json();ws=new WebSocket(pages.find(p=>p.type==='page').webSocketDebuggerUrl);await new Promise(r=>ws.addEventListener('open',r,{once:true}));let id=0;const pending=new Map();
 ws.addEventListener('message',e=>{const message=JSON.parse(e.data);const waiter=pending.get(message.id);if(waiter){pending.delete(message.id);message.error?waiter.reject(Error(message.error.message)):waiter.resolve(message.result);}});
 const cdp=(method,params={})=>new Promise((resolve,reject)=>{const seq=++id;const timeout=setTimeout(()=>reject(Error('CDP timeout: '+method)),15000);pending.set(seq,{resolve:r=>{clearTimeout(timeout);resolve(r);},reject:e=>{clearTimeout(timeout);reject(e);}});ws.send(JSON.stringify({id:seq,method,params}));});
 const evaluate=async expression=>{const r=await cdp('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});if(r.exceptionDetails)throw Error(r.exceptionDetails.text);return r.result.value;};
 const wait=async fn=>{for(let n=0;n<150;n++){try{if(await fn())return;}catch{}await sleep(200);}throw Error('Viewer UI timed out');};
 await cdp('Page.enable');await cdp('Runtime.enable');
 console.log('Chrome started');await cdp('Page.navigate',{url:origin+'/login.html'});await wait(()=>evaluate("document.readyState==='complete'&&!!document.querySelector('form')"));console.log('Login page ready');
 const loginLayout=await evaluate("({display:getComputedStyle(document.body).display,overflow:getComputedStyle(document.body).overflow,card:document.querySelector('.login-card').getBoundingClientRect().toJSON(),width:innerWidth})");assert.equal(loginLayout.display,'flex');assert.notEqual(loginLayout.overflow,'hidden');assert.ok(Math.abs(loginLayout.card.x+loginLayout.card.width/2-loginLayout.width/2)<2,'Login card stays centered');
 const login=await evaluate(`fetch('/auth/login',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({username:'quality',password:${JSON.stringify(password)}})}).then(r=>r.status)`);assert.equal(login,200);console.log('Authenticated');
 await cdp('Page.navigate',{url:origin+'/'});
 await wait(()=>evaluate("document.querySelector('#connection-card')?.hidden && document.querySelector('canvas')?.width>0"));
 const queue=await evaluate(`(async()=>{
   const {default:Display}=await import('/novnc/core/display.js');
   const {QualityController}=await import('/controls.js');
   const canvas=document.createElement('canvas');canvas.width=canvas.height=16;
   const display=new Display(canvas);display.resize(16,16);
   const quality=new QualityController();let clock=performance.now();let pending,level;
   Object.defineProperty(performance,'now',{configurable:true,value:()=>clock});
   try {
     display.imageRect(0,0,16,16,'image/svg+xml',new TextEncoder().encode('<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><path fill="red" d="M0 0H16V16H0Z"/></svg>'));
     display.flip();clock+=200;quality.sample(display.renderStats,false);clock+=200;
     pending=display.renderStats;level=quality.sample(pending,false);
   } finally { delete performance.now; }
   await display.flush();const drained=display.renderStats;
   const pixel=Array.from(display.getImageData().data.slice(0,4));
   for(let n=0;n<10;n++){display.fillRect(0,0,16,16,[255,0,0]);display.flip();quality.sample(display.renderStats,false);}
   return {pending,level,drained,pixel,recovered:quality.level};
 })()`);
 assert.ok(queue.pending.pending>=2 && queue.pending.frames>=1 && queue.pending.oldestMs>=200);
 assert.equal(queue.level,2);assert.equal(queue.drained.pending,0);assert.deepEqual(queue.pixel,[255,0,0,255]);assert.equal(queue.recovered,6);
 await wait(()=>evaluate("(()=>{const c=document.querySelector('canvas');const d=c.getContext('2d').getImageData(0,0,c.width,c.height).data;const colors=new Set();for(let i=0;i<d.length;i+=128)colors.add(d[i]+','+d[i+1]+','+d[i+2]);return colors.size>32})()"));
 const initial=await evaluate("({header:getComputedStyle(document.querySelector('header')).position,screen:document.querySelector('#screen').clientHeight,height:innerHeight,canvas:document.querySelector('canvas').width,styles:document.styleSheets.length})");assert.equal(initial.header,'fixed');assert.equal(initial.screen,initial.height);assert.ok(initial.styles>0);
 await evaluate("document.querySelector('#pan-mode').checked=true;document.querySelector('#pan-mode').dispatchEvent(new Event('change'))");
 const before=await evaluate("document.querySelector('canvas').width");
 await cdp('Emulation.setTouchEmulationEnabled',{enabled:true,maxTouchPoints:5});
 await cdp('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:350,y:400},{x:450,y:400}]});
 await cdp('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:300,y:400},{x:500,y:400}]});
 await cdp('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await sleep(200);
 const zoomed=await evaluate("document.querySelector('canvas').width");assert.ok(zoomed<before*.75,{before,zoomed});
 const imageBefore=await evaluate("document.querySelector('canvas').toDataURL()");
 await cdp('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:600,y:500}]});await cdp('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:550,y:450}]});await cdp('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await sleep(200);
 await wait(async()=>await evaluate("document.querySelector('canvas').toDataURL()")!==imageBefore);
 const imageAfter=await evaluate("document.querySelector('canvas').toDataURL()");assert.ok(imageBefore!==imageAfter,'Panning must move the actual remote framebuffer');
 await cdp('Emulation.setDeviceMetricsOverride',{width:900,height:700,deviceScaleFactor:1,mobile:false});await sleep(300);
 assert.ok(await evaluate("document.querySelector('#screen').classList.contains('viewer-panning')"));
 await evaluate("document.querySelector('#pan-mode').checked=false;document.querySelector('#pan-mode').dispatchEvent(new Event('change'))");
 await wait(async()=>await evaluate("document.querySelector('canvas').width")===initial.canvas);
 const screenshot=await cdp('Page.captureScreenshot',{format:'png'});writeFileSync(join(out,'viewer-live.png'),Buffer.from(screenshot.data,'base64'));
 writeFileSync(join(out,'viewer-live-result.json'),JSON.stringify({passed:true,connected:true,css:true,queue,localPinch:true,localPan:true,resizeRetained:true,restoredFit:true,before,zoomed},null,2));
 console.log('PASS: authenticated real noVNC viewer, valid CSS, local pinch/pan, resize preservation and Fit restoration');
} finally {
 ws?.close();chrome?.kill('SIGTERM');
 if(chrome && chrome.exitCode===null)await Promise.race([new Promise(r=>chrome.once('exit',r)),sleep(5000)]);
 if(chrome?.exitCode===null)chrome.kill('SIGKILL');
 try{writeFileSync(join(out,'container.log'),run('logs',name));}catch{}
 try{run('rm','-f',name);}catch{}
 rmSync(profile,{recursive:true,force:true});
}
