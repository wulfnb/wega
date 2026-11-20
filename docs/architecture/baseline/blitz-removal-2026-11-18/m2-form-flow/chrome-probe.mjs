// Isolated mature-browser reference. Chrome's sandbox stays enabled.
// CDP viewport capture avoids the CLI screenshot's document-origin crop after scroll.
import { spawn } from 'node:child_process';
import { writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
const [binary, profile, file, screenshot] = process.argv.slice(2);
const chrome = spawn(binary, ['--headless=new', '--disable-gpu', '--no-first-run',
  '--no-default-browser-check', '--disable-background-networking',
  `--user-data-dir=${profile}`, '--remote-debugging-pipe'],
  { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
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
chrome.on('exit', code => { for (const handler of pending.values()) handler.reject(new Error(`Chrome exited: ${code}`)); });
const deadline = setTimeout(() => { chrome.kill(); process.exit(2); }, 18000);
try {
  const version = await send('Browser.getVersion');
  const target = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId: target.targetId, flatten: true });
  await send('Page.enable', {}, sessionId);
  await send('Emulation.setDeviceMetricsOverride', { width: 300, height: 300, deviceScaleFactor: 1, mobile: false }, sessionId);
  const loaded = new Promise(resolve => {
    const listener = message => { if (message.sessionId === sessionId && message.method === 'Page.loadEventFired') { listeners.delete(listener); resolve(); } };
    listeners.add(listener);
  });
  await send('Page.navigate', { url: pathToFileURL(file).href }, sessionId); await loaded;
  await send('Runtime.evaluate', { expression: 'document.fonts.ready.then(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))))', awaitPromise: true }, sessionId);
  const result = await send('Runtime.evaluate', { expression: "({geometry:Array.from(document.querySelectorAll('input,button')).map(e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return [e.id,r.x,r.y,r.width,r.height,s.margin,s.padding,s.borderWidth]}),target:document.elementFromPoint(20,60)?.id,viewport:[innerWidth,innerHeight]})", returnByValue: true }, sessionId);
  if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
  const capture = await send('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false }, sessionId);
  await writeFile(screenshot, Buffer.from(capture.data, 'base64'));
  const report = { ...result.result.value, browser: version.product };
  await writeFile(screenshot + '.json', JSON.stringify(report));
  process.stdout.write(JSON.stringify(report.geometry));
} finally {
  clearTimeout(deadline); chrome.kill();
}
