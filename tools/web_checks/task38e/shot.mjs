// A screenshot of a page in a headless Chrome (web/check.mjs's flags: its own profile in the
// repo, nothing but 127.0.0.1 reachable), after it had <seconds> to load; optional JS run first.
//   node tools/web_checks/task38e/shot.mjs <url> <out.jpg> [seconds] [js]
import { spawn } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const [url, out, seconds = '3', js = ''] = process.argv.slice(2);
const profile = resolve(import.meta.dirname, '../../../re/scratch/task38e/chrome-shot');
const port = 9361;
rmSync(profile, { recursive: true, force: true });
mkdirSync(profile, { recursive: true });
const browser = spawn('C:/Program Files/Google/Chrome/Application/chrome.exe', [
  '--headless=new', `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`, '--window-size=1100,1300',
  '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--mute-audio',
  '--proxy-server=http://127.0.0.1:9', '--proxy-bypass-list=127.0.0.1;localhost;[::1]',
  '--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost, EXCLUDE 127.0.0.1', 'about:blank',
], { env: { ...process.env, TEMP: profile, TMP: profile }, stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let target;
for (let i = 0; i < 100 && !target; i++) {
  await sleep(200);
  try {
    target = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find((t) => t.type === 'page');
  } catch {}
}
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener('open', r));
let id = 0;
const waiting = new Map();
ws.addEventListener('message', (e) => {
  const m = JSON.parse(e.data);
  if (waiting.has(m.id)) waiting.get(m.id)(m.result);
});
const send = (method, params = {}) => new Promise((r) => {
  waiting.set(++id, r);
  ws.send(JSON.stringify({ id, method, params }));
});
await send('Page.navigate', { url });
await sleep(Number(seconds) * 1000);
if (js) await send('Runtime.evaluate', { expression: js, awaitPromise: true });
await sleep(500);
const { data } = await send('Page.captureScreenshot', { format: 'jpeg', quality: 70, captureBeyondViewport: false });
writeFileSync(out, Buffer.from(data, 'base64'));
await send('Browser.close').catch(() => {});
for (let i = 0; i < 50 && browser.exitCode === null; i++) await sleep(100);
if (browser.exitCode === null) browser.kill();
await sleep(1000);
rmSync(profile, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
console.log(`${out}: ${Buffer.from(data, 'base64').length} bytes`);
process.exit(0);
