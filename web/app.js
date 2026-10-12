// SPDX-License-Identifier: GPL-3.0-or-later

// rustyAC's browser preview: the page around the WebAssembly build (crates/rustyac-web).
//
// This file does what a browser makes asynchronous: it finds the content (a preview pack
// behind a URL, or the Assetto Corsa folder the player picks), hands the files of the chosen
// car and track to the wasm side, and then calls it once per frame with the time, the keys
// that are down and the gamepad. The simulation, the loaders and the picture are Rust.
//
// URL options (all optional):
//   ?pack=<url>        the preview pack's folder (default: preview/ next to the page)
//   ?car=<id>&track=<id>[&layout=<l>]   preselect
//   ?spawn=pit|hotlap|start             where the car starts
//   ?go=1              start driving as soon as the content is there (no click)
//   ?autodrive=1       a line follower drives (starts from the hot-lap point)
//   ?backend=webgl     do not use WebGPU
//   ?tex=<pixels>      longest texture side kept (default: 1024, or 512 where textures are unpacked)
//   ?msaa=1            no multisampling
//   ?nocache=1         do not keep the pack's files in the browser's storage
//   ?selftest=<steps>  with ?go=1&autodrive=1: do not start the frame loop; run that many
//                      physics steps at once and report the car's state hash in
//                      `window.rustyac.selftest` (the desktop prints the same number:
//                      `cargo run --release -p rustyac-web --example selftest`)
//   ?keys=<list>       with ?selftest: keys pressed on the way, through the page's own key
//                      handler: `KeyT@300,Shift+KeyT@600,BracketRight@900` presses T after
//                      300 steps and so on (`KeyboardEvent.code` names). What the display
//                      showed after each press is in `window.rustyac.selftest.presses`. The
//                      desktop's selftest takes the same list with `--keys`.
//   ?testfolder=<url>  the test hook of the folder picker: reads the folder over HTTP from
//                      `web/serve.py --ac <folder>` through the same code the picker feeds
//   ?server=<ws url>   multiplayer: one more server in the list, by its gateway's address
//                      (`ws://127.0.0.1:9601/net/spa`); `servers.json` lists the usual ones
//   ?join=<id>         join that server as soon as the content is there (no click); with
//                      &name=<driver name> &mpcar=<model> &password=<p>
//   ?cars=<metres>     how far other players' cars are drawn with their model (default 150)
//   ?arrivals=1        keep when every car state arrived (`window.rustyac.arrivals()`)

import init, * as rac from './pkg/rustyac_web.js';

const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);
const canvas = $('view');

const state = {
  source: null, // where the files come from: { kind, sizes: Map(path -> bytes), open(path) }
  cars: [], // { id, name, refused }
  tracks: [], // { track, layout, name, refused }
  game: null,
  loading: false,
  running: false,
  paused: false,
  testing: false, // the self test runs: its keys count although nothing is being driven
  servers: [], // multiplayer: { id, name, url, cars }
  online: null, // on a server: { ws, link, server }
};

// what a test (or the curious) can read from outside
window.rustyac = { state: 'starting', error: null, hud: null, perf: null, notes: '', log: [] };

function setStatus(text) {
  $('status').textContent = text;
  window.rustyac.log.push(text);
}

function fail(error) {
  const text = String(error && error.message ? error.message : error);
  console.error(error);
  window.rustyac.state = 'error';
  window.rustyac.error = text;
  $('status').textContent = '';
  $('refusal').hidden = false;
  $('refusal').textContent = text;
  $('progress').hidden = true;
  $('drive').disabled = !state.source;
}

const megabytes = (bytes) => (bytes / 1e6).toFixed(bytes < 1e7 ? 1 : 0) + ' MB';
const nextPaint = () => new Promise((resolve) => requestAnimationFrame(() => setTimeout(resolve, 0)));

// ---------------------------------------------------------------- the pack behind a URL

async function packCache() {
  if (params.has('nocache') || !navigator.storage || !navigator.storage.getDirectory) return null;
  try {
    const root = await navigator.storage.getDirectory();
    return await root.getDirectoryHandle('rustyac-pack-1', { create: true });
  } catch {
    return null;
  }
}

async function openPack(base) {
  const response = await fetch(base + 'manifest.json', { cache: 'no-cache' });
  if (!response.ok) return null;
  const manifest = await response.json();
  if (manifest.format !== 'rustyac-preview-pack-1') throw new Error(`${base}manifest.json is not a rustyAC preview pack`);
  const entries = new Map(manifest.files.map((f) => [f.path.toLowerCase(), f]));
  const cache = await packCache();
  return {
    kind: 'pack',
    manifest,
    sizes: new Map(manifest.files.map((f) => [f.path, f.bytes])),
    cars: manifest.cars.map((c) => ({ id: c.id, name: c.name, refused: null, bytes: c.bytes })),
    tracks: manifest.tracks.map((t) => ({ track: t.id, layout: t.layout, name: t.name, refused: null, bytes: t.bytes })),
    async open(path) {
      const entry = entries.get(path.toLowerCase());
      if (!entry) throw new Error(`${path} is not in the preview pack`);
      // a file fetched on an earlier visit is in the browser's own storage, under its hash
      if (cache) {
        try {
          const file = await (await cache.getFileHandle(entry.sha256)).getFile();
          if (file.size === entry.bytes) return { size: file.size, stream: file.stream(), sha256: '', from: 'cache' };
        } catch {
          // not cached yet
        }
      }
      const got = await fetch(base + entry.path.split('/').map(encodeURIComponent).join('/'));
      if (!got.ok || !got.body) throw new Error(`${entry.path}: the server answered ${got.status}`);
      let keep = null;
      if (cache) {
        try {
          keep = await (await cache.getFileHandle(entry.sha256, { create: true })).createWritable();
        } catch {
          keep = null;
        }
      }
      return {
        size: entry.bytes,
        stream: got.body,
        sha256: entry.sha256,
        from: 'network',
        keep,
        async discard() {
          if (cache) await cache.removeEntry(entry.sha256).catch(() => {});
        },
      };
    },
  };
}

// ---------------------------------------------------------------- the player's own folder

// A folder handle over HTTP, for the test hook: the same three calls the picker's handle has.
class HttpFolder {
  constructor(url, name) {
    this.kind = 'directory';
    this.url = url.endsWith('/') ? url : url + '/';
    this.name = name;
  }

  async *entries() {
    const list = await (await fetch(this.url + '?list=1')).json();
    for (const entry of list) {
      const url = this.url + encodeURIComponent(entry.name);
      yield [entry.name, entry.dir ? new HttpFolder(url, entry.name) : { kind: 'file', name: entry.name, getFile: async () => (await fetch(url)).blob() }];
    }
  }
}

async function children(folder) {
  const out = new Map();
  for await (const [name, handle] of folder.entries()) out.set(name.toLowerCase(), { name, handle });
  return out;
}

// Walks the parts of the folder a drive can need and returns Map(path -> file handle).
// Skins, sounds, previews and the like are never looked at.
async function scanFolder(root, report) {
  const top = await children(root);
  if (!top.has('content') || !top.has('system')) {
    throw new Error(`"${root.name}" is not Assetto Corsa's folder: it has no content and system folders in it. Pick the folder named assettocorsa.`);
  }
  const files = new Map();
  const add = (path, handle) => files.set(path, handle);
  const walk = async (folder, path, depth, skip) => {
    for (const [lower, { name, handle }] of await children(folder)) {
      if (handle.kind === 'file') add(`${path}/${name}`, handle);
      else if (depth > 0 && !skip.includes(lower)) await walk(handle, `${path}/${name}`, depth - 1, skip);
    }
  };
  const content = await children(top.get('content').handle);
  const cars = content.has('cars') ? await children(content.get('cars').handle) : new Map();
  let done = 0;
  for (const { name, handle } of cars.values()) {
    if (handle.kind !== 'directory') continue;
    const inside = await children(handle);
    for (const [lower, entry] of inside) {
      if (entry.handle.kind === 'file' && (lower === 'data.acd' || lower.endsWith('.kn5'))) add(`content/cars/${name}/${entry.name}`, entry.handle);
    }
    // a car without an archive keeps its data as plain files (the SDK's cars, unpacked mods)
    if (!inside.has('data.acd') && inside.has('data')) await walk(inside.get('data').handle, `content/cars/${name}/data`, 0, []);
    if (inside.has('ui')) {
      const ui = await children(inside.get('ui').handle);
      if (ui.has('ui_car.json')) add(`content/cars/${name}/ui/ui_car.json`, ui.get('ui_car.json').handle);
    }
    report(`reading the folder: ${++done} cars`);
  }
  const tracks = content.has('tracks') ? await children(content.get('tracks').handle) : new Map();
  done = 0;
  for (const { name, handle } of tracks.values()) {
    if (handle.kind !== 'directory') continue;
    await walk(handle, `content/tracks/${name}`, 3, ['skins', 'extension', 'sfx', 'texture']);
    report(`reading the folder: ${cars.size} cars, ${++done} tracks`);
  }
  const system = await children(top.get('system').handle);
  if (system.has('data')) await walk(system.get('data').handle, 'system/data', 0, []);
  return files;
}

// The small files the lists are made from: every track's models file, menu entry and
// surfaces, every car's menu entry. (A car's data is read when it is checked.)
function isIndexFile(path) {
  const lower = path.toLowerCase();
  if (lower.startsWith('system/data/')) return lower.endsWith('.ini');
  if (lower.startsWith('content/cars/')) return lower.endsWith('/ui/ui_car.json');
  if (!lower.startsWith('content/tracks/')) return false;
  const name = lower.slice(lower.lastIndexOf('/') + 1);
  return name === 'ui_track.json' || name === 'dlc_ui_track.json' || name === 'surfaces.ini' || (name.startsWith('models') && name.endsWith('.ini'));
}

async function openFolder(root) {
  const report = (text) => {
    $('content-line').textContent = text;
  };
  const handles = await scanFolder(root, report);
  const sizes = new Map();
  rac.fs_list([...handles.keys()].join('\n'));
  const source = {
    kind: 'folder',
    name: root.name,
    sizes,
    find(path) {
      const handle = handles.get(path) || [...handles].find(([p]) => p.toLowerCase() === path.toLowerCase())?.[1];
      if (!handle) throw new Error(`${path} is not in the folder`);
      return handle;
    },
    // (asked before a load, so the progress bar knows its total)
    async size(path) {
      if (!sizes.has(path)) sizes.set(path, (await this.find(path).getFile()).size);
      return sizes.get(path);
    },
    async open(path) {
      const file = await this.find(path).getFile();
      sizes.set(path, file.size);
      return { size: file.size, stream: file.stream(), sha256: '', from: 'folder' };
    },
  };
  const index = [...handles.keys()].filter(isIndexFile);
  let read = 0;
  for (const path of index) {
    const file = await handles.get(path).getFile();
    rac.fs_put(path, new Uint8Array(await file.arrayBuffer()));
    if (++read % 20 === 0) report(`reading the folder: ${read} of ${index.length} small files`);
  }
  const found = JSON.parse(rac.index(false));
  source.cars = found.cars;
  source.tracks = found.tracks;
  // which cars load: each car's data (a few hundred kB) is read and the car is built once,
  // in the background, while the lists can already be used
  source.checking = (async () => {
    let checked = 0;
    for (const car of source.cars) {
      const data = [...handles.keys()].filter((p) => p.toLowerCase().startsWith(`content/cars/${car.id.toLowerCase()}/data`));
      try {
        for (const path of data) {
          if (!rac.fs_has(path)) rac.fs_put(path, new Uint8Array(await (await handles.get(path).getFile()).arrayBuffer()));
        }
        car.refused = rac.check_car(car.id) || null;
      } catch (error) {
        car.refused = String(error);
      }
      checked += 1;
      source.checked = checked;
      if (state.source === source) {
        markRefused();
        report(`${root.name}: ${source.cars.length} cars (${checked} checked), ${source.tracks.length} tracks and layouts`);
      }
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
  })();
  return source;
}

// The picked folder is remembered (IndexedDB can hold the handle itself), so the next visit
// needs one click at most, or none where the browser kept the permission.
function handleStore() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('rustyac', 1);
    request.onupgradeneeded = () => request.result.createObjectStore('handles');
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function rememberFolder(handle) {
  try {
    const db = await handleStore();
    db.transaction('handles', 'readwrite').objectStore('handles').put(handle, 'ac');
  } catch {
    // the folder just has to be picked again next time
  }
}

async function rememberedFolder() {
  try {
    const db = await handleStore();
    return await new Promise((resolve) => {
      const request = db.transaction('handles').objectStore('handles').get('ac');
      request.onsuccess = () => resolve(request.result || null);
      request.onerror = () => resolve(null);
    });
  } catch {
    return null;
  }
}

// `<input webkitdirectory>` (Firefox, Safari): every file of the folder as a list, with paths.
function folderFromFiles(list) {
  const all = [...list];
  const marker = all.map((f) => f.webkitRelativePath.toLowerCase()).find((p) => p.includes('/content/cars/') || p.includes('/content/tracks/'));
  if (!marker) throw new Error("that is not Assetto Corsa's folder: no content/cars or content/tracks in it");
  const prefix = marker.slice(0, marker.indexOf('/content/') + 1).length;
  // the same shape the picker's handle has
  const tree = { kind: 'directory', name: all[0].webkitRelativePath.slice(0, prefix - 1), items: new Map() };
  for (const file of all) {
    const parts = file.webkitRelativePath.slice(prefix).split('/');
    let folder = tree;
    for (const part of parts.slice(0, -1)) {
      if (!folder.items.has(part)) folder.items.set(part, { kind: 'directory', name: part, items: new Map() });
      folder = folder.items.get(part);
    }
    folder.items.set(parts[parts.length - 1], { kind: 'file', name: parts[parts.length - 1], getFile: async () => file });
  }
  const withEntries = (folder) => {
    folder.entries = async function* () {
      for (const [name, item] of folder.items) yield [name, item];
    };
    for (const item of folder.items.values()) if (item.kind === 'directory') withEntries(item);
    return folder;
  };
  return withEntries(tree);
}

// ---------------------------------------------------------------- the lists

function fillLists() {
  const source = state.source;
  state.cars = source.cars;
  state.tracks = source.tracks;
  const car = $('car');
  const track = $('track');
  car.replaceChildren(...state.cars.map((c) => new Option(c.name === c.id ? c.id : `${c.name} (${c.id})`, c.id)));
  track.replaceChildren(...state.tracks.map((t) => new Option(t.layout ? `${t.name} (${t.track} / ${t.layout})` : `${t.name} (${t.track})`, `${t.track}|${t.layout}`)));
  car.disabled = track.disabled = false;
  const wantCar = params.get('car');
  const wantTrack = params.get('track');
  const firstCar = state.cars.find((c) => (wantCar ? c.id === wantCar : !c.refused));
  const firstTrack = state.tracks.find((t) => (wantTrack ? t.track === wantTrack && (!params.has('layout') || t.layout === params.get('layout')) : !t.refused));
  if (firstCar) car.value = firstCar.id;
  if (firstTrack) track.value = `${firstTrack.track}|${firstTrack.layout}`;
  markRefused();
}

function markRefused() {
  for (const option of $('car').options) {
    const car = state.cars.find((c) => c.id === option.value);
    option.disabled = !!(car && car.refused);
  }
  for (const option of $('track').options) {
    const track = state.tracks.find((t) => `${t.track}|${t.layout}` === option.value);
    option.disabled = !!(track && track.refused);
  }
  const refused = [...state.cars.filter((c) => c.refused).map((c) => `${c.id}: ${c.refused}`), ...state.tracks.filter((t) => t.refused).map((t) => `${t.track}${t.layout ? ' / ' + t.layout : ''}: ${t.refused}`)];
  const list = $('refused-list');
  list.hidden = refused.length === 0;
  list.querySelector('summary').textContent = `${refused.length} cars and tracks that do not load, and why`;
  list.querySelector('ul').replaceChildren(
    ...refused.map((text) => {
      const item = document.createElement('li');
      item.textContent = text;
      return item;
    }),
  );
  window.rustyac.content = { kind: state.source.kind, cars: state.cars.length, cars_checked: state.source.kind === 'pack' ? state.cars.length : state.source.checked || 0, tracks: state.tracks.length, refused };
  const chosen = state.cars.find((c) => c.id === $('car').value);
  const chosenTrack = state.tracks.find((t) => `${t.track}|${t.layout}` === $('track').value);
  const why = (chosen && chosen.refused) || (chosenTrack && chosenTrack.refused);
  $('drive').disabled = !chosen || !chosenTrack || !!why || state.running || state.loading;
}

async function useSource(source, line) {
  state.source = source;
  $('content-line').textContent = line;
  fillLists();
  window.rustyac.state = 'menu';
}

// ---------------------------------------------------------------- files into the wasm side

async function loadFiles(paths, done) {
  for (const path of paths) {
    const file = await state.source.open(path);
    let writer = new rac.FileWriter(file.size, !!file.sha256);
    let keep = file.keep || null;
    try {
      const reader = file.stream.getReader();
      for (;;) {
        const { done: end, value } = await reader.read();
        if (end) break;
        writer.write(value);
        if (keep) {
          try {
            await keep.write(value);
          } catch {
            // the browser's storage is full or gone: go on without keeping the file
            await keep.abort().catch(() => {});
            keep = null;
            if (file.discard) await file.discard();
          }
        }
        done(value.length, path, file.from);
      }
      // (commit takes the writer; it checks the hash before the file is mounted or kept)
      const committing = writer;
      writer = null;
      committing.commit(path, file.sha256 || '');
      if (keep) await keep.close().catch(() => {});
    } catch (error) {
      if (writer) writer.free();
      if (keep) await keep.abort().catch(() => {});
      if (file.discard) await file.discard();
      throw error;
    }
  }
}

async function drive() {
  const car = $('car').value;
  const [track, layout] = $('track').value.split('|');
  const autodrive = params.get('autodrive') === '1';
  const spawn = params.get('spawn') || (autodrive ? 'hotlap' : $('spawn').value);
  state.loading = true;
  $('drive').disabled = true;
  $('refusal').hidden = true;
  $('notes').hidden = true;
  window.rustyac.state = 'loading';
  try {
    // the files: the wasm side says which it still needs; more can be named once the first
    // ones are there (the model a lods.ini points to), hence the rounds
    const started = performance.now();
    let loaded = 0;
    for (let round = 0; round < 8; round++) {
      const wanted = rac.files_wanted(car, track, layout).split('\n').filter(Boolean);
      if (!wanted.length) break;
      let total = 0;
      for (const path of wanted) total += state.source.size ? await state.source.size(path) : state.source.sizes.get(path) || 0;
      let got = 0;
      $('progress').hidden = false;
      await loadFiles(wanted, (bytes, path, from) => {
        got += bytes;
        loaded += bytes;
        $('progress-bar').style.width = total ? `${Math.min(100, (got / total) * 100).toFixed(1)}%` : '100%';
        setStatus(`${from === 'cache' ? 'from this browser\'s storage' : from === 'folder' ? 'reading' : 'downloading'}: ${path.slice(path.lastIndexOf('/') + 1)} (${megabytes(got)}${total ? ' of ' + megabytes(total) : ''})`);
      });
    }
    const fileSeconds = (performance.now() - started) / 1000;
    $('progress').hidden = true;
    setStatus('building the track and the car (a large track takes some seconds; the page stands still meanwhile) ...');
    await nextPaint();
    resizeCanvas();
    const built = performance.now();
    let backend = params.get('backend') || $('backend').value;
    if (backend !== 'webgl') {
      const adapter = navigator.gpu ? await navigator.gpu.requestAdapter().catch(() => null) : null;
      if (!adapter) backend = 'webgl';
    }
    const game = await rac.Game.create(canvas, car, track, layout, spawn, $('auto-shifter').checked, backend, Number(params.get('tex') || 0), Number(params.get('msaa') || 4));
    rac.fs_forget_models();
    state.game = game;
    game.set_autodrive(autodrive);
    const notes = `${megabytes(loaded)} of files in ${fileSeconds.toFixed(1)} s, built in ${((performance.now() - built) / 1000).toFixed(1)} s\n${game.notes()}`;
    window.rustyac.notes = notes;
    $('notes').textContent = notes;
    $('notes').hidden = false;
    window.rustyac.bindings = game.bindings();
    if (params.has('selftest')) {
      const steps = Number(params.get('selftest'));
      // ?keys: `Shift+KeyT@600` = that key after 600 steps
      const script = (params.get('keys') || '')
        .split(',')
        .filter(Boolean)
        .map((item) => {
          // (a `+` in an address arrives as a space)
          const [name, at] = item.replace(/ /g, '+').split('@');
          // (a step that is not a plain whole number counts as 0, as in the desktop's self test)
          return { key: name, at: Math.min(steps, /^\d+$/.test(at || '') ? Number(at) : 0) };
        })
        .sort((a, b) => a.at - b.at);
      const presses = [];
      const before = performance.now();
      state.testing = true;
      $('hud').hidden = false;
      let done = 0;
      for (const { key, at } of script) {
        // (a key at the step of the one before it comes a step later: that step has been run)
        game.run_steps(Math.max(0, at - done));
        done = Math.max(at, done);
        showHud(JSON.parse(game.hud()));
        const was = aidsShown();
        const prevented = window.rustyac.press(key);
        // the command runs before the next physics step
        if (done < steps) {
          game.run_steps(1);
          done += 1;
        }
        showHud(JSON.parse(game.hud()));
        presses.push({ key, at, prevented, before: was, after: aidsShown(), note: $('hud-note').textContent });
      }
      game.run_steps(steps - done);
      state.testing = false;
      const seconds = (performance.now() - before) / 1000;
      showHud(JSON.parse(game.hud()));
      window.rustyac.selftest = { car, track, layout, steps, hash: game.state_hash(), seconds, steps_per_second: steps / seconds, hud: JSON.parse(game.hud()), presses };
      setStatus(`self test: ${steps} steps in ${seconds.toFixed(2)} s, state ${window.rustyac.selftest.hash}`);
      window.rustyac.state = 'selftest';
      state.loading = false;
      return;
    }
    state.loading = false;
    startDriving();
  } catch (error) {
    state.loading = false;
    fail(error);
  }
}

// ---------------------------------------------------------------- multiplayer

// The player's id on servers: a number made once from random bits and kept in this browser
// (Assetto Corsa sends the Steam id; a browser has none). 8 and sixteen digits, as the
// desktop's own id: no Steam id starts like that.
function playerId() {
  const make = () => {
    const digits = crypto.getRandomValues(new Uint8Array(16));
    return '8' + [...digits].map((d) => d % 10).join('');
  };
  try {
    let id = localStorage.getItem('rustyac.player-id');
    if (!/^8\d{16}$/.test(id || '')) {
      id = make();
      localStorage.setItem('rustyac.player-id', id);
    }
    return id;
  } catch {
    // no storage (a private window): an id for this visit
    state.visitId = state.visitId || make();
    return state.visitId;
  }
}

// `/net/spa` -> `wss://<this site>/net/spa`; a whole ws:// or wss:// address stays.
function gatewayUrl(url) {
  if (/^wss?:\/\//.test(url)) return url;
  return (location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + (url.startsWith('/') ? url : '/' + url);
}

function setMultiplayer(text) {
  $('mp-status').textContent = text;
  window.rustyac.log.push(text);
}

function markServers() {
  const server = state.servers.find((s) => s.id === $('mp-server').value);
  const cars = $('mp-car');
  const chosen = cars.value;
  // (a server gives a slot by the car's model: one has to be named. "Any free car" tries
  // the server's models one after the other; a server that is not in servers.json with its
  // cars is tried with the cars that are here)
  const models = server && server.cars && server.cars.length ? server.cars : state.cars.map((c) => c.id);
  cars.replaceChildren(new Option('any free car', ''), ...models.map((id) => new Option(state.cars.find((c) => c.id === id)?.name || id, id)));
  const wanted = chosen || params.get('mpcar') || '';
  cars.value = [...cars.options].some((o) => o.value === wanted) ? wanted : '';
  $('mp-join').disabled = !server || !state.source || state.loading || state.running || !!state.game;
}

async function loadServers() {
  let servers = [];
  try {
    const response = await fetch('servers.json', { cache: 'no-cache' });
    if (response.ok) servers = (await response.json()).servers || [];
  } catch {
    // a site without the file has no server list
  }
  if (params.get('server')) servers.push({ id: 'address', name: `the address bar's server (${params.get('server')})`, url: params.get('server'), cars: [] });
  state.servers = servers.filter((s) => s && s.id && s.url);
  $('mp-server').replaceChildren(...state.servers.map((s) => new Option(s.name || s.id, s.id)));
  $('mp-server').disabled = state.servers.length === 0;
  if (params.get('join') && state.servers.some((s) => s.id === params.get('join'))) $('mp-server').value = params.get('join');
  try {
    $('mp-name').value = params.get('name') || localStorage.getItem('rustyac.player-name') || 'rustyAC driver';
  } catch {
    $('mp-name').value = params.get('name') || 'rustyAC driver';
  }
  if (params.get('password')) $('mp-password').value = params.get('password');
  if (!state.servers.length) setMultiplayer('this site lists no server (servers.json)');
  markServers();
  if (params.get('mpcar')) $('mp-car').value = params.get('mpcar');
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Joins the chosen server: the WebSocket to its gateway, the handshake and the car list (the
// wasm side's client, the desktop's own), then the files of what the server runs, then the
// drive.
async function joinServer() {
  const server = state.servers.find((s) => s.id === $('mp-server').value);
  if (!server || state.loading || state.game) return;
  const name = $('mp-name').value.trim() || 'rustyAC driver';
  try {
    localStorage.setItem('rustyac.player-name', name);
  } catch {
    // not kept, then
  }
  state.loading = true;
  $('mp-join').disabled = true;
  $('drive').disabled = true;
  $('refusal').hidden = true;
  $('notes').hidden = true;
  window.rustyac.state = 'joining';
  let ws = null;
  let join = null;
  let link = null;
  try {
    const url = gatewayUrl(server.url);
    // one try: a WebSocket to the gateway, the handshake for this model, the car list
    const attempt = async (car) => {
      setMultiplayer(`connecting to ${server.name || server.id} ...`);
      ws = new WebSocket(url);
      ws.binaryType = 'arraybuffer';
      const socket = ws;
      const made = new rac.NetLink((bytes) => socket.send(bytes));
      if (params.has('arrivals')) made.record(true);
      socket.onmessage = (event) => {
        if (event.data instanceof ArrayBuffer) made.push(new Uint8Array(event.data));
      };
      socket.onclose = (event) => made.lost(event.reason || (event.code === 1006 ? 'the connection to the gateway broke' : `the gateway closed the connection (code ${event.code})`));
      await new Promise((resolve, reject) => {
        socket.onopen = resolve;
        socket.onerror = () => reject(new Error(`no connection to ${url}: the gateway is not running there, or it does not let this page's address in`));
      });
      setMultiplayer(`asking ${server.name || server.id} for a ${car} ...`);
      join = rac.NetJoin.start(made, playerId(), name, car, $('mp-password').value);
      for (;;) {
        const status = JSON.parse(join.status());
        if (status.stage === 'done') {
          link = made;
          return status;
        }
        if (status.stage === 'error') {
          join = null;
          socket.close(1000);
          throw new Error(status.error);
        }
        await sleep(25);
      }
    };
    const candidates = $('mp-car').value ? [$('mp-car').value] : [...$('mp-car').options].map((o) => o.value).filter(Boolean);
    if (!candidates.length) throw new Error('no car to ask the server for: the content has no car');
    let status = null;
    for (const [index, car] of candidates.entries()) {
      try {
        status = await attempt(car);
        break;
      } catch (error) {
        // no free slot with this model: the next one (any other refusal ends it)
        if (index + 1 === candidates.length || !/no available slots/i.test(String(error.message))) throw error;
      }
    }
    // what the server runs has to be here: its track, the car it gave, the other cars
    const models = [...new Set([status.car, ...status.cars])];
    const track = state.tracks.find((t) => t.track.toLowerCase() === status.track.toLowerCase() && (t.layout || '') === (status.layout || ''));
    const missing = models.filter((m) => !state.cars.some((c) => c.id.toLowerCase() === m.toLowerCase()));
    const where = state.source.kind === 'pack' ? "this site's preview pack" : 'the folder you opened';
    const what = `${status.server} runs the track ${status.track}${status.layout ? ' / ' + status.layout : ''} with the cars ${models.join(', ')}`;
    if (!track) throw new Error(`${what}. The track is not in ${where}, so the server cannot be joined from here.`);
    if (missing.length) throw new Error(`${what}. Not in ${where}: ${missing.join(', ')}. Every car of a server has to be there (the others' cars are in the picture and in the physics), so the server cannot be joined from here.`);
    const refused = [track.refused, ...models.map((m) => state.cars.find((c) => c.id.toLowerCase() === m.toLowerCase()).refused)].filter(Boolean);
    if (refused.length) throw new Error(`${what}, and rustyAC cannot load all of it: ${refused.join('; ')}`);
    setMultiplayer(`${status.server}: slot ${status.slot}, ${status.car} at ${status.track}, session ${status.session}; ${status.drivers.length ? 'there now: ' + status.drivers.join(', ') : 'nobody else is there yet'}`);
    window.rustyac.join = status;

    // the files: of every car on the server, of the track, and the ones the server wants
    // checksums of (they are worked out here, from the same files)
    const started = performance.now();
    let loaded = 0;
    const cars = models.map((m) => state.cars.find((c) => c.id.toLowerCase() === m.toLowerCase()).id);
    for (let round = 0; round < 8; round++) {
      const wanted = new Set();
      for (const car of cars) for (const path of rac.files_wanted(car, track.track, track.layout).split('\n')) if (path) wanted.add(path);
      for (const path of status.checksum_files) if (!rac.fs_has(path)) wanted.add(path);
      if (!wanted.size) break;
      let total = 0;
      for (const path of wanted) total += state.source.size ? await state.source.size(path) : state.source.sizes.get(path) || 0;
      let got = 0;
      $('progress').hidden = false;
      await loadFiles([...wanted], (bytes, path, from) => {
        got += bytes;
        loaded += bytes;
        $('progress-bar').style.width = total ? `${Math.min(100, (got / total) * 100).toFixed(1)}%` : '100%';
        setStatus(`${from === 'cache' ? "from this browser's storage" : from === 'folder' ? 'reading' : 'downloading'}: ${path.slice(path.lastIndexOf('/') + 1)} (${megabytes(got)}${total ? ' of ' + megabytes(total) : ''})`);
      });
    }
    const fileSeconds = (performance.now() - started) / 1000;
    $('progress').hidden = true;
    setStatus('building the track and the cars (a large track takes some seconds; the page stands still meanwhile) ...');
    await nextPaint();
    resizeCanvas();
    const built = performance.now();
    let backend = params.get('backend') || $('backend').value;
    if (backend !== 'webgl') {
      const adapter = navigator.gpu ? await navigator.gpu.requestAdapter().catch(() => null) : null;
      if (!adapter) backend = 'webgl';
    }
    const autodrive = params.get('autodrive') === '1';
    const spawn = params.get('spawn') || 'pit';
    // (create_online takes the join: from here on the game owns the session)
    const joining = join;
    join = null;
    const game = await rac.Game.create_online(canvas, joining, spawn, $('auto-shifter').checked, backend, Number(params.get('tex') || 0), Number(params.get('msaa') || 4), Number(params.get('cars') || 0));
    rac.fs_forget_models();
    state.game = game;
    state.online = { ws, link, server };
    game.set_autodrive(autodrive);
    const notes = `${megabytes(loaded)} of files in ${fileSeconds.toFixed(1)} s, built in ${((performance.now() - built) / 1000).toFixed(1)} s\n${game.notes()}`;
    window.rustyac.notes = notes;
    $('notes').textContent = notes;
    $('notes').hidden = false;
    window.rustyac.bindings = game.bindings();
    state.loading = false;
    startDriving();
  } catch (error) {
    state.loading = false;
    if (join) {
      try {
        join.abandon();
      } catch {
        // the link is gone already
      }
    }
    // (a moment for the goodbye to leave before the socket closes)
    if (ws) setTimeout(() => ws.close(1000), 50);
    setMultiplayer('');
    fail(error);
    markServers();
  }
}

// Leaves the server: the protocol's goodbye, then the WebSocket. The page starts afresh for
// the next drive (a drive's picture and memory are not taken apart piece by piece).
function leaveServer(reload) {
  const online = state.online;
  if (!online) return;
  state.online = null;
  try {
    if (state.game) state.game.leave();
  } catch {
    // the session is gone already
  }
  try {
    online.ws.close(1000);
  } catch {
    // closed already
  }
  if (reload) {
    const keep = new URLSearchParams(params);
    for (const key of ['go', 'join']) keep.delete(key);
    setTimeout(() => location.assign(location.pathname + (keep.toString() ? '?' + keep : '')), 150);
  }
}

window.addEventListener('pagehide', () => leaveServer(false));

function showOnline(online) {
  window.rustyac.online = online;
  const box = $('hud-online');
  box.hidden = !online;
  if (!online) return;
  const lines = [`${online.server}   ${online.session}   ping ${online.ping} ms${online.associated ? '' : '   (waiting for the server)'}`];
  for (const d of online.drivers) {
    lines.push(`${d.me ? '>' : ' '} ${(d.name || '?').padEnd(18).slice(0, 18)} ${d.car.padEnd(17).slice(0, 17)} laps ${String(d.laps).padEnd(3)} best ${time(d.best_ms)}${d.me ? '' : `  ${d.ping} ms`}`);
  }
  lines.push(...online.chat);
  lines.push('Enter: chat   Esc: leave');
  box.textContent = lines.join('\n');
}

function openChat() {
  const input = $('chat');
  if (!state.online || !input.hidden) return;
  state.game.release_keys();
  input.value = '';
  input.hidden = false;
  input.focus();
}

$('chat').addEventListener('keydown', (event) => {
  event.stopPropagation();
  const input = $('chat');
  if (event.key === 'Enter') {
    const text = input.value.trim();
    // (the game sends one line a second at most)
    if (text && state.game && !state.game.chat(text)) return;
    input.hidden = true;
    input.blur();
  } else if (event.key === 'Escape') {
    input.hidden = true;
    input.blur();
  }
});
$('chat').addEventListener('keyup', (event) => event.stopPropagation());

// ---------------------------------------------------------------- driving

function resizeCanvas() {
  const scale = Math.min(window.devicePixelRatio || 1, 1.5);
  const width = Math.max(16, Math.round(canvas.clientWidth * scale));
  const height = Math.max(16, Math.round(canvas.clientHeight * scale));
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
    if (state.game) state.game.resize(width, height);
  }
}

// The small key list over the picture: shown for the first seconds of a drive, H brings it back.
const KEY_LIST =
  'gas, brake      Up / W, Down / S      RT, LT\n' +
  'steer       Left / A, Right / D   left stick\n' +
  'gears     Space / E, Ctrl / Q         Y, X\n' +
  'clutch, DRS, KERS   Shift, F, K     A, LB, B\n' +
  'TC up, down     T, Shift+T   D-pad up, down\n' +
  'ABS up, down    Y, Shift+Y   Back + D-pad up, down\n' +
  'brake bias fwd, back   ], [   D-pad right, left\n' +
  'pits R   track Shift+R   view F1 / C   new car N\n' +
  'gearbox G   pause P   menu Esc   this list H';
let keysTimer = 0;

function showKeys(forMs) {
  $('keys').textContent = KEY_LIST;
  $('keys').hidden = false;
  clearTimeout(keysTimer);
  if (forMs) keysTimer = setTimeout(() => ($('keys').hidden = true), forMs);
}

function startDriving() {
  $('menu').hidden = true;
  $('hud').hidden = false;
  showKeys(15000);
  state.running = true;
  state.paused = false;
  window.rustyac.state = 'driving';
  perf.since = performance.now();
  requestAnimationFrame(frame);
}

function stopDriving() {
  if (state.online) {
    // a car that stands still in the menu would stay on the server's track: Esc leaves
    state.running = false;
    $('menu').hidden = false;
    $('hud').hidden = true;
    $('keys').hidden = true;
    $('chat').hidden = true;
    setMultiplayer(`you left ${state.online.server.name || state.online.server.id}`);
    window.rustyac.state = 'left';
    leaveServer(true);
    return;
  }
  state.running = false;
  if (state.game) state.game.release_keys();
  $('menu').hidden = false;
  $('hud').hidden = true;
  $('keys').hidden = true;
  setStatus('paused in the menu: press Drive to go on with the same car, or pick another');
  window.rustyac.state = 'menu';
  markRefused();
}

const time = (ms) => {
  if (!(ms > 0)) return '-:--.---';
  const total = Math.floor(ms);
  return `${Math.floor(total / 60000)}:${String(Math.floor(total / 1000) % 60).padStart(2, '0')}.${String(total % 1000).padStart(3, '0')}`;
};

const perf = { since: 0, text: '' };

// The display over the picture, from the wasm side's numbers. The aids' levels and the brake
// bias are there all the time (lit while the aid acts); a change shows as a note for a moment.
function showHud(hud) {
  window.rustyac.hud = hud;
  $('hud-gear').textContent = hud.gear === 0 ? 'R' : hud.gear === 1 ? 'N' : String(hud.gear - 1);
  $('hud-speed').textContent = Math.round(hud.kmh);
  $('hud-rpm-bar').style.width = `${Math.min(100, (hud.rpm / Math.max(1, hud.rpm_limit)) * 100).toFixed(1)}%`;
  $('hud-rpm-text').textContent = `${Math.round(hud.rpm)} rpm`;
  $('hud-lap-time').textContent = time(hud.lap_ms);
  $('hud-last').textContent = time(hud.last_ms);
  $('hud-best').textContent = time(hud.best_ms);
  $('hud-info').textContent =
    `${hud.camera} view   ${hud.device}\n` +
    `${hud.auto_shifter ? 'automatic' : 'manual'} gearbox${hud.drs ? '   DRS' : ''}\n` +
    `lap ${hud.laps + 1}${hud.valid ? '' : ' (not valid)'}${hud.in_pit_lane ? '   pit lane' : ''}${state.paused ? '\nPAUSED (P)' : ''}`;
  $('hud-tc').textContent = hud.tc_text;
  $('hud-tc').classList.toggle('acting', hud.tc);
  $('hud-abs').textContent = hud.abs_text;
  $('hud-abs').classList.toggle('acting', hud.abs);
  $('hud-bias').textContent = hud.bias_text;
  $('hud-note').textContent = hud.note;
  $('hud-note').hidden = !hud.note;
  showOnline(hud.online);
}

// What the display says about the aids right now (a test reads it).
const aidsShown = () => `${$('hud-tc').textContent} | ${$('hud-abs').textContent} | ${$('hud-bias').textContent}`;

function frame(now) {
  if (!state.running) return;
  resizeCanvas();
  pollPad();
  if (!state.paused) {
    try {
      state.game.frame(now);
    } catch (error) {
      state.running = false;
      $('menu').hidden = false;
      fail(error);
      return;
    }
  }
  let hud;
  try {
    hud = JSON.parse(state.game.hud());
  } catch (error) {
    state.running = false;
    $('menu').hidden = false;
    fail(error);
    return;
  }
  showHud(hud);
  if (now - perf.since >= 1000) {
    const stats = JSON.parse(state.game.take_stats());
    const seconds = (now - perf.since) / 1000;
    stats.fps = stats.frames / seconds;
    stats.steps_per_second = stats.steps / seconds;
    window.rustyac.perf = stats;
    if (state.online) {
      // what the session printed (who came, who left, laps, chat): kept for a look
      for (const line of state.game.take_console().split('\n')) if (line) window.rustyac.log.push(line);
      if (window.rustyac.log.length > 400) window.rustyac.log.splice(0, 200);
    }
    $('hud-perf').textContent =
      `${stats.fps.toFixed(0)} frames/s   ${stats.steps_per_second.toFixed(0)} physics steps/s   ${stats.work_ms_per_frame.toFixed(2)} ms per frame\n` +
      `${stats.meshes} meshes, ${(stats.triangles / 1000).toFixed(0)}k triangles   ${stats.backend}` +
      (state.online ? `   other cars: ${stats.other_cars_drawn} drawn, ${stats.other_cars_as_boxes} as boxes` : '') +
      (stats.dropped_seconds > 0.05 ? `\nslow frames: ${stats.dropped_seconds.toFixed(1)} s dropped in the last second` : '');
    perf.since = now;
  }
  requestAnimationFrame(frame);
}

// Windows virtual-key codes, as AC's controls.ini and its keyboard class name the keys.
const VK = {
  ArrowUp: 0x26, ArrowDown: 0x28, ArrowLeft: 0x25, ArrowRight: 0x27, Space: 0x20, Enter: 0x0d, Tab: 0x09, Backspace: 0x08,
  ShiftLeft: 0xa0, ShiftRight: 0xa1, ControlLeft: 0xa2, ControlRight: 0xa3, AltLeft: 0xa4, AltRight: 0xa5,
  Insert: 0x2d, Delete: 0x2e, Home: 0x24, End: 0x23, PageUp: 0x21, PageDown: 0x22, BracketLeft: 0xdb, BracketRight: 0xdd,
};
for (let i = 0; i < 26; i++) VK[`Key${String.fromCharCode(65 + i)}`] = 0x41 + i;
for (let i = 0; i < 10; i++) VK[`Digit${i}`] = 0x30 + i;
for (let i = 1; i <= 12; i++) VK[`F${i}`] = 0x6f + i;

// `rustyac_game::input_file::event`: commands that run before the next physics step.
const EVENT = { RESET: 1, REBUILD: 2, TC_UP: 4, TC_DN: 8, ABS_UP: 16, ABS_DN: 32, AUTO_SHIFTER: 64, TO_TRACK: 256 };

// The page's own keys. With the keys the built-in layout drives with (asked of the wasm side
// once a car is there) they are the only keys the page keeps from the browser.
const PAGE_KEYS = ['Escape', 'Pause', 'KeyP', 'F1', 'KeyC', 'KeyR', 'KeyN', 'KeyG', 'KeyH', 'KeyT', 'KeyY', 'BracketLeft', 'BracketRight', 'Enter', 'NumpadEnter'];
let drivingKeys = null;

function pageUses(event) {
  if (PAGE_KEYS.includes(event.code)) return true;
  if (!drivingKeys) drivingKeys = new Set(state.game.keys_used());
  return drivingKeys.has(VK[event.code]);
}

window.addEventListener('keydown', (event) => {
  if (!(state.running || state.testing) || !state.game) return;
  // a key with Ctrl, Alt or the Windows key is the browser's (Ctrl+T, Alt+Left ...), except
  // that a Ctrl key is itself a driving key (gear down)
  const isControl = event.code === 'ControlLeft' || event.code === 'ControlRight';
  if (event.metaKey || event.altKey || (event.ctrlKey && !isControl)) return;
  // every other key the page has no use for stays the browser's too (F5, F11, F12, Tab ...)
  if (!pageUses(event)) return;
  const code = VK[event.code];
  event.preventDefault();
  if (event.repeat) return;
  switch (event.code) {
    case 'Escape':
      stopDriving();
      return;
    case 'Enter':
    case 'NumpadEnter':
      // on a server: the chat line (offline the key is the layout's, if it has it)
      if (state.online) {
        openChat();
        return;
      }
      break;
    case 'KeyP':
    case 'Pause':
      // (a server's clock does not stop: no pause there)
      if (!state.online) state.paused = !state.paused;
      return;
    case 'F1':
    case 'KeyC':
      state.game.next_camera();
      break;
    case 'KeyR':
      state.game.request(event.shiftKey ? EVENT.TO_TRACK : EVENT.RESET);
      break;
    case 'KeyN':
      state.game.request(EVENT.REBUILD);
      break;
    case 'KeyG':
      state.game.request(EVENT.AUTO_SHIFTER);
      break;
    // the aids and the brake bias: the same commands the desktop's keys and the pad give
    case 'KeyT':
      state.game.request(event.shiftKey ? EVENT.TC_DN : EVENT.TC_UP);
      break;
    case 'KeyY':
      state.game.request(event.shiftKey ? EVENT.ABS_DN : EVENT.ABS_UP);
      break;
    case 'BracketRight':
      state.game.bias(1);
      break;
    case 'BracketLeft':
      state.game.bias(-1);
      break;
    case 'KeyH':
      if ($('keys').hidden) showKeys(0);
      else $('keys').hidden = true;
      break;
    default:
  }
  if (code !== undefined) state.game.key(code, true);
});

window.addEventListener('keyup', (event) => {
  const code = VK[event.code];
  if (state.game && code !== undefined) state.game.key(code, false);
});

window.addEventListener('blur', () => state.game && state.game.release_keys());

// The test hook of the keys: `press('KeyT')`, `press('Shift+KeyT')`, `press('BracketRight')`
// send a key-down and a key-up through the handlers above, as a keyboard would. Returns
// whether the page kept the key from the browser (`preventDefault`).
window.rustyac.press = (name) => {
  const parts = name.split('+');
  const code = parts.pop();
  const init = { code, shiftKey: parts.includes('Shift'), ctrlKey: parts.includes('Ctrl'), altKey: parts.includes('Alt'), bubbles: true, cancelable: true };
  const kept = !window.dispatchEvent(new KeyboardEvent('keydown', init));
  window.dispatchEvent(new KeyboardEvent('keyup', init));
  return kept;
};

// The Gamepad API's standard layout, as XInput's `wButtons` bits: A B X Y, LB RB, (the
// triggers are analog), Back Start, the stick presses, the d-pad.
const PAD_BITS = [0x1000, 0x2000, 0x4000, 0x8000, 0x0100, 0x0200, 0, 0, 0x0020, 0x0010, 0x0040, 0x0080, 0x0001, 0x0002, 0x0004, 0x0008];
const pad = { start: false, rumbleAt: 0 };

function pollPad() {
  const game = state.game;
  const found = navigator.getGamepads ? [...navigator.getGamepads()].find((p) => p && p.connected && p.mapping === 'standard') : null;
  if (!found) {
    game.pad(false, 0, 0, 0, 0, 0, 0, 0);
    return;
  }
  let buttons = 0;
  found.buttons.forEach((button, i) => {
    if (button.pressed && PAD_BITS[i]) buttons |= PAD_BITS[i];
  });
  const stick = (value) => Math.max(-32768, Math.min(32767, Math.round(value * 32767)));
  const trigger = (button) => Math.round(Math.max(0, Math.min(1, button ? button.value : 0)) * 255);
  // XInput's sticks are positive up, the Gamepad API's positive down
  game.pad(true, buttons, trigger(found.buttons[6]), trigger(found.buttons[7]), stick(found.axes[0] || 0), stick(-(found.axes[1] || 0)), stick(found.axes[2] || 0), stick(-(found.axes[3] || 0)));
  const start = !!(buttons & 0x0010);
  if (start && !pad.start && !state.online) state.paused = !state.paused;
  pad.start = start;
  const now = performance.now();
  if (found.vibrationActuator && now - pad.rumbleAt > 80) {
    pad.rumbleAt = now;
    const [strong, weak] = state.paused ? [0, 0] : game.rumble();
    if (strong > 0 || weak > 0 || pad.rumbling) {
      found.vibrationActuator.playEffect('dual-rumble', { duration: 120, strongMagnitude: Math.min(1, strong), weakMagnitude: Math.min(1, weak) }).catch(() => {});
    }
    pad.rumbling = strong > 0 || weak > 0;
  }
}

// ---------------------------------------------------------------- start

async function pickFolder() {
  try {
    if (window.showDirectoryPicker) {
      const handle = await window.showDirectoryPicker({ id: 'assettocorsa', mode: 'read' });
      await useFolder(handle, true);
    } else {
      $('folder-input').click();
    }
  } catch (error) {
    if (error && error.name === 'AbortError') return;
    fail(error);
  }
}

async function useFolder(handle, remember) {
  $('refusal').hidden = true;
  setStatus('');
  $('content-line').textContent = 'reading the folder ...';
  const source = await openFolder(handle);
  if (remember) await rememberFolder(handle);
  await useSource(source, `${handle.name}: ${source.cars.length} cars, ${source.tracks.length} tracks and layouts`);
}

async function main() {
  await init();
  $('version').textContent = rac.version();
  $('open-folder').addEventListener('click', pickFolder);
  $('folder-input').addEventListener('change', async (event) => {
    try {
      await useFolder(folderFromFiles(event.target.files), false);
    } catch (error) {
      fail(error);
    }
  });
  $('car').addEventListener('change', markRefused);
  $('track').addEventListener('change', markRefused);
  $('drive').addEventListener('click', () => {
    if (state.game && state.lastChoice === `${$('car').value}|${$('track').value}`) startDriving();
    else if (state.game) location.search = new URLSearchParams({ ...Object.fromEntries(params), car: $('car').value, track: $('track').value.split('|')[0], layout: $('track').value.split('|')[1], go: '1' }).toString();
    else {
      state.lastChoice = `${$('car').value}|${$('track').value}`;
      drive();
    }
  });
  $('mp-server').addEventListener('change', markServers);
  $('mp-join').addEventListener('click', joinServer);
  window.addEventListener('resize', resizeCanvas);
  if (params.get('backend') === 'webgl') $('backend').value = 'webgl';
  if (params.get('spawn')) $('spawn').value = params.get('spawn');

  let ready = false;
  if (params.has('testfolder')) {
    // the folder picker's path without the picker (it needs a click a test cannot give)
    await useFolder(new HttpFolder(params.get('testfolder'), 'assettocorsa (test hook)'), false);
    ready = true;
  } else {
    const base = params.get('pack') || 'preview/';
    const pack = await openPack(base.endsWith('/') ? base : base + '/').catch((error) => {
      console.warn(error);
      return null;
    });
    if (pack) {
      rac.fs_list(pack.manifest.files.map((f) => f.path).join('\n'));
      const line = `preview pack: ${pack.cars.map((c) => `${c.name} (${megabytes(c.bytes)})`).join(', ')}; ${pack.tracks.map((t) => `${t.name} (${megabytes(t.bytes)})`).join(', ')}`;
      $('pack-line').hidden = false;
      $('pack-line').textContent = `A preview pack is on this server (${megabytes(pack.manifest.bytes)} in all; only the chosen car and track are fetched, and kept in this browser for the next visit).`;
      await useSource(pack, line);
      ready = true;
    } else {
      const handle = window.showDirectoryPicker ? await rememberedFolder() : null;
      if (handle) {
        const granted = (await handle.queryPermission({ mode: 'read' })) === 'granted';
        if (granted) {
          await useFolder(handle, false).catch(fail);
          ready = !!state.source;
        } else {
          const again = $('reopen-folder');
          again.hidden = false;
          again.textContent = `Use "${handle.name}" again`;
          again.addEventListener('click', async () => {
            try {
              if ((await handle.requestPermission({ mode: 'read' })) === 'granted') await useFolder(handle, false);
            } catch (error) {
              fail(error);
            }
          });
        }
      }
      if (!ready) {
        window.rustyac.state = 'menu';
        $('content-line').textContent = 'no preview pack on this server: open your own Assetto Corsa folder';
      }
    }
  }
  await loadServers();
  if (ready && params.get('join') && !$('mp-join').disabled) {
    await joinServer();
  } else if (ready && params.get('go') === '1' && !$('drive').disabled) {
    state.lastChoice = `${$('car').value}|${$('track').value}`;
    await drive();
  }
}

// Test hooks of a drive on a server: where the other cars are, a chat line, the arrival
// times of the car states (?arrivals=1), leaving.
window.rustyac.others = () => (state.game && state.online ? JSON.parse(state.game.other_cars()) : []);
window.rustyac.chat = (text) => !!(state.game && state.online && state.game.chat(text));
window.rustyac.arrivals = () => (state.online ? state.online.link.arrivals() : '');
window.rustyac.leave = () => state.online && stopDriving();

main().catch(fail);
