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
//        multiplayer (the address has ?join=<server id>, see web/app.js):
//        [--chat "hello@20,bye@40"]     chat lines, each so many seconds into the drive
//        [--arrivals <file.csv>]        with &arrivals=1 in the address: when every car state
//                                       arrived at the page and when the session took it
//        [--shots <folder>] [--shot-near 60] [--shot-cars 2]
//                                       a screenshot every 2 s while that many other cars are
//                                       within that many metres (at most 8)
//        [--end kill]                   end by killing the browser (a tab that just vanishes),
//                                       not by closing it
//        [--freeze 30@60]               30 s into the drive the page is frozen for 60 s (what
//                                       a tab in the background or a sleeping laptop gets)
//        [--menu]          do not wait for a drive: the menu with its content is enough
//        [--launcher]      do not wait for a drive: AC's launcher in the page (?launcher=1) is enough
//        [--script <file.js>]   run that file's text in the page at the end (the body of an
//                          async function; what it returns is the report's `script`)
//        [--pick-folder <folder>]   with --menu: press "Open it as a file list" and answer the
//                          browser's file chooser with that folder (what `<input
//                          webkitdirectory>` does with it is the report's `picked`)
//
// Waits until the page drives (or its self test is done, with ?selftest=N), lets it drive for
// --seconds, takes a screenshot and prints the lap timer, the speed, the frame rate, the
// physics steps per second and the page's notes as JSON. A self test with `&keys=...` in the
// address reports its key presses in `selftest.presses`.

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
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
  // (as the desktop launcher's WebView2: the theme plays its sounds without a click first)
  '--autoplay-policy=no-user-gesture-required',
  // this machine only: every address that is not this machine's goes to a proxy that is not
  // there, and no name but localhost resolves (the page, the launcher's theme and the service
  // worker reach no other server, whatever they ask for)
  '--proxy-server=http://127.0.0.1:9',
  '--proxy-bypass-list=127.0.0.1;localhost;[::1]',
  '--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost, EXCLUDE 127.0.0.1',
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
    // (a browser that was killed never answers)
    if (!closed) await Promise.race([send('Browser.close'), sleep(5000)]);
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
let fileChooser = null;

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
    } else if (message.method === 'Page.fileChooserOpened') {
      fileChooser = message.params;
    } else if (message.method === 'Runtime.exceptionThrown') {
      messages.push(`exception: ${message.params.exceptionDetails.exception?.description || message.params.exceptionDetails.text}`);
    }
  };
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
  const started = Date.now();
  await send('Page.navigate', { url });

  // --flow <file.json>: steps across the page's reloads (AC's launcher starts a drive by
  // starting the page again): [{ until: "<state>" }, { js: "<expression>" }, { wait: s },
  // { shot: "<file>" }]; each step's result goes into the report's `flow`
  const flowFile = option('--flow');
  if (flowFile) {
    const flow = JSON.parse(readFileSync(resolve(flowFile), 'utf8'));
    const report = { url, flow: [] };
    let failed = false;
    for (const step of flow) {
      const at = (Date.now() - started) / 1000;
      try {
        if (step.until) {
          let now = null;
          for (;;) {
            await sleep(250);
            now = await evaluate('window.rustyac ? window.rustyac.state + "|" + (window.rustyac.error || "") : ""').catch(() => '');
            if (now.split('|')[0] === step.until) break;
            if (now.startsWith('error|')) throw new Error(`the page failed: ${now.slice(6)}`);
            if (Date.now() - started > timeout) throw new Error(`no state ${step.until} within ${timeout / 1000} s (state: ${now})`);
          }
          report.flow.push({ at, until: step.until });
        } else if (step.js) {
          report.flow.push({ at, js: step.js.slice(0, 80), value: await evaluate(step.js) });
        } else if (step.pick) {
          // { pick: "<folder>", button: "<id>" }: that button's file chooser answered with the folder
          await send('Page.setInterceptFileChooserDialog', { enabled: true });
          fileChooser = null;
          await send('Runtime.evaluate', { expression: `document.getElementById(${JSON.stringify(step.button || 'open-folder-list')}).click()`, userGesture: true });
          for (let i = 0; i < 100 && !fileChooser; i++) await sleep(100);
          if (!fileChooser) throw new Error('the page opened no file chooser');
          await send('DOM.setFileInputFiles', { files: [resolve(step.pick)], backendNodeId: fileChooser.backendNodeId });
          report.flow.push({ at, pick: step.pick });
        } else if (step.cdp) {
          // { cdp: "Input.dispatchKeyEvent", params: {...} }: real (trusted) input, as a keyboard or mouse gives it
          report.flow.push({ at, cdp: step.cdp, value: await send(step.cdp, step.params || {}) });
        } else if (step.wait) {
          await sleep(step.wait * 1000);
        } else if (step.shot) {
          const { data } = await send('Page.captureScreenshot', { format: step.shot.toLowerCase().endsWith('.png') ? 'png' : 'jpeg', quality: 80 });
          mkdirSync(dirname(resolve(step.shot)), { recursive: true });
          writeFileSync(step.shot, Buffer.from(data, 'base64'));
          report.flow.push({ at, shot: step.shot });
        }
      } catch (error) {
        report.flow.push({ at, failed: String(error.message || error) });
        failed = true;
        break;
      }
    }
    report.console = messages.filter((m) => !m.startsWith('log:')).slice(0, 30);
    console.log(JSON.stringify(report, null, 2));
    await close(failed ? 1 : 0);
  }

  const page = () => evaluate('JSON.stringify(window.rustyac ? { state: window.rustyac.state, error: window.rustyac.error, hud: window.rustyac.hud, perf: window.rustyac.perf, notes: window.rustyac.notes, content: window.rustyac.content || null, selftest: window.rustyac.selftest || null, last: window.rustyac.log.slice(-1)[0] } : null)').then((text) => JSON.parse(text));
  let now = null;
  let lastLine = '';
  for (;;) {
    await sleep(500);
    now = await page().catch(() => null);
    if (now && now.last && now.last !== lastLine && process.stderr.isTTY) process.stderr.write(`\r${now.last.slice(0, 110).padEnd(110)}`);
    lastLine = now?.last || lastLine;
    if (now && ['driving', 'selftest', 'error'].includes(now.state)) break;
    if (now && now.state === 'menu' && args.includes('--menu')) break;
    if (now && now.state === 'launcher' && args.includes('--launcher')) break;
    if (Date.now() - started > timeout) throw new Error(`the page did not start driving within ${timeout / 1000} s (state: ${now?.state}, last: ${now?.last})`);
  }
  if (process.stderr.isTTY) process.stderr.write('\n');
  const report = { url, browser: chrome, load_seconds: (Date.now() - started) / 1000, state: now.state, error: now.error, notes: now.notes };
  if (now.state === 'driving') {
    const samples = [];
    const drivingSince = Date.now();
    const until = drivingSince + seconds * 1000;
    let seen = null;
    // multiplayer: chat lines by the clock, screenshots while other cars are near
    const chat = option('--chat', '')
      .split(',')
      .filter(Boolean)
      .map((item) => ({ text: item.slice(0, item.lastIndexOf('@')), at: Number(item.slice(item.lastIndexOf('@') + 1)) }))
      .sort((a, b) => a.at - b.at);
    const shots = option('--shots');
    const shotNear = Number(option('--shot-near', '60'));
    const shotCars = Number(option('--shot-cars', '2'));
    let lastShot = 0;
    report.chat_sent = [];
    report.shots = [];
    report.others_seen = 0;
    const [freezeAt, freezeFor] = option('--freeze', '').split('@').map(Number);
    while (Date.now() < until) {
      await sleep(250);
      now = await page();
      const drove = (Date.now() - drivingSince) / 1000;
      if (freezeFor && !report.frozen && drove >= freezeAt) {
        await send('Page.setWebLifecycleState', { state: 'frozen' });
        await sleep(freezeFor * 1000);
        await send('Page.setWebLifecycleState', { state: 'active' });
        report.frozen = { at: drove, seconds: freezeFor };
      }
      while (chat.length && drove >= chat[0].at) {
        const line = chat.shift();
        report.chat_sent.push({ text: line.text, at: drove, sent: await evaluate(`window.rustyac.chat(${JSON.stringify(line.text)})`) });
      }
      if (now.hud?.online) {
        const others = JSON.parse(await evaluate('JSON.stringify(window.rustyac.others())'));
        const there = others.filter((car) => car.there);
        report.others_seen = Math.max(report.others_seen, there.length);
        const near = there.filter((car) => car.distance < shotNear);
        if (shots && near.length >= shotCars && report.shots.length < 8 && Date.now() - lastShot > 2000) {
          lastShot = Date.now();
          const { data } = await send('Page.captureScreenshot', { format: 'jpeg', quality: 80 });
          mkdirSync(resolve(shots), { recursive: true });
          const file = resolve(shots, `near_${report.shots.length + 1}.jpg`);
          writeFileSync(file, Buffer.from(data, 'base64'));
          report.shots.push({ file, at: drove, near: near.map((car) => `slot ${car.slot} ${car.distance} m`), drawn: now.hud.online.cars_drawn, boxes: now.hud.online.cars_as_boxes });
        }
      }
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
    if (now.hud?.online) {
      report.online = now.hud.online;
      report.others = JSON.parse(await evaluate('JSON.stringify(window.rustyac.others())'));
      report.join = JSON.parse(await evaluate('JSON.stringify(window.rustyac.join || null)'));
      report.log = JSON.parse(await evaluate('JSON.stringify(window.rustyac.log.slice(-60))'));
      const arrivals = option('--arrivals');
      if (arrivals) {
        mkdirSync(dirname(resolve(arrivals)), { recursive: true });
        writeFileSync(arrivals, await evaluate('window.rustyac.arrivals()'));
        report.arrivals = arrivals;
      }
    }
  } else if (now.state === 'selftest') {
    report.selftest = now.selftest;
  }
  const pick = option('--pick-folder');
  if (pick) {
    // the page's second way to open the folder: the browser's file chooser, answered here
    await send('Page.setInterceptFileChooserDialog', { enabled: true });
    const asked = Date.now();
    await send('Runtime.evaluate', { expression: "document.getElementById('open-folder-list').click()", userGesture: true });
    for (let i = 0; i < 100 && !fileChooser; i++) await sleep(100);
    if (!fileChooser) throw new Error('the page opened no file chooser');
    report.picked = { folder: resolve(pick), chooser_mode: fileChooser.mode };
    try {
      await send('DOM.setFileInputFiles', { files: [resolve(pick)], backendNodeId: fileChooser.backendNodeId });
      report.picked.accepted_after_seconds = (Date.now() - asked) / 1000;
    } catch (error) {
      report.picked.refused = String(error.message || error);
    }
    for (let i = 0; i < 2 * (timeout / 1000) && !report.picked.refused; i++) {
      await sleep(500);
      now = await page();
      const listed = JSON.parse(await evaluate('JSON.stringify(window.rustyac.listed || null)'));
      if (listed) Object.assign(report.picked, listed);
      if ((listed && listed.ready_ms) || now.state === 'error') break;
    }
    report.picked.line = await evaluate("document.getElementById('content-line').textContent");
    report.state = now.state;
    report.error = now.error;
  }
  if (args.includes('--wait-checks')) {
    // the folder's cars are checked in the background: wait until every one was built once
    for (let i = 0; i < 1200 && now.content && now.content.cars_checked < now.content.cars; i++) {
      await sleep(500);
      now = await page();
    }
  }
  report.content = now.content;
  const script = option('--script');
  if (script) report.script = await evaluate(`(async () => { ${readFileSync(resolve(script), 'utf8')}\n})()`);
  const shot = option('--screenshot');
  if (shot) {
    const { data } = await send('Page.captureScreenshot', { format: shot.toLowerCase().endsWith('.png') ? 'png' : 'jpeg', quality: 82 });
    mkdirSync(dirname(resolve(shot)), { recursive: true });
    writeFileSync(shot, Buffer.from(data, 'base64'));
    report.screenshot = shot;
  }
  report.console = messages.filter((m) => !m.startsWith('log:')).slice(0, 20);
  console.log(JSON.stringify(report, null, 2));
  if (option('--end') === 'kill') {
    // a tab that vanishes: the page gets no chance to say goodbye
    browser.kill();
  }
  await close(report.state === 'error' ? 1 : 0);
} catch (error) {
  console.error(String(error && error.stack ? error.stack : error));
  if (messages.length) console.error(messages.slice(-10).join('\n'));
  await close(1);
}
