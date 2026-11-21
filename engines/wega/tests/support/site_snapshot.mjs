// Freeze a live site into a self-contained, script-free snapshot and render
// that snapshot offline in Chrome as the reference for native_site_corpus.rs.
//
//   node site_snapshot.mjs <chrome> <profile-dir> <url> <out-dir> <name> [width] [height]
//
// Writes <name>.html (stylesheets inlined, scripts removed, URLs absolute),
// <name>-chrome.png and <name>-chrome.json (one [x,y,w,h] per element of
// `body *` in document order, the same probe the Wega test evaluates).
import { spawn } from 'node:child_process';
import { writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import path from 'node:path';

const [binary, profile, url, outDir, name, w = '1280', h = '800'] = process.argv.slice(2);
const width = Number(w), height = Number(h);
const chrome = spawn(binary, ['--headless=new', '--disable-gpu', '--no-first-run',
  '--no-default-browser-check', '--hide-scrollbars', `--user-data-dir=${profile}`,
  '--remote-debugging-pipe'], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
let next = 0, buffer = '';
const pending = new Map(), listeners = new Set();
function send(method, params = {}, sessionId) {
  const id = ++next;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    chrome.stdio[3].write(JSON.stringify({ id, method, params, sessionId }) + '\0');
  });
}
chrome.stdio[4].on('data', data => {
  buffer += data.toString();
  let boundary;
  while ((boundary = buffer.indexOf('\0')) >= 0) {
    const message = JSON.parse(buffer.slice(0, boundary)); buffer = buffer.slice(boundary + 1);
    if (message.id) {
      const handler = pending.get(message.id); pending.delete(message.id);
      if (handler) message.error ? handler.reject(new Error(JSON.stringify(message.error))) : handler.resolve(message.result);
    } else for (const listener of listeners) listener(message);
  }
});
const deadline = setTimeout(() => { chrome.kill(); process.exit(2); }, 60000);

// Shared with native_site_corpus.rs: keep the two in sync.
export const PROBE = "Array.from(document.body.querySelectorAll('*')).slice(0,800).map(e=>{const r=e.getBoundingClientRect();return [Math.round(r.x),Math.round(r.y),Math.round(r.width),Math.round(r.height)]})";

const FREEZE = `(async () => {
  const absolute = (text, base) => text.replace(/url\\(\\s*(['"]?)([^'")]+)\\1\\s*\\)/g,
    (m, q, u) => u.startsWith('data:') ? m : 'url("' + new URL(u, base) + '")');
  for (const link of Array.from(document.querySelectorAll('link[rel~=stylesheet]'))) {
    const style = document.createElement('style');
    try { style.textContent = absolute(await (await fetch(link.href)).text(), link.href); } catch (e) {}
    if (link.media) style.media = link.media;
    link.replaceWith(style);
  }
  // CSS-in-JS sheets built with insertRule have no text of their own.
  for (const style of Array.from(document.querySelectorAll('style'))) {
    if (!style.textContent.trim() && style.sheet && style.sheet.cssRules.length)
      style.textContent = Array.from(style.sheet.cssRules, r => r.cssText).join('\\n');
  }
  for (const sheet of document.adoptedStyleSheets || []) {
    const style = document.createElement('style');
    style.textContent = Array.from(sheet.cssRules, r => r.cssText).join('\\n');
    document.head.append(style);
  }
  document.querySelectorAll('script,noscript,iframe,link[rel=preload],link[rel=modulepreload]').forEach(n => n.remove());
  for (const el of document.querySelectorAll('[src]')) el.setAttribute('src', el.src);
  for (const el of document.querySelectorAll('a[href]')) el.setAttribute('href', el.href);
  for (const el of document.querySelectorAll('[srcset]')) el.removeAttribute('srcset');
  for (const el of document.querySelectorAll('[style]')) el.setAttribute('style', absolute(el.getAttribute('style'), location.href));
  return '<!DOCTYPE html>\\n' + document.documentElement.outerHTML;
})()`;

async function open(target, offline) {
  const t = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId: t.targetId, flatten: true });
  await send('Page.enable', {}, sessionId);
  await send('Network.enable', {}, sessionId);
  if (offline) await send('Network.emulateNetworkConditions', { offline: true, latency: 0, downloadThroughput: -1, uploadThroughput: -1 }, sessionId);
  await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false }, sessionId);
  await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: 'light' }] }, sessionId);
  const loaded = new Promise(resolve => {
    const listener = m => { if (m.sessionId === sessionId && m.method === 'Page.loadEventFired') { listeners.delete(listener); resolve(); } };
    listeners.add(listener);
  });
  await send('Page.navigate', { url: target }, sessionId);
  await loaded;
  await send('Runtime.evaluate', { expression: 'document.fonts.ready.then(()=>new Promise(r=>setTimeout(r,1500)))', awaitPromise: true }, sessionId);
  return sessionId;
}
async function evaluate(sessionId, expression) {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, sessionId);
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
}

try {
  const live = await open(url, false);
  const html = await evaluate(live, FREEZE);
  const file = path.resolve(outDir, `${name}.html`);
  await writeFile(file, html);
  const frozen = await open(pathToFileURL(file).href, true);
  await evaluate(frozen, 'scrollTo(0,0)');
  const geometry = await evaluate(frozen, PROBE);
  const shot = await send('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false }, frozen);
  await writeFile(path.resolve(outDir, `${name}-chrome.png`), Buffer.from(shot.data, 'base64'));
  await writeFile(path.resolve(outDir, `${name}-chrome.json`), JSON.stringify({ url, width, height, geometry }));
  process.stdout.write(`${name}: ${html.length} bytes, ${geometry.length} elements\n`);
} finally {
  clearTimeout(deadline); chrome.kill();
}
