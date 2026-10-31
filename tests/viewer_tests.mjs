import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import {fileURLToPath} from 'node:url';
const filename = fileURLToPath(new URL('../docker/viewer/viewer.js', import.meta.url));
class Element {
  constructor() { this.listeners = new Map(); this.value = ''; this.children = []; this.hidden = false; this.passwords = [{value: 'secret'}]; this.elements = {username: {value: ''}}; this.files = []; this.attributes = {}; this.classList = {toggle: (name, value) => {this[name] = value;}}; }
  addEventListener(event, callback) { const previous=this.listeners.get(event); this.listeners.set(event,previous?async e=>{await previous(e);return callback(e);}:callback); }
  async fire(event, detail = {}) { await this.listeners.get(event)?.({preventDefault() {}, detail, ...detail}); await new Promise(setImmediate); }
  replaceChildren() { this.children = []; }
  append(...children) { this.children.push(...children); }
  showModal() { this.open = true; }
  close() { this.open = false; this.listeners.get('close')?.(); }
  querySelector() { return this.button ||= {disabled: false}; }
  querySelectorAll() { return this.passwords; }
  focus() {this.focused = true;}
  select() {this.selected = true;}
  setAttribute(name,value) {this.attributes[name]=value;}
  setSelectionRange(start,end) {this.selection=[start,end];}
  blur() {this.focused=false;}
  click() {this.clicked=true;}
  reset() { this.resets = (this.resets || 0) + 1; }
  dispatchEvent(event) { return this.listeners.get(event.type)?.(event); }
  getBoundingClientRect() { return {left:0,top:0}; }
}
async function fixture(constructorFails = false, savedQuality = null, touch = false) {
  const elements = new Map();
  const get = id => { if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id); };
  const clients = [], calls = [], replies = [], locations = [], timers = [];
  class RFB extends Element {
    constructor(screen, url, options) { super(); if (constructorFails) {constructorFails=false; throw new Error('WebSocket unavailable');} clients.push(this); this.url = url; this.options = options; }
    focus() { this.focused = true; }
    blur() { this.focused = false; }
    disconnect() { this.disconnected = true; }
    clipboardPasteFrom(value) { this.pasted = value; }
    sendKey(...args) {(this.keys ||= []).push(args);}
  }
  const document = {listeners:new Map(), addEventListener(event, callback) {this.listeners.set(event, callback);}, querySelector: get, createElement: () => new Element(), documentElement: {requestFullscreen: async () => {document.fullscreenElement = true;}}, exitFullscreen: async () => {document.fullscreenElement = false;}};
  const storage = {getItem:()=>savedQuality,setItem:(key,value)=>{storage.value=value;}};
  const connection = new Element();
  const clipboard = {writeText:async value => {clipboard.value=value;}};
  const context = vm.createContext({document, Event, navigator:{clipboard,connection,maxTouchPoints:touch?1:0}, localStorage:storage, location: {host: 'test.local', replace: url => locations.push(url)}, fetch: async (...args) => {
    calls.push(args);
    const response = replies.shift();
    if (response instanceof Error) throw response;
    if (typeof response === 'function') return await response();
    assert.ok(response, 'Unexpected fetch ' + args[0]);
    return response;
  }, FormData: class { constructor(form) { return new Map([['username', form.elements.username.value], ['password', 'new-secret']]); } }, setTimeout: (callback, delay) => { const timer = {callback, delay, active:true}; timers.push(timer); return timer; }, clearTimeout: timer => {if (timer) timer.active = false;}});
  const module = new vm.SourceTextModule(readFileSync(filename, 'utf8'), {context, identifier: filename});
  await module.link(specifier => specifier === './controls.js' ? new vm.SourceTextModule(readFileSync(new URL('../docker/viewer/controls.js', import.meta.url), 'utf8'), {context}) : new vm.SyntheticModule(['default'], function () { this.setExport('default', RFB); }, {context}));
  await module.evaluate();
  return {get, document, clients, calls, replies, locations, timers, clipboard, storage, connection};
}
const ok = value => ({ok: true, status: 200, json: async () => value});

test('connection events ignore stale clients and reveal authentication expiry', async () => {
  const f = await fixture(); const first = f.clients[0];
  assert.equal(first.url, 'wss://test.local/websockify');
  await first.fire('connect'); assert.equal(f.get('#status').textContent, 'Connected');
  await first.fire('clipboard', {text: 'from browser'}); assert.equal(f.get('#clipboard-text').value, 'from browser');
  await f.get('#reconnect').fire('click'); assert.equal(first.disconnected, true);
  await first.fire('connect'); await first.fire('disconnect'); await first.fire('securityfailure');
  assert.equal(f.get('#status').textContent, 'Connecting to your browser…');
  const second = f.clients[1];
  f.replies.push({status: 401}); await second.fire('disconnect'); assert.deepEqual(f.locations, ['/login.html']);
  assert.equal(f.get('#reconnect').hidden, false);
});

test('fullscreen failure and clipboard controls restore focus', async () => {
  const f = await fixture();
  await f.get('#fullscreen').fire('click'); assert.equal(f.document.fullscreenElement, true);
  await f.get('#fullscreen').fire('click'); assert.equal(f.document.fullscreenElement, false);
  f.document.documentElement.requestFullscreen = async () => {throw new Error('denied');};
  await f.get('#fullscreen').fire('click'); assert.equal(f.get('#status').textContent, 'Full screen unavailable');
  await f.get('#clipboard').fire('click'); assert.equal(f.get('#clipboard-dialog').open, true);
  f.get('#clipboard-text').value = 'to browser';
  await f.get('#send-clipboard').fire('click'); assert.equal(f.clients[0].pasted, 'to browser'); assert.equal(f.clients[0].focused, true);
});

test('downloads show only completed files with safe URLs and handle errors', async () => {
  const f = await fixture();
  f.replies.push(ok([{type: 'file', name: 'z file.bin', size: 1048576}, {type: 'directory', name: 'directory'}, {type: 'file', name: '.partial'}, {type: 'file', name: 'a.bin', size: 0}]));
  await f.get('#downloads').fire('click');
  const rows = f.get('#downloads-list').children;
  assert.equal(rows.length, 2); assert.equal(rows[0].children[0].textContent, 'a.bin');
  assert.equal(rows[1].children[0].href, '/downloads/z%20file.bin');
  assert.equal(rows[1].children[1].textContent, '1.0 MiB');
  f.replies.push(ok([])); await f.get('#refresh-downloads').fire('click'); assert.ok(f.get('#downloads-notice').textContent.startsWith('No completed'));
  f.replies.push({ok: false, status: 503}); await f.get('#refresh-downloads').fire('click'); assert.ok(f.get('#downloads-notice').textContent.includes('HTTP 503'));
  await f.get('#close-downloads').fire('click'); assert.equal(f.clients[0].focused, true);
});

test('account loading errors, saving and signout clear secrets and check CSRF', async () => {
  const f = await fixture();
  f.replies.push({ok: false, status: 401}); await f.get('#account').fire('click'); assert.ok(f.get('#account-notice').textContent.includes('Please sign in'));
  f.replies.push({ok: false, status: 503}); await f.get('#account').fire('click'); assert.ok(f.get('#account-notice').textContent.includes('Could not load'));
  f.replies.push(ok({username: 'alice', csrf: 'csrf-token'})); await f.get('#account').fire('click');
  assert.equal(f.get('#account-form').elements.username.value, 'alice');
  f.replies.push(ok({})); await f.get('#account-form').fire('submit');
  const post = f.calls.at(-1); assert.equal(post[1].headers['X-CSRF-Token'], 'csrf-token');
  assert.equal(f.get('#account-form').passwords[0].value, ''); assert.equal(f.get('#account-form').button.disabled, false);
  assert.equal(f.get('#status').textContent, 'Account updated');
  f.timers.find(t => t.delay === 700).callback(); assert.equal(f.clients.length, 2);
  for (const result of [{error: 'Rejected'}, {}]) {
    f.replies.push({ok: false, json: async () => result}); await f.get('#account-form').fire('submit');
    assert.ok(f.get('#account-notice').textContent.includes(result.error || 'Could not save'));
  }
  await f.get('#close-account').fire('click');
  f.replies.push(ok({csrf: 'logout-token'}), {ok: false}); await f.get('#signout').fire('click'); assert.equal(f.get('#status').textContent, 'Could not sign out.');
  f.replies.push(ok({csrf: 'logout-token'}), ok({})); await f.get('#signout').fire('click');
  assert.equal(f.calls.at(-1)[1].headers['X-CSRF-Token'], 'logout-token');
  assert.equal(f.locations.at(-1), '/login.html'); assert.equal(f.clients.at(-1).disconnected, true);
});

test('bridge disconnect during credential rotation does not redirect or check old cookie', async () => {
  const f = await fixture();
  f.replies.push(async () => { await f.clients[0].fire('disconnect'); return ok({}); });
  await f.get('#account-form').fire('submit');
  assert.equal(f.calls.length, 1); assert.deepEqual(f.locations, []);
});


test('viewport modes allow actual-size panning and explicit remote resizing', async () => {
  const f = await fixture();
  const mode = f.get('#viewport-mode');
  for (const value of ['actual', 'resize', 'fit']) {
    mode.value = value; await mode.fire('change');
    assert.equal(f.clients[0].scaleViewport, value !== 'actual');
    assert.equal(f.clients[0].resizeSession, value === 'resize');
    assert.equal(f.get('#screen')['actual-size'], value === 'actual');
  }
});

test('uploads are bounded, authenticated and return a selectable remote path', async () => {
  const f = await fixture();
  f.replies.push(ok({csrf:'upload-token'})); await f.get('#upload').fire('click');
  assert.equal(f.get('#upload-dialog').open, true);
  await f.get('#upload-form').fire('submit');
  assert.match(f.get('#upload-notice').textContent, /Choose a file/);
  f.get('#upload-file').files = [{name:'large', size:33*1024*1024}];
  await f.get('#upload-form').fire('submit'); assert.equal(f.calls.length, 1);
  f.get('#upload-file').files = [{name:'a b.txt', size:4}];
  f.replies.push(ok({path:'/config/auth/uploads/file-123/a b.txt'}));
  await f.get('#upload-form').fire('submit');
  assert.equal(f.calls[1][0], '/auth/upload');
  assert.equal(f.calls[1][1].headers['X-CSRF-Token'], 'upload-token');
  assert.equal(f.calls[1][1].headers['X-Upload-Name'], 'a%20b.txt');
  assert.equal(f.get('#upload-path').selected, true);
  assert.match(f.get('#upload-notice').textContent, /Uploaded/);
  f.replies.push({ok:false,json:async()=>({error:'Storage full'})});
  await f.get('#upload-form').fire('submit'); assert.equal(f.get('#upload-notice').textContent, 'Storage full');
  f.replies.push({ok:false,json:async()=>({})});
  await f.get('#upload-form').fire('submit'); assert.equal(f.get('#upload-notice').textContent, 'Upload failed.');
  f.replies.push(new Error('offline')); await f.get('#upload-form').fire('submit');
  assert.equal(f.get('#upload-notice').textContent, 'offline');
  // Proxy error pages are HTML; the notice explains them instead of a parse error.
  const html = status => ({ok:false,status,json:async()=>{throw new SyntaxError("Unexpected token '<'");}});
  f.replies.push(html(413)); await f.get('#upload-form').fire('submit');
  assert.equal(f.get('#upload-notice').textContent, 'This file is too large to upload.');
  f.replies.push(html(502)); await f.get('#upload-form').fire('submit');
  assert.match(f.get('#upload-notice').textContent, /browser is not responding/);
  f.replies.push(html(404)); await f.get('#upload-form').fire('submit');
  assert.match(f.get('#upload-notice').textContent, /unexpected reply/);
  await f.get('#close-upload').fire('click'); assert.equal(f.clients[0].focused, true);
  f.replies.push(new Error('offline')); await f.get('#upload').fire('click');
  assert.equal(f.get('#upload-notice').textContent, 'offline');
});

test('silent connection timeout retries with backoff and successful recovery cancels timers', async () => {
  const f = await fixture();
  assert.equal(f.get('#connection-card').hidden, false);
  for (const delay of [1000, 2000, 5000, 10000, 10000]) {
    f.replies.push(new Error('offline'));
    f.timers.findLast(t => t.active && t.delay === 15000).callback();
    await new Promise(setImmediate);
    assert.match(f.get('#connection-message').textContent, /server not responding/);
    const retry = f.timers.findLast(t => t.active && t.delay === delay);
    assert.ok(retry);
    retry.callback();
  }
  await f.clients.at(-1).fire('connect');
  assert.equal(f.get('#connection-card').hidden, true);
  assert.deepEqual(f.timers.filter(t => t.active && t.delay !== 2000 && t.delay !== 1500).map(t=>t.delay), [1000]);
  f.replies.push(ok({}));
  await f.clients.at(-1).fire('securityfailure');
  assert.match(f.get('#connection-message').textContent, /Connection refused.*1 s/);
  await f.get('#retry-connection').fire('click');
  assert.equal(f.get('#status').textContent, 'Connecting to your browser…');
});

test('viewport selection during a lost connection is safe', async () => {
  const f = await fixture();
  f.replies.push(ok({}));
  await f.clients[0].fire('disconnect');
  f.get('#viewport-mode').value = 'actual';
  await f.get('#viewport-mode').fire('change');
  await f.get('#retry-connection').fire('click');
  assert.equal(f.clients.at(-1).scaleViewport, false);
});

test('floating controls reveal from the top edge or keyboard and hide after idle', async () => {
  const f = await fixture();
  const header = f.get('header');
  f.timers.findLast(t => t.delay === 2000).callback();
  assert.equal(header['toolbar-hidden'], true);
  f.document.listeners.get('pointermove')({clientY:20});
  assert.equal(header['toolbar-hidden'], false);
  f.document.listeners.get('pointermove')({clientY:200});
  f.document.listeners.get('pointerdown')({clientY:20});
  f.document.listeners.get('pointerdown')({clientY:200});
  let prevented = false;
  f.document.listeners.get('keydown')({altKey:true, shiftKey:true, code:'KeyV', preventDefault(){prevented=true;}});
  assert.equal(prevented, true);
  assert.equal(f.get('#clipboard').focused, true);
  f.document.listeners.get('keydown')({code:'KeyA'});
  const summary = {focus(){this.focused=true;}}, menu = {open:true, querySelector:()=>summary};
  f.document.querySelectorAll = selector => { assert.equal(selector, 'header details[open]'); return [menu]; };
  f.document.listeners.get('keydown')({key:'Escape', code:'Escape'});
  assert.equal(menu.open, false); assert.equal(summary.focused, true);
  f.document.listeners.get('fullscreenchange')();
  assert.equal(header['toolbar-hidden'], true);
});

async function bootstrapFixture(outcome) {
  const elements = new Map();
  const get = id => { if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id); };
  const timers = [];
  let reloads = 0;
  const context = vm.createContext({document:{querySelector:get}, location:{reload(){reloads++;}}, setTimeout(callback, delay){const timer={callback,delay};timers.push(timer);return timer;},clearTimeout(timer){timer.cleared=true;}});
  const script = new vm.Script(readFileSync(new URL('../docker/viewer/bootstrap.js', import.meta.url),'utf8'), {importModuleDynamically:async()=>{
    if (outcome === 'failure') throw new Error('Module HTTP 500');
    if (outcome === 'hang') await new Promise(()=>{});
    const module = new vm.SyntheticModule([],function(){},{context});
    await module.link(()=>{}); await module.evaluate(); return module;
  }});
  script.runInContext(context);
  await new Promise(setImmediate);
  return {get,timers,reloads:()=>reloads};
}
for (const outcome of ['failure','hang']) {
  test(`bootstrap exposes reload when module ${outcome === 'hang' ? 'never loads' : 'fails'}`, async()=>{
    const f = await bootstrapFixture(outcome);
    if (outcome === 'hang') f.timers[0].callback();
    assert.equal(f.get('#connection-card').hidden,false);
    assert.match(f.get('#connection-message').textContent,/could not load/);
    f.get('#retry-connection').onclick();
    assert.equal(f.reloads(),1);
    f.timers[0].callback(); // A late deadline must not overwrite a settled result.
  });
}
test('bootstrap clears deadline after a successful module load',async()=>{
  const f = await bootstrapFixture('success');
  assert.equal(f.timers[0].cleared,true);
  f.timers[0].callback();
  assert.equal(f.get('#connection-message').textContent,undefined);
});

test('synchronous connection failure exposes retry and recovers',async()=>{
  const f = await fixture(true);
  assert.match(f.get('#connection-message').textContent,/Can't reach the browser/);
  assert.equal(f.get('#retry-connection').hidden,false);
  await f.get('#retry-connection').fire('click');
  await f.clients.at(-1).fire('connect');
  assert.equal(f.get('#connection-card').hidden,true);
});

test('a connection dropped during a rejected account update is restored', async()=>{
  const f = await fixture();
  f.replies.push(async()=>{await f.clients[0].fire('disconnect'); return {ok:false,json:async()=>({error:'Rejected'})};});
  await f.get('#account-form').fire('submit');
  assert.equal(f.clients.length,2);
  assert.equal(f.get('#account-notice').textContent,'Rejected');
  assert.equal(f.get('#connection-card').hidden,false);
});

test('viewer clipboard uses explicit paste and copy gestures with manual fallback', async()=>{
  const f=await fixture();
  await f.get('#screen').fire('paste',{});
  await f.get('#screen').fire('paste',{clipboardData:{getData:()=> 'local text'}});
  assert.equal(f.clients[0].pasted,'local text');
  await f.clients[0].fire('clipboard',{text:'remote text'});
  assert.equal(f.get('#clipboard-offer').hidden,false);
  await f.get('#copy-local').fire('click');
  assert.equal(f.clipboard.value,'remote text');
  assert.equal(f.get('#clipboard-offer').hidden,true);
  f.clipboard.writeText=async()=>{throw new Error('Denied');};
  await f.get('#copy-local').fire('click');
  assert.equal(f.get('#clipboard-dialog').open,true);
  assert.equal(f.get('#clipboard-text').selected,true);
  await f.get('#dismiss-copy').fire('click');
  assert.equal(f.get('#clipboard-offer').hidden,true);
  f.replies.push(ok({}));await f.clients[0].fire('disconnect');
  await f.get('#screen').fire('paste',{clipboardData:{getData:()=> 'ignored'}});
});

test('picture presets persist and Auto follows available connection hints',async()=>{
  const f=await fixture(false,'sharp');
  assert.equal(f.clients[0].qualityLevel,9);
  const mode=f.get('#quality-mode');
  mode.value='fast';await mode.fire('change');
  assert.equal(f.clients[0].qualityLevel,2);assert.equal(f.storage.value,'fast');
  mode.value='auto';await mode.fire('change');assert.equal(f.clients[0].qualityLevel,6);
  f.connection.effectiveType='3g';await f.connection.fire('change');assert.equal(f.clients[0].qualityLevel,2);
  f.connection.effectiveType='4g';f.connection.saveData=true;await f.connection.fire('change');assert.equal(f.clients[0].compressionLevel,6);
  f.connection.saveData=false;await f.connection.fire('change');assert.equal(f.clients[0].qualityLevel,6);
  f.storage.setItem=()=>{throw new Error('Disabled');};await mode.fire('change');
  f.replies.push(ok({}));await f.clients[0].fire('disconnect');await mode.fire('change');
  const invalid=await fixture(false,'invalid');assert.equal(invalid.get('#quality-mode').value,'auto');
});

async function pollChooser(f, response) {
  f.replies.push(response);
  const timer=f.timers.findLast(timer=>timer.delay===1500);
  await timer.callback();
}
const pickerRequest = {id:'a'.repeat(32),origin:'https://example.test',multiple:false,extensions:['txt','bad/path']};
test('viewer uploads directly into a current native chooser and supports cancellation',async()=>{
  const f=await fixture();
  await pollChooser(f,ok({request:pickerRequest}));
  assert.equal(f.get('#file-request').hidden,false);
  assert.equal(f.get('#chooser-files').accept,'.txt');
  await f.get('#choose-local').fire('click');assert.equal(f.get('#chooser-files').clicked,true);
  f.get('#chooser-files').files=[];await f.get('#chooser-files').fire('change');
  f.get('#chooser-files').files=[{name:'a.txt',size:2}];
  f.replies.push(ok({csrf:'token'}),ok({path:'/config/uploads/file-123/a.txt'}),ok({ok:true}));
  await f.get('#chooser-files').fire('change');
  assert.equal(JSON.parse(f.calls.at(-1)[1].body).id,pickerRequest.id);
  assert.deepEqual(JSON.parse(f.calls.at(-1)[1].body).paths,['/config/uploads/file-123/a.txt']);
  assert.equal(f.get('#file-request').hidden,true);
  await f.get('#cancel-file-request').fire('click');
  await pollChooser(f,ok({request:pickerRequest}));
  f.replies.push(ok({csrf:'token'}),ok({}));await f.get('#cancel-file-request').fire('click');
  assert.deepEqual(JSON.parse(f.calls.at(-1)[1].body).paths,[]);
  await pollChooser(f,ok({request:null}));assert.equal(f.get('#file-request').hidden,true);
  await pollChooser(f,{ok:false});await pollChooser(f,new Error('offline'));
});
test('viewer chooser keeps errors visible and rejects excessive files before uploading',async()=>{
  const f=await fixture();await pollChooser(f,ok({request:pickerRequest}));
  f.get('#chooser-files').files=[{name:'large',size:33*1024*1024}];await f.get('#chooser-files').fire('change');assert.match(f.get('#file-request-label').textContent,/32 MiB/);
  f.get('#chooser-files').files=[{name:'x',size:1},{name:'y',size:1}];await f.get('#chooser-files').fire('change');assert.match(f.get('#file-request-label').textContent,/32 MiB/);
  f.get('#chooser-files').files=[{name:'x',size:1}];
  for (const result of [{error:'Storage full'},{}]) {
    f.replies.push(ok({csrf:'token'}),{ok:false,json:async()=>result});await f.get('#chooser-files').fire('change');assert.match(f.get('#file-request-label').textContent,/Storage full|Upload failed/);
  }
  for (const result of [{error:'Chooser closed'},{}]) {
    f.replies.push(ok({csrf:'token'}),ok({path:'/config/uploads/file-x/x'}),{ok:false,json:async()=>result});await f.get('#chooser-files').fire('change');assert.match(f.get('#file-request-label').textContent,/Chooser closed|could not accept/);
  }
  await pollChooser(f,ok({request:pickerRequest}));assert.match(f.get('#file-request-label').textContent,/could not accept/);
});
test('dropping files only submits when a page has an active chooser',async()=>{
  const f=await fixture();
  await f.get('#screen').fire('dragover',{});await f.get('#screen').fire('drop',{});
  const files=[{name:'drop.txt',size:2}];
  await f.get('#screen').fire('drop',{dataTransfer:{files}});assert.equal(f.calls.length,0);
  await pollChooser(f,ok({request:pickerRequest}));
  let prevented=false;
  await f.get('#screen').fire('dragover',{dataTransfer:{types:['Files']},preventDefault(){prevented=true;}});assert.equal(prevented,true);
  f.replies.push(ok({csrf:'token'}),ok({path:'/config/uploads/file-x/drop.txt'}),ok({}));
  await f.get('#screen').fire('drop',{dataTransfer:{files}});
  assert.equal(f.get('#file-request').hidden,true);
});

test('virtual keyboard sends Unicode edits, deletion and special keys to the remote browser',async()=>{
  const f=await fixture();const input=f.get('#remote-keyboard'), client=f.clients[0];
  await f.get('#keyboard').fire('click');assert.equal(input.focused,true);
  await input.fire('focus');assert.equal(client.focusOnClick,false);
  const baseline=input.value;
  input.value=baseline+'é😀';await input.fire('input');assert.deepEqual(client.keys.slice(-2).map(key=>key[0]),[233,0x0101f600]);
  input.value=baseline+'é';await input.fire('input');assert.equal(client.keys.at(-1)[0],0xff08);
  input.value+='\n';await input.fire('input');assert.equal(client.keys.at(-1)[0],0xff0d);
  await input.fire('keydown',{key:'Enter',code:'Enter'});assert.equal(client.keys.at(-1)[0],0xff0d);
  await input.fire('keydown',{key:'x',code:'KeyX'});
  await input.fire('keydown',{key:'a',code:'KeyA',ctrlKey:true});assert.equal(client.keys.at(-3)[0],0xffe3);assert.equal(client.keys.at(-1)[2],false);
  f.document.activeElement=input;let prevented=false;await f.get('#screen').fire('mousedown',{preventDefault(){prevented=true;}});assert.equal(prevented,true);
  await f.get('#keyboard').fire('click');assert.equal(input.focused,false);
  f.document.activeElement=undefined;await f.get('#screen').fire('mousedown');
  await input.fire('blur');assert.equal(client.focusOnClick,true);
  input.value='';await input.fire('input');assert.ok(input.value.length);
  input.value='x'.repeat(257);await input.fire('input');assert.equal(input.value.length,32);
  f.replies.push(ok({}));await client.fire('disconnect');await input.fire('focus');await input.fire('blur');await input.fire('keydown',{key:'Enter'});await input.fire('input');
});
test('touch hints are dismissible and optional storage failure does not block controls',async()=>{
  const f=await fixture(false,null,true);assert.equal(f.get('#touch-hint').hidden,false);
  await f.get('#dismiss-touch-hint').fire('click');assert.equal(f.get('#touch-hint').hidden,true);
  f.storage.setItem=()=>{throw new Error('Disabled');};await f.get('#dismiss-touch-hint').fire('click');
});

test('measured quality sampling pauses when hidden, respects manual modes and abandons stale clients', async()=>{
  const f=await fixture(); const c=f.clients[0];
  c.renderStats={pending:1,frames:1,oldestMs:300,completed:0};
  await c.fire('connect');
  let timer=f.timers.findLast(t=>t.active&&t.delay===1000);
  f.document.hidden=true;timer.callback();assert.equal(c.qualityLevel,6);
  f.document.hidden=false;
  for(let n=0;n<2;n++) f.timers.findLast(t=>t.active&&t.delay===1000).callback();
  assert.equal(c.qualityLevel,2);
  f.get('#quality-mode').value='sharp';await f.get('#quality-mode').fire('change');assert.equal(c.qualityLevel,9);
  timer=f.timers.findLast(t=>t.active&&t.delay===1000);
  await f.get('#reconnect').fire('click');assert.equal(timer.active,false);
  timer.callback();assert.equal(c.qualityLevel,9);
});

test('viewer navigation mode consumes local gestures and restores normal control', async()=>{
  const f=await fixture();const c=f.clients[0],s=f.get('#screen');
  s.clientWidth=500;s.clientHeight=400;
  c.viewportMetrics={width:1000,height:800,scale:1,x:0,y:0};
  c.setViewport=(...args)=>{c.transform=args;};
  await s.fire('touchmove',{touches:[]}); // Remote interaction is untouched.
  await s.fire('mousemove',{});
  f.get('#pan-mode').checked=true;await f.get('#pan-mode').fire('change',{target:{checked:true}});
  assert.equal(c.viewOnly,true);assert.deepEqual(c.transform,[.5,0,0]);
  const event={preventDefault(){this.prevented=true;},stopImmediatePropagation(){this.stopped=true;},touches:[{clientX:50,clientY:50}]};
  await s.fire('touchstart',event);await s.fire('touchmove',{...event,touches:[{clientX:25,clientY:25}]});
  assert.deepEqual(c.transform,[1,25,25]);
  await s.fire('touchend',{...event,touches:[]});await s.fire('touchcancel',{...event,touches:[]});
  await s.fire('mousedown',{...event,buttons:1,clientX:20,clientY:20});
  await s.fire('mousemove',{...event,buttons:1,clientX:10,clientY:10});assert.deepEqual(c.transform,[1,10,10]);
  await s.fire('mouseup',{...event,buttons:0});await s.fire('mouseleave',{...event,buttons:0});
  await f.get('#reset-view').fire('click');assert.deepEqual(c.transform,[.5,0,0]);
  await f.get('#pan-mode').fire('change',{target:{checked:false}});assert.equal(c.viewOnly,false);
  f.get('#viewport-mode').value='resize';await f.get('#viewport-mode').fire('change');
  f.get('#pan-mode').checked=true;await f.get('#reconnect').fire('click');
  assert.equal(f.clients.at(-1).resizeSession,false,'Reconnect must keep remote resizing paused while local pan mode is enabled');
  f.clients[1].viewportMetrics=c.viewportMetrics;f.clients[1].setViewport=c.setViewport;
  await f.clients[1].fire('connect');
});
