// SPDX-License-Identifier: GPL-3.0-or-later

// Drives the browser preview in a headless Chromium and reports what it did: no window, its
// own throw-away profile, closed again at the end. Needs Node 22+ (its built-in WebSocket and
// fetch) and Chrome or Edge; nothing is installed.
//
//   node web/check.mjs --url "http://127.0.0.1:8080/?go=1&autodrive=1&car=bmw_z4_gt3&track=ks_laguna_seca"
//        [--seconds 20] [--screenshot shot.png] [--size 1280x720] [--chrome <chrome.exe>]
//        [--profile <folder>] [--port 9333] [--timeout 900] [--flags "--use-angle=d3d11 ..."]
//        [--wait-checks]   (with the folder: wait until every car was checked, then list the refusals)
//        [--keep-profile]  (do not empty the browser profile before and after)
//        [--keys "KeyT,Shift+KeyT,KeyY,BracketRight,BracketLeft"]   (after --seconds of driving:
//                          real key presses, one by one, with what the display showed of the
//                          aids before and after each)
//
// Waits until the page drives (or its self test is done, with ?selftest=N), lets it drive for
// --seconds, takes a screenshot and prints the lap timer, the speed, the frame rate, the
// physics steps per second and the page's notes as JSON. A self test with `&keys=...` in the
// address reports its key presses in `selftest.presses`.

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const at = args.indexOf(name);
  return at >= 0 && at + 1 < args.length ? args[at + 1] : fallback;
};
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const url = option('--url');
if (!url) {
  console.error('usage: node web/check.mjs --url <page url> [--seconds 20] [--screenshot file.png] [--size 1280x720] [--chrome <path>] [--profile <folder>] [--port 9333]');
  process.exit(2);
}
const seconds = Number(option('--seconds', '20'));
const [width, height] = option('--size', '1280x720').split('x').map(Number);
const port = Number(option('--port', '9333'));
const timeout = Number(option('--timeout', '900')) * 1000;
const profile = resolve(option('--profile', resolve(root, 'target', 'web-check-profile')));
const chrome =
  option('--chrome') ||
  ['C:/Program Files/Google/Chrome/Application/chrome.exe', 'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe', 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', '/usr/bin/google-chrome', '/usr/bin/chromium'].find(existsSync);
if (!chrome) {
  console.error('no Chrome or Edge found: pass --chrome <path>');
  process.exit(2);
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
// --keep-profile: the browser's storage of the last run stays (to see the page's file cache work)
const keepProfile = args.includes('--keep-profile');
// the profile folder is emptied before and after: it has to be one this script made (it leaves
// a marker file beside it), so a slip of the hand cannot delete a real browser profile
const marker = `${profile}.rustyac-check`;
const emptied = { recursive: true, force: true, maxRetries: 40, retryDelay: 250 };
if (existsSync(profile) && readdirSync(profile).length > 0 && !existsSync(marker)) {
  console.error(`${profile} exists and was not made by this script: give --profile a new or empty folder`);
  process.exit(2);
}
if (!keepProfile) rmSync(profile, emptied);
mkdirSync(profile, { recursive: true });
writeFileSync(marker, 'the folder beside this file is a throw-away browser profile of web/check.mjs\n');
// (the browser's temporary files go into the profile's folder too, not the system's)
const env = { ...process.env, TEMP: profile, TMP: profile, TMPDIR: profile };
const flags = [
  '--headless=new',
  `--user-data-dir=${profile}`,
  `--remote-debugging-port=${port}`,
  `--window-size=${width},${height}`,
  '--no-first-run',
  '--no-default-browser-check',
  '--disable-extensions',
  '--mute-audio',
  '--enable-unsafe-webgpu',
  '--ignore-gpu-blocklist',
  '--disable-background-timer-throttling',
  '--disable-renderer-backgrounding',
  '--disable-backgrounding-occluded-windows',
  ...option('--flags', '').split(' ').filter(Boolean),
  'about:blank',
];
const browser = spawn(chrome, flags, { env, stdio: 'ignore' });
let closed = false;
browser.on('exit', () => {
  closed = true;
});

async function close(code) {
  try {
    await send('Browser.close');
  } catch {
    // already gone
  }
  // (a busy machine can take a while to let the browser go)
  for (let i = 0; i < 300 && !closed; i++) await sleep(100);
  if (!closed) browser.kill();
  await sleep(1000);
  if (!keepProfile) {
    // (the browser lets go of its files a moment after it has gone)
    try {
      rmSync(profile, emptied);
      rmSync(marker, { force: true });
    } catch (error) {
      console.error(`the profile ${profile} could not be removed: ${error.message}`);
    }
  }
  process.exit(code);
}

let socket;
let nextId = 1;
const waiting = new Map();
const messages = [];

function send(method, params = {}) {
  return new Promise((done, failed) => {
    const id = nextId++;
    waiting.set(id, { done, failed });
    socket.send(JSON.stringify({ id, method, params }));
  });
}

async function evaluate(expression) {
  const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
  return result.result.value;
}

// A key press as a keyboard makes it (a trusted event, through the browser's own input path).
const KEY_FACTS = { KeyT: ['t', 0x54], KeyY: ['y', 0x59], BracketRight: [']', 0xdd], BracketLeft: ['[', 0xdb], KeyR: ['r', 0x52], KeyG: ['g', 0x47], KeyH: ['h', 0x48] };
async function pressKey(name) {
  const parts = name.split('+');
  const code = parts.pop();
  const [key, windowsVirtualKeyCode] = KEY_FACTS[code] || [code, 0];
  const shift = parts.includes('Shift');
  const base = { code, key: shift ? key.toUpperCase() : key, windowsVirtualKeyCode, modifiers: (shift ? 8 : 0) | (parts.includes('Ctrl') ? 2 : 0) | (parts.includes('Alt') ? 1 : 0) };
  await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...base });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
}

try {
  let target;
  for (let i = 0; i < 100 && !target; i++) {
    await sleep(200);
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      target = list.find((t) => t.type === 'page');
    } catch {
      // not listening yet
    }
  }
  if (!target) throw new Error(`${chrome} did not open its debugging port ${port}`);
  socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((done, failed) => {
    socket.onopen = done;
    socket.onerror = failed;
  });
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    if (message.id && waiting.has(message.id)) {
      const { done, failed } = waiting.get(message.id);
      waiting.delete(message.id);
      if (message.error) failed(new Error(message.error.message));
      else done(message.result);
    } else if (message.method === 'Runtime.consoleAPICalled') {
      messages.push(`${message.params.type}: ${message.params.args.map((a) => a.value ?? a.description ?? '').join(' ')}`);
    } else if (message.method === 'Runtime.exceptionThrown') {
      messages.push(`exception: ${message.params.exceptionDetails.exception?.description || message.params.exceptionDetails.text}`);
    }
  };
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
  const started = Date.now();
  await send('Page.navigate', { url });

  const page = () => evaluate('JSON.stringify(window.rustyac ? { state: window.rustyac.state, error: window.rustyac.error, hud: window.rustyac.hud, perf: window.rustyac.perf, notes: window.rustyac.notes, content: window.rustyac.content || null, selftest: window.rustyac.selftest || null, last: window.rustyac.log.slice(-1)[0] } : null)').then((text) => JSON.parse(text));
  let now = null;
  let lastLine = '';
  for (;;) {
    await sleep(500);
    now = await page().catch(() => null);
    if (now && now.last && now.last !== lastLine && process.stderr.isTTY) process.stderr.write(`\r${now.last.slice(0, 110).padEnd(110)}`);
    lastLine = now?.last || lastLine;
    if (now && ['driving', 'selftest', 'error'].includes(now.state)) break;
    if (Date.now() - started > timeout) throw new Error(`the page did not start driving within ${timeout / 1000} s (state: ${now?.state}, last: ${now?.last})`);
  }
  if (process.stderr.isTTY) process.stderr.write('\n');
  const report = { url, browser: chrome, load_seconds: (Date.now() - started) / 1000, state: now.state, error: now.error, notes: now.notes };
  if (now.state === 'driving') {
    const samples = [];
    const until = Date.now() + seconds * 1000;
    let seen = null;
    while (Date.now() < until) {
      await sleep(250);
      now = await page();
      if (now.perf && JSON.stringify(now.perf) !== seen) {
        seen = JSON.stringify(now.perf);
        samples.push(now.perf);
      }
      if (now.state !== 'driving') break;
    }
    const wantKeys = option('--keys', '').split(',').filter(Boolean);
    if (wantKeys.length && now.state === 'driving') {
      const shown = () => evaluate("['hud-tc', 'hud-abs', 'hud-bias'].map((id) => document.getElementById(id).textContent).join(' | ') + '  note: ' + (document.getElementById('hud-note').hidden ? '' : document.getElementById('hud-note').textContent)");
      report.key_presses = [];
      for (const name of wantKeys) {
        const before = await shown();
        await pressKey(name);
        await sleep(400);
        report.key_presses.push({ key: name, before, after: await shown() });
      }
      now = await page();
    }
    // (the first second holds the shader compiles)
    const steady = samples.slice(1);
    const mean = (key) => (steady.length ? steady.reduce((sum, s) => sum + s[key], 0) / steady.length : null);
    Object.assign(report, {
      state: now.state,
      error: now.error,
      drove_seconds: seconds,
      lap_timer_ms: now.hud?.lap_ms,
      position_on_lap: now.hud?.position,
      kmh: now.hud?.kmh,
      gear: now.hud?.gear,
      hud: now.hud,
      frames_per_second: mean('fps'),
      physics_steps_per_second: mean('steps_per_second'),
      work_ms_per_frame: mean('work_ms_per_frame'),
      meshes_drawn: now.perf?.meshes,
      triangles_drawn: now.perf?.triangles,
      backend: now.perf?.backend,
      dropped_seconds: now.perf?.dropped_seconds,
    });
  } else if (now.state === 'selftest') {
    report.selftest = now.selftest;
  }
  if (args.includes('--wait-checks')) {
    // the folder's cars are checked in the background: wait until every one was built once
    for (let i = 0; i < 1200 && now.content && now.content.cars_checked < now.content.cars; i++) {
      await sleep(500);
      now = await page();
    }
  }
  report.content = now.content;
  const shot = option('--screenshot');
  if (shot) {
    const { data } = await send('Page.captureScreenshot', { format: shot.toLowerCase().endsWith('.png') ? 'png' : 'jpeg', quality: 82 });
    mkdirSync(dirname(resolve(shot)), { recursive: true });
    writeFileSync(shot, Buffer.from(data, 'base64'));
    report.screenshot = shot;
  }
  report.console = messages.filter((m) => !m.startsWith('log:')).slice(0, 20);
  console.log(JSON.stringify(report, null, 2));
  await close(report.state === 'error' ? 1 : 0);
} catch (error) {
  console.error(String(error && error.stack ? error.stack : error));
  if (messages.length) console.error(messages.slice(-10).join('\n'));
  await close(1);
}
