import RFB from '/novnc/core/rfb.js';
import {QualityController, ViewportController} from './controls.js';
const status = document.querySelector('#status');
const reconnect = document.querySelector('#reconnect');
const screen = document.querySelector('#screen');
const dialog = document.querySelector('#clipboard-dialog');
const text = document.querySelector('#clipboard-text');
let rfb;
let updatingAccount = false;
let connectionTimer;
let retryTimer;
let qualityTimer;
let retryAttempt = 0;
let viewport;
const adaptiveQuality = new QualityController();
const connectionCard = document.querySelector('#connection-card');
const connectionMessage = document.querySelector('#connection-message');
function connectionState(message, connected = false) {
  status.textContent = message;
  connectionMessage.textContent = message;
  connectionCard.hidden = connected;
  reconnect.hidden = connected;
}
function stopTimers() {
  clearTimeout(connectionTimer);
  clearTimeout(retryTimer);
  clearTimeout(qualityTimer);
}
function failed(client, message) {
  if (rfb !== client) return;
  stopTimers();
  // Invalidate before disconnect: noVNC may dispatch disconnect synchronously.
  rfb = undefined;
  client?.disconnect();
  if (updatingAccount) { connectionState('Reconnecting browser controls…'); return; }
  const delay = [1, 2, 5, 10][Math.min(retryAttempt++, 3)];
  connectionState(`${message} Reconnecting in ${delay} s…`);
  retryTimer = setTimeout(connect, delay * 1000);
  fetch('/auth/account', {cache: 'no-store'}).then(response => {
    if (response.status === 401) { stopTimers(); location.replace('/login.html'); }
  }).catch(() => {});
}
const qualityMode = document.querySelector('#quality-mode');
qualityMode.value = 'auto';
try {
  const saved = localStorage.getItem('wega-viewer-quality');
  if (['auto', 'sharp', 'fast'].includes(saved)) qualityMode.value = saved;
} catch { /* Storage may be disabled by the browser. */ }
function applyQuality() {
  if (!rfb) return;
  const network = navigator.connection;
  const slow = network && (network.saveData || ['slow-2g', '2g', '3g'].includes(network.effectiveType));
  const level = qualityMode.value === 'auto' ? adaptiveQuality.sample(rfb.renderStats, slow) : qualityMode.value === 'sharp' ? 9 : 2;
  rfb.qualityLevel = level;
  rfb.compressionLevel = level <= 2 ? 6 : 2;
}
qualityMode.addEventListener('change', () => {
  try { localStorage.setItem('wega-viewer-quality', qualityMode.value); } catch { /* Optional preference storage. */ }
  adaptiveQuality.reset(false); applyQuality();
});
navigator.connection?.addEventListener('change', applyQuality);
function connect() {
  stopTimers();
  const previous = rfb;
  rfb = undefined;
  previous?.disconnect();
  connectionState('Connecting to your browser…');
  screen.replaceChildren();
  let client;
  try {
    client = new RFB(screen, `wss://${location.host}/websockify`, {shared: true});
  } catch {
    failed(undefined, "Can't reach the browser.");
    return;
  }
  rfb = client;
  adaptiveQuality.reset(Boolean(navigator.connection?.saveData || ['slow-2g', '2g', '3g'].includes(navigator.connection?.effectiveType)));
  viewport = new ViewportController(client, screen);
  viewport.enable(Boolean(document.querySelector('#pan-mode').checked));
  const mode = document.querySelector('#viewport-mode').value;
  client.scaleViewport = mode !== 'actual';
  client.resizeSession = mode === 'resize' && !viewport.active;
  applyQuality();
  connectionTimer = setTimeout(() => failed(client, "Can't reach the browser: server not responding."), 15000);
  client.addEventListener('connect', () => {
    if (rfb !== client) return;
    stopTimers();
    retryAttempt = 0;
    connectionState('Connected', true);
    client.focus();
    if (viewport.active) viewport.reset();
    const sample = () => {
      if (rfb !== client) return;
      if (!document.hidden) applyQuality();
      qualityTimer = setTimeout(sample, 1000);
    };
    qualityTimer = setTimeout(sample, 1000);
  });
  client.addEventListener('disconnect', () => failed(client, 'Connection lost.'));
  client.addEventListener('securityfailure', () => failed(client, 'Connection refused.'));
  client.addEventListener('clipboard', event => {
    if (rfb !== client) return;
    text.value = event.detail.text;
    document.querySelector('#clipboard-offer').hidden = false;
  });
}
reconnect.addEventListener('click', connect);
document.querySelector('#retry-connection').addEventListener('click', connect);
document.querySelector('#fullscreen').addEventListener('click', async () => {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await document.documentElement.requestFullscreen();
  } catch { status.textContent = 'Full screen unavailable'; }
});
document.querySelector('#clipboard').addEventListener('click', () => {rfb?.blur();dialog.showModal();});
dialog.addEventListener('close', () => rfb?.focus());
document.querySelector('#send-clipboard').addEventListener('click', () => {
  rfb?.clipboardPasteFrom(text.value);
  dialog.close();
});
connect();
const downloadsDialog = document.querySelector('#downloads-dialog');
async function listDownloads() {
  const notice = document.querySelector('#downloads-notice');
  const list = document.querySelector('#downloads-list');
  notice.textContent = 'Loading…';
  list.replaceChildren();
  try {
    const response = await fetch('/downloads/', {cache: 'no-store'});
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const files = (await readJson(response)).filter(f => f.type === 'file' && !f.name.startsWith('.'));
    files.sort((a, b) => a.name.localeCompare(b.name));
    for (const file of files) {
      const row = document.createElement('li');
      const link = document.createElement('a');
      link.textContent = file.name;
      link.href = `/downloads/${encodeURIComponent(file.name)}`;
      link.download = file.name;
      const size = document.createElement('span');
      size.textContent = `${(file.size / 1048576).toFixed(1)} MiB`;
      row.append(link, size);
      list.append(row);
    }
    notice.textContent = files.length ? `${files.length} completed file(s)` : 'No completed downloads yet. Use ↓ in the native browser to start a download.';
  } catch (error) { notice.textContent = `Could not list downloads: ${error.message}`; }
}
document.querySelector('#downloads').addEventListener('click', () => { rfb?.blur(); downloadsDialog.showModal(); listDownloads(); });
document.querySelector('#refresh-downloads').addEventListener('click', listDownloads);
document.querySelector('#close-downloads').addEventListener('click', () => downloadsDialog.close());
downloadsDialog.addEventListener('close', () => rfb?.focus());

const accountDialog = document.querySelector('#account-dialog');
const accountForm = document.querySelector('#account-form');
const accountNotice = document.querySelector('#account-notice');
let csrf = '';
// Proxy error pages (nginx 413/502/504) are HTML, not JSON: explain them
// instead of surfacing a parse error such as "Unexpected token '<'".
async function readJson(response) {
  try {
    return await response.json();
  } catch {
    if (response.status === 413) throw new Error('This file is too large to upload.');
    throw new Error(response.status >= 500
      ? 'The browser is not responding. Wait a moment and try again.'
      : 'The server sent an unexpected reply. Reload the page and try again.');
  }
}
async function loadAccount() {
  const response = await fetch('/auth/account', {cache: 'no-store'});
  if (response.status === 401) { location.replace('/login.html'); throw new Error('Please sign in.'); }
  if (!response.ok) throw new Error('Could not load account settings.');
  const result = await readJson(response);
  csrf = result.csrf;
  return result;
}
document.querySelector('#account').addEventListener('click', async () => {
  rfb?.blur();
  accountForm.reset();
  accountNotice.textContent = 'Loading…';
  accountDialog.showModal();
  try { accountForm.elements.username.value = (await loadAccount()).username; accountNotice.textContent = ''; }
  catch (error) { accountNotice.textContent = error.message; }
});
accountForm.addEventListener('submit', async event => {
  event.preventDefault();
  const button = accountForm.querySelector('button[type="submit"]');
  button.disabled = true;
  updatingAccount = true;
  accountNotice.textContent = 'Saving…';
  try {
    const response = await fetch('/auth/account', {
      method: 'POST', headers: {'Content-Type': 'application/json', 'X-CSRF-Token': csrf},
      body: JSON.stringify(Object.fromEntries(new FormData(accountForm)))
    });
    const result = await readJson(response);
    if (!response.ok) throw new Error(result.error || 'Could not save account settings.');
    status.textContent = 'Account updated';
    accountDialog.close();
    setTimeout(connect, 700);
  } catch (error) {
    accountNotice.textContent = error.message;
    if (!rfb) connect();
  }
  finally {
    for (const input of accountForm.querySelectorAll('input[type="password"]')) input.value = '';
    button.disabled = false;
    updatingAccount = false;
  }
});
document.querySelector('#close-account').addEventListener('click', () => accountDialog.close());
accountDialog.addEventListener('close', () => { accountForm.reset(); csrf = ''; rfb?.focus(); });
document.querySelector('#signout').addEventListener('click', async () => {
  try {
    await loadAccount();
    const response = await fetch('/auth/logout', {
      method: 'POST', headers: {'Content-Type': 'application/json', 'X-CSRF-Token': csrf}, body: '{}'
    });
    if (!response.ok) throw new Error('Could not sign out.');
    rfb?.disconnect();
    location.replace('/login.html');
  } catch (error) { status.textContent = error.message; }
});

const viewportMode = document.querySelector('#viewport-mode');
viewportMode.addEventListener('change', () => {
  document.querySelector('#pan-mode').checked = false;
  viewport?.enable(false);
  screen.classList.toggle('actual-size', viewportMode.value === 'actual');
  if (rfb) {
    rfb.scaleViewport = viewportMode.value !== 'actual';
    rfb.resizeSession = viewportMode.value === 'resize';
  }
  status.textContent = viewportMode.value === 'actual' ? 'Actual size: scroll to pan the desktop' : viewportMode.value === 'resize' ? 'Remote resize requested; use Fit if unsupported' : 'Fit to window';
});
document.querySelector('#pan-mode').addEventListener('change', event => {
  viewport?.enable(event.target.checked);
  if (event.target.checked) {
    viewport?.reset();
    status.textContent = 'Viewer pan/zoom: drag to pan, pinch to zoom. Turn off to control the page.';
  } else { viewportMode.dispatchEvent(new Event('change')); }
});
document.querySelector('#reset-view').addEventListener('click', () => viewport?.reset());
for (const type of ['touchstart', 'touchmove', 'touchend', 'touchcancel']) {
  screen.addEventListener(type, event => {
    if (!viewport?.active) return;
    event.preventDefault(); event.stopImmediatePropagation();
    const rect = screen.getBoundingClientRect();
    const points = Array.from(event.touches).slice(0,2).map(t=>({x:t.clientX-rect.left,y:t.clientY-rect.top}));
    if (type === 'touchmove') viewport.move(points); else viewport.start(points);
  }, {capture:true, passive:false});
}
for (const type of ['mousedown', 'mousemove', 'mouseup', 'mouseleave']) {
  screen.addEventListener(type, event => {
    if (!viewport?.active) return;
    event.preventDefault(); event.stopImmediatePropagation();
    const points = event.buttons === 1 ? [{x:event.clientX,y:event.clientY}] : [];
    if (type === 'mousemove') viewport.move(points); else viewport.start(points);
  }, {capture:true});
}
const uploadDialog = document.querySelector('#upload-dialog');
const uploadForm = document.querySelector('#upload-form');
const uploadNotice = document.querySelector('#upload-notice');
const uploadPath = document.querySelector('#upload-path');
let uploadCsrf = '';
document.querySelector('#upload').addEventListener('click', async () => {
  rfb?.blur(); uploadForm.reset(); uploadPath.value = ''; uploadNotice.textContent = 'Loading…'; uploadDialog.showModal();
  try { uploadCsrf = (await loadAccount()).csrf; uploadNotice.textContent = ''; }
  catch (error) { uploadNotice.textContent = error.message; }
});
uploadForm.addEventListener('submit', async event => {
  event.preventDefault();
  const file = document.querySelector('#upload-file').files[0];
  if (!file || file.size > 32 * 1024 * 1024) { uploadNotice.textContent = 'Choose a file of 32 MiB or smaller.'; return; }
  const button = uploadForm.querySelector('button[type="submit"]'); button.disabled = true;
  uploadNotice.textContent = 'Uploading…'; uploadPath.value = '';
  try {
    const response = await fetch('/auth/upload', {method:'POST', headers:{'Content-Type':'application/octet-stream', 'X-CSRF-Token':uploadCsrf, 'X-Upload-Name':encodeURIComponent(file.name)}, body:file});
    const result = await readJson(response);
    if (!response.ok) throw new Error(result.error || 'Upload failed.');
    uploadPath.value = result.path; uploadPath.focus(); uploadPath.select();
    uploadNotice.textContent = 'Uploaded. Copy this remote path into the website file chooser.';
  } catch (error) { uploadNotice.textContent = error.message; }
  finally { button.disabled = false; }
});
document.querySelector('#close-upload').addEventListener('click', () => uploadDialog.close());
uploadDialog.addEventListener('close', () => { uploadForm.reset(); uploadCsrf = ''; rfb?.focus(); });

const toolbar = document.querySelector('header');
let toolbarTimer;
function revealToolbar() {
  toolbar.classList.toggle('toolbar-hidden', false);
  clearTimeout(toolbarTimer);
  toolbarTimer = setTimeout(() => toolbar.classList.toggle('toolbar-hidden', true), 2000);
}
document.addEventListener('pointermove', event => { if (event.clientY < 60) revealToolbar(); });
document.addEventListener('pointerdown', event => { if (event.clientY < 60) revealToolbar(); });
document.addEventListener('keydown', event => {
  // Escape closes an open View/Account menu and returns focus to its button.
  if (event.key === 'Escape') {
    for (const menu of document.querySelectorAll('header details[open]')) {
      menu.open = false;
      menu.querySelector('summary').focus();
    }
  }
  if (event.altKey && event.shiftKey && event.code === 'KeyV') {
    event.preventDefault();
    revealToolbar();
    document.querySelector('#clipboard').focus();
  }
});
document.addEventListener('fullscreenchange', () => toolbar.classList.toggle('toolbar-hidden', true));
revealToolbar();

// Paste is supplied by the user's paste gesture; do not read the local clipboard in the background.
screen.addEventListener('paste', event => {
  const value = event.clipboardData?.getData('text/plain');
  if (!rfb || !value) return;
  event.preventDefault();
  rfb.clipboardPasteFrom(value);
});
document.querySelector('#copy-local').addEventListener('click', async () => {
  try {
    await navigator.clipboard.writeText(text.value);
    document.querySelector('#clipboard-offer').hidden = true;
    status.textContent = 'Copied to this computer';
  } catch {
    rfb?.blur();
    dialog.showModal();
    text.focus(); text.select();
    status.textContent = 'Select and copy the text manually';
  }
});
document.querySelector('#dismiss-copy').addEventListener('click', () => {
  document.querySelector('#clipboard-offer').hidden = true;
});

const fileRequest = document.querySelector('#file-request');
const chooserFiles = document.querySelector('#chooser-files');
let pendingFileRequest;
async function pollFileRequest() {
  try {
    const response = await fetch('/auth/chooser', {cache:'no-store'});
    if (!response.ok) return;
    const next = (await readJson(response)).request;
    const changed = next?.id !== pendingFileRequest?.id;
    pendingFileRequest = next;
    fileRequest.hidden = !pendingFileRequest;
    if (pendingFileRequest && changed) {
      document.querySelector('#file-request-label').textContent = `${pendingFileRequest.origin} is asking for a file. Choose here or drop files onto the browser.`;
      chooserFiles.multiple = pendingFileRequest.multiple;
      chooserFiles.accept = pendingFileRequest.extensions.filter(value => /^[a-zA-Z0-9]+$/.test(value)).map(value => '.' + value).join(',');
    }
  } catch { /* A disconnected viewer retries on the next poll. */ }
  finally { setTimeout(pollFileRequest, 1500); }
}
setTimeout(pollFileRequest, 1500);
document.querySelector('#choose-local').addEventListener('click', () => { chooserFiles.value=''; chooserFiles.click(); });
async function completeFileRequest(files) {
  const request = pendingFileRequest;
  if (!request) return;
  const button = document.querySelector('#choose-local');
  button.disabled = true;
  try {
    if (files.length > 32 || files.some(file => file.size > 32 * 1024 * 1024) || (!request.multiple && files.length > 1)) throw new Error('Choose at most 32 files, each 32 MiB or smaller.');
    const token = (await loadAccount()).csrf;
    const paths = [];
    for (const file of files) {
      const response = await fetch('/auth/upload', {method:'POST',headers:{'Content-Type':'application/octet-stream','X-CSRF-Token':token,'X-Upload-Name':encodeURIComponent(file.name)},body:file});
      const result = await readJson(response);
      if (!response.ok) throw new Error(result.error || 'Upload failed.');
      paths.push(result.path);
    }
    const response = await fetch('/auth/chooser', {method:'POST',headers:{'Content-Type':'application/json','X-CSRF-Token':token},body:JSON.stringify({id:request.id,paths})});
    const result = await readJson(response);
    if (!response.ok) throw new Error(result.error || 'The file chooser could not accept the upload.');
    if (pendingFileRequest?.id === request.id) { pendingFileRequest=undefined;fileRequest.hidden=true; }
    status.textContent = files.length ? 'File sent to the page' : 'File selection cancelled';
  } catch (error) { document.querySelector('#file-request-label').textContent=error.message; }
  finally { button.disabled=false; }
}
chooserFiles.addEventListener('change', () => { const files=Array.from(chooserFiles.files); if (files.length) return completeFileRequest(files); });
document.querySelector('#cancel-file-request').addEventListener('click', () => completeFileRequest([]));
screen.addEventListener('dragover', event => {
  if (pendingFileRequest && Array.from(event.dataTransfer?.types || []).includes('Files')) event.preventDefault();
});
screen.addEventListener('drop', event => {
  const files = Array.from(event.dataTransfer?.files || []);
  if (!pendingFileRequest || !files.length) return;
  event.preventDefault();
  return completeFileRequest(files);
});

const remoteKeyboard = document.querySelector('#remote-keyboard');
const keyboardButton = document.querySelector('#keyboard');
let keyboardValue = '';
function resetKeyboard() {
  keyboardValue = '_'.repeat(32);
  remoteKeyboard.value = keyboardValue;
  remoteKeyboard.setSelectionRange(keyboardValue.length, keyboardValue.length);
}
keyboardButton.addEventListener('click', () => {
  if (document.activeElement === remoteKeyboard) { remoteKeyboard.blur(); return; }
  rfb?.blur(); resetKeyboard(); remoteKeyboard.focus();
});
remoteKeyboard.addEventListener('focus', () => {
  keyboardButton.setAttribute('aria-pressed','true');
  if (rfb) rfb.focusOnClick = false;
});
remoteKeyboard.addEventListener('blur', () => {
  keyboardButton.setAttribute('aria-pressed','false');
  if (rfb) rfb.focusOnClick = true;
});
remoteKeyboard.addEventListener('input', () => {
  if (!rfb) { resetKeyboard(); return; }
  const before = Array.from(keyboardValue), after = Array.from(remoteKeyboard.value);
  let common = 0;
  while (common < before.length && common < after.length && before[common] === after[common]) common++;
  for (let index=common;index<before.length;index++) rfb.sendKey(0xff08,'Backspace');
  for (const character of after.slice(common)) {
    const code=character.codePointAt(0);
    rfb.sendKey(code === 10 ? 0xff0d : code <= 255 ? code : 0x01000000 + code);
  }
  keyboardValue = remoteKeyboard.value;
  if (!after.length || after.length > 256) resetKeyboard();
});
remoteKeyboard.addEventListener('keydown', event => {
  const special = {Enter:0xff0d,Tab:0xff09,Escape:0xff1b,ArrowLeft:0xff51,ArrowUp:0xff52,ArrowRight:0xff53,ArrowDown:0xff54};
  let key = special[event.key];
  if (!key && (event.ctrlKey || event.metaKey) && !event.altKey && Array.from(event.key || '').length === 1) {
    const code = event.key.codePointAt(0);
    key = code <= 255 ? code : 0x01000000 + code;
  }
  if (!rfb || !key) return;
  event.preventDefault();
  const modifiers = [[event.ctrlKey,0xffe3,'ControlLeft'],[event.altKey,0xffe9,'AltLeft'],[event.shiftKey,0xffe1,'ShiftLeft'],[event.metaKey,0xffeb,'SuperLeft']].filter(item=>item[0]);
  for (const [,symbol,code] of modifiers) rfb.sendKey(symbol,code,true);
  rfb.sendKey(key,event.code);
  for (const [,symbol,code] of modifiers.reverse()) rfb.sendKey(symbol,code,false);
});
screen.addEventListener('mousedown', event => {
  if (document.activeElement === remoteKeyboard) event.preventDefault();
});
try {
  if (navigator.maxTouchPoints > 0 && !localStorage.getItem('wega-touch-hint')) document.querySelector('#touch-hint').hidden=false;
} catch { /* The hint remains optional if device storage is unavailable. */ }
document.querySelector('#dismiss-touch-hint').addEventListener('click', () => {
  document.querySelector('#touch-hint').hidden=true;
  try { localStorage.setItem('wega-touch-hint','seen'); } catch { /* Optional preference. */ }
});
