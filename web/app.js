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
//   ?backend=auto      the debug view (WebGPU, else WebGL2)
//   ?backend=webgl     the debug view on WebGL2
//   ?backend=full      Full mode: AC's own renderer draws (WebGPU only; else the debug view)
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
//   ?nocontrols=1      do not read the Controls panel's settings kept in this browser
//   ?set=<list>        Controls panel values for this visit, before the drive:
//                      `X360.SPEED_SENSITIVITY=0,KEYBOARD.STEERING_SPEED=3` (a test's way in;
//                      they are not kept)
//   ?hold=<list>       with ?selftest: keys held on the way, as Windows virtual-key codes with
//                      the steps they go down and up: `38@100-3000,37@1200-1500` holds Up from
//                      step 100 to 3000 and Left from 1200 to 1500 (use it with autodrive=0;
//                      the desktop's selftest takes the same list with `--hold`)

import init, * as rac from './pkg/rustyac_web.js';
import { startLauncher } from './launcher.js';

const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);
let canvas = $('view');

// A new drive in the same page: the last drive's game (and its GPU objects) goes, and a fresh
// canvas takes the old one's place (a canvas keeps the context it was first given).
function freshCanvas() {
  if (state.game) state.game.free();
  state.game = null;
  const next = canvas.cloneNode(false);
  canvas.replaceWith(next);
  canvas = next;
}

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

// The page's own log for a report (Copy diagnostics): connections, joins, downloads, errors
// of the last few visits, in the browser's storage.
const DIAG_KEY = 'rustyac.diagnostics';
const diagnostics = (() => {
  let visits = [];
  try {
    visits = JSON.parse(localStorage.getItem(DIAG_KEY) || '[]');
  } catch {
    visits = [];
  }
  if (!Array.isArray(visits)) visits = [];
  const visit = { started: new Date().toISOString(), page: location.pathname + location.search, browser: navigator.userAgent, events: [] };
  visits = [...visits.slice(-4), visit];
  return { visits, visit };
})();

function diag(kind, text) {
  const events = diagnostics.visit.events;
  events.push(`${new Date().toISOString().slice(11, 23)} ${kind}: ${text}`);
  if (events.length > 300) events.splice(0, 100);
  try {
    localStorage.setItem(DIAG_KEY, JSON.stringify(diagnostics.visits));
  } catch {
    // no storage: the log lives as long as the page
  }
}

const diagnosticsText = () =>
  diagnostics.visits.map((v) => `== visit ${v.started} ${v.page}\n${v.browser}\n${v.events.join('\n')}`).join('\n\n');

function setStatus(text) {
  $('status').textContent = text;
  window.rustyac.log.push(text);
}

function fail(error) {
  let text = String(error && error.message ? error.message : error);
  // (a pack's files are checked as they arrive: one that does not decrypt is not a broken download)
  if (state.source && state.source.kind === 'pack' && /do not decrypt|encrypted or unreadable/.test(text) && !/couldn't download/.test(text)) {
    text += " The files were downloaded whole and match the pack's list: the pack itself holds them this way.";
  }
  console.error(error);
  diag('error', text);
  window.rustyac.state = 'error';
  window.rustyac.error = text;
  $('status').textContent = '';
  $('refusal').hidden = false;
  $('refusal').textContent = text;
  $('progress').hidden = true;
  $('drive').disabled = !state.source;
  // the launcher's frame gives way to the menu, where the refusal is
  if ($('launcher')) $('launcher').hidden = true;
  if ($('corner')) $('corner').hidden = true;
  $('menu').hidden = false;
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
    // (the launcher reads a file it did not read before at once, over HTTP)
    urlOf: (path) => base + path.split('/').map(encodeURIComponent).join('/'),
    sizes: new Map(manifest.files.map((f) => [f.path, f.bytes])),
    cars: manifest.cars.map((c) => ({ id: c.id, name: c.name, refused: null, bytes: c.bytes, pinned: !!c.pinned })),
    tracks: manifest.tracks.map((t) => ({ track: t.id, layout: t.layout, name: t.name, refused: null, bytes: t.bytes, pinned: !!t.pinned })),
    // from: resume at this byte (HTTP Range); fresh: not from the browser's storage
    async open(path, { from = 0, fresh = false } = {}) {
      const entry = entries.get(path.toLowerCase());
      if (!entry) throw new Error(`${path} is not in the preview pack`);
      const discard = async () => {
        if (cache) await cache.removeEntry(entry.sha256).catch(() => {});
      };
      // a file fetched on an earlier visit is in the browser's own storage, under its hash
      // (and checked against that hash again, as a download is)
      if (cache && entry.sha256 && !fresh && !from) {
        try {
          const file = await (await cache.getFileHandle(entry.sha256)).getFile();
          if (file.size === entry.bytes) return { size: file.size, stream: file.stream(), sha256: entry.sha256, from: 'cache', discard };
        } catch {
          // not cached yet
        }
      }
      const got = await fetch(base + entry.path.split('/').map(encodeURIComponent).join('/'), from ? { headers: { Range: `bytes=${from}-` } } : {});
      if (!got.ok || !got.body) throw new Error(`the server answered ${got.status}`);
      // (a server that ignores the Range sends the whole file again)
      const resumed = from > 0 && got.status === 206;
      const length = Number(got.headers.get('content-length'));
      const expected = entry.bytes - (resumed ? from : 0);
      if (got.headers.has('content-length') && length !== expected && !got.headers.has('content-encoding')) {
        await got.body.cancel().catch(() => {});
        throw new Error(`the server sent ${length} bytes, the pack's list says ${expected}`);
      }
      return { size: entry.bytes, stream: got.body, sha256: entry.sha256 || '', from: 'network', resumed, discard };
    },
    // the browser's storage for a file being downloaded (written while it arrives, closed
    // only once the whole file passed its check: never a part of one)
    async keep(path) {
      const entry = entries.get(path.toLowerCase());
      if (!cache || !entry || !entry.sha256) return null;
      try {
        return await (await cache.getFileHandle(entry.sha256, { create: true })).createWritable();
      } catch {
        return null;
      }
    },
  };
}

// The files of an older pack (another hash, so another name) are no use: dropped once the
// pack's list is read.
async function dropStaleCopies(manifest) {
  const cache = await packCache();
  if (!cache) return;
  const listed = new Set(manifest.files.map((f) => f.sha256));
  try {
    for await (const name of cache.keys()) {
      if (!listed.has(name)) {
        await cache.removeEntry(name).catch(() => {});
        diag('cache', `dropped ${name.slice(0, 12)}...: a file of an older pack`);
      }
    }
  } catch {
    // the storage is gone: nothing to drop
  }
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
  // the game's rules a drive from AC's launcher reads (system/cfg: the race's over time, the pits)
  if (system.has('cfg')) await walk(system.get('cfg').handle, 'system/cfg', 0, []);
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
    root,
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
    // Full mode: more of the folder than the first walk took ([folder, depth, folders to skip]
    // each): AC's renderer reads the shaders, the sky's and the pits' files, a car's skins
    async listMore(parts) {
      const added = [];
      const walk = async (folder, at, depth, skip) => {
        for (const [lower, { name, handle }] of await children(folder)) {
          const path = `${at}/${name}`;
          if (handle.kind === 'file') {
            if (!handles.has(path)) added.push(path);
            handles.set(path, handle);
          } else if (depth > 0 && !skip.includes(lower)) await walk(handle, path, depth - 1, skip);
        }
      };
      for (const [path, depth, skip] of parts) {
        let folder = root;
        for (const name of path.split('/')) folder = folder && (await children(folder)).get(name.toLowerCase())?.handle;
        if (folder && folder.kind === 'directory') await walk(folder, path, depth, skip);
      }
      if (added.length) rac.fs_list(added.join('\n'));
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
    // (one pass over the folder's list, not one per car)
    const dataOf = new Map();
    for (const path of handles.keys()) {
      const parts = path.toLowerCase().split('/');
      if (parts[1] !== 'cars' || !(parts[3] || '').startsWith('data')) continue;
      if (!dataOf.has(parts[2])) dataOf.set(parts[2], []);
      dataOf.get(parts[2]).push(path);
    }
    for (const car of source.cars) {
      const data = dataOf.get(car.id.toLowerCase()) || [];
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

// The parts of a path below the game's folder that a drive can need: a car's data, models and
// menu entry, a track without its skins, sounds and textures, the game's tables. The rest of
// a folder with tens of thousands of files (skins, sounds, previews, apps) is never kept.
function listedPathWanted(parts) {
  const lower = parts.map((part) => part.toLowerCase());
  const name = lower[lower.length - 1];
  // AC's launcher: its cfg, the themes (no videos), the weathers (web/launcher.js walkFolder)
  if (lower[0] === 'cfg') return parts.length <= 6;
  if (lower[0] === 'launcher') return lower[1] === 'themes' && parts.length <= 11 && !name.endsWith('.ogv');
  if (lower[0] === 'system') return ((lower[1] === 'data' || lower[1] === 'cfg') && parts.length === 3) || (lower[1] === 'cfg' && parts.length === 4) || (lower[1] === 'shaders' && lower[2] === 'win' && parts.length === 4);
  if (lower[0] !== 'content') return false;
  // (and what Full mode's renderer reads: the weathers, the sky's, the pits' and the drivers' files)
  if (lower[1] === 'weather') return parts.length <= 4;
  if (['texture', 'driver', 'fonts', 'objects3d'].includes(lower[1])) return parts.length <= 5;
  if (lower[1] === 'cars') {
    if (parts.length === 4) return true;
    if (lower[3] === 'ui') return parts.length <= 7;
    if (lower[3] === 'skins') return parts.length <= 7;
    if (lower[3] === 'texture' || lower[3] === 'animations') return parts.length <= 6;
    return parts.length === 5 && lower[3] === 'data';
  }
  if (lower[1] !== 'tracks') return false;
  if (lower[3] === 'ui') return parts.length <= 7 && !name.endsWith('.ogv');
  return parts.length <= 7 && !lower.slice(3, -1).some((part) => ['skins', 'extension', 'sfx', 'texture'].includes(part));
}

// `<input webkitdirectory>` (Firefox, Safari, and Chrome for a folder its picker refuses): the
// browser hands over every file of the folder as one list, with paths.
function folderFromFiles(list) {
  // where the game's folder ends in the paths: the highest `content/cars` or `content/tracks`
  // (the game's own `server` folder has a `content/cars` of its own, deeper down)
  let prefix = Infinity;
  for (const file of list) {
    const path = file.webkitRelativePath.toLowerCase();
    for (const marker of ['/content/cars/', '/content/tracks/']) {
      const at = path.indexOf(marker);
      if (at >= 0 && at + 1 < prefix) prefix = at + 1;
    }
  }
  if (prefix === Infinity) throw new Error("that is not Assetto Corsa's folder: no content/cars or content/tracks in it");
  // the same shape the picker's handle has
  const tree = { kind: 'directory', name: list[0].webkitRelativePath.slice(0, prefix - 1), items: new Map() };
  for (const kind of ['content', 'system']) tree.items.set(kind, { kind: 'directory', name: kind, items: new Map() });
  let kept = 0;
  for (const file of list) {
    const parts = file.webkitRelativePath.slice(prefix).split('/');
    if (!listedPathWanted(parts)) continue;
    kept += 1;
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
  window.rustyac.listed = { files: list.length, kept };
  return withEntries(tree);
}

// ---------------------------------------------------------------- the lists

const carLabel = (c) => (c.name === c.id ? c.id : `${c.name} (${c.id})`);
const trackLabel = (t) => (t.layout ? `${t.name} (${t.track} / ${t.layout})` : `${t.name} (${t.track})`);
const trackValue = (t) => `${t.track}|${t.layout}`;
// a list this long gets a search box
const SEARCH_FROM = 13;

// A pack's lists: what its config named itself comes first, in its order (the preview's two
// cars and two tracks); what a "*" added follows, sorted by name.
function ordered(items, label) {
  const pinned = items.filter((item) => item.pinned);
  if (pinned.length === 0) return items;
  const rest = items.filter((item) => !item.pinned).sort((a, b) => label(a).localeCompare(label(b), undefined, { sensitivity: 'base', numeric: true }));
  return [...pinned, ...rest];
}

// The entries of a list that have every word of its search box, in the list's order. The
// chosen entry stays chosen while it is among them.
function fillOptions(select, search, items, label, value) {
  const words = search.value.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = words.length ? items.filter((item) => words.every((word) => label(item).toLowerCase().includes(word))) : items;
  const chosen = select.value;
  select.replaceChildren(...shown.map((item) => new Option(label(item), value(item))));
  if (shown.some((item) => value(item) === chosen)) select.value = chosen;
  search.classList.toggle('nothing', words.length > 0 && shown.length === 0);
}

function searchLists() {
  fillOptions($('car'), $('car-search'), state.cars, carLabel, (c) => c.id);
  fillOptions($('track'), $('track-search'), state.tracks, trackLabel, trackValue);
  markRefused();
}

function fillLists() {
  const source = state.source;
  state.cars = ordered(source.cars, carLabel);
  state.tracks = ordered(source.tracks, trackLabel);
  const car = $('car');
  const track = $('track');
  $('car-search').hidden = state.cars.length < SEARCH_FROM;
  $('track-search').hidden = state.tracks.length < SEARCH_FROM;
  $('car-search').value = $('track-search').value = '';
  fillOptions(car, $('car-search'), state.cars, carLabel, (c) => c.id);
  fillOptions(track, $('track-search'), state.tracks, trackLabel, trackValue);
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
  markServers();
}

async function useSource(source, line) {
  state.source = source;
  $('content-line').textContent = line;
  fillLists();
  window.rustyac.state = 'menu';
}

// ---------------------------------------------------------------- files into the wasm side

// Each file whole and checked: a download that breaks off goes on where it stopped (HTTP
// Range), after a pause that grows; one that arrives wrong (or a stored copy that is wrong)
// is thrown away and fetched again. What cannot be had fails with the file's name and why.
const DOWNLOAD_TRIES = 5;

async function loadFiles(paths, done) {
  for (const path of paths) await loadFile(path, done);
}

async function loadFile(path, done) {
  const source = state.source;
  let tries = 0;
  let fresh = false;
  let from = 0;
  let file = null;
  let writer = null;
  let keep = null;
  const restart = async () => {
    if (writer) writer.free();
    if (keep) await keep.abort().catch(() => {});
    writer = null;
    keep = null;
    from = 0;
  };
  for (;;) {
    try {
      file = await source.open(path, { from, fresh });
      if (from && file.resumed) diag('download', `${path}: goes on from ${megabytes(from)} (HTTP Range)`);
      else if (from) await restart();
      if (!writer) {
        writer = new rac.FileWriter(file.size, !!file.sha256);
        if (file.from === 'network' && source.keep) keep = await source.keep(path);
      }
      const reader = file.stream.getReader();
      for (;;) {
        const { done: end, value } = await reader.read();
        if (end) break;
        writer.write(value);
        from += value.length;
        if (keep) {
          try {
            await keep.write(value);
          } catch {
            // the browser's storage is full or gone: go on without keeping the file
            await keep.abort().catch(() => {});
            keep = null;
          }
        }
        done(value.length, path, file.from);
      }
      // (commit takes the writer; it checks the size and hash before the file is mounted)
      const committing = writer;
      writer = null;
      try {
        committing.commit(path, file.sha256 || '');
      } catch (error) {
        // the whole file arrived and is wrong: no use resuming it
        from = 0;
        throw error;
      }
      if (keep) await keep.close().catch(() => {});
      keep = null;
      if (tries || file.from === 'network') diag('download', `${path}: ${megabytes(file.size)} ${file.from === 'cache' ? 'from storage' : 'downloaded'}${tries ? `, ${tries} retries` : ''}`);
      return;
    } catch (error) {
      const why = String(error && error.message ? error.message : error);
      if (source.kind !== 'pack') {
        await restart();
        throw error;
      }
      if (file && file.from === 'cache') {
        // the stored copy is not the file: away with it, and from the network
        diag('cache', `${path}: the stored copy failed its check (${why}): downloaded again`);
        await file.discard();
        await restart();
        fresh = true;
        file = null;
        continue;
      }
      tries++;
      diag('download', `${path}: try ${tries} of ${DOWNLOAD_TRIES} failed at ${megabytes(from)}: ${why}`);
      if (!from) await restart();
      if (tries >= DOWNLOAD_TRIES) {
        await restart();
        throw new Error(`couldn't download ${path} (${why}; ${DOWNLOAD_TRIES} tries)`);
      }
      file = null;
      setStatus(`downloading ${path.slice(path.lastIndexOf('/') + 1)}: ${why}; trying again ...`);
      await sleep(500 * 3 ** (tries - 1));
    }
  }
}

// The cars of a race.ini: [RACE] MODEL and every [CAR_n] MODEL but the player's `-`, once each.
function raceModels(race) {
  const models = [];
  for (const part of race.split(/^\[/m)) {
    const section = part.slice(0, part.indexOf(']')).trim().toUpperCase();
    if (section !== 'RACE' && !/^CAR_\d+$/.test(section)) continue;
    const line = part.split(/\r?\n/).find((l) => l.split('=')[0].trim().toUpperCase() === 'MODEL');
    const model = line ? line.slice(line.indexOf('=') + 1).trim() : '';
    if (model && model !== '-' && !models.includes(model)) models.push(model);
  }
  return models;
}

// The picture a drive gets: ?backend=, else the Picture menu (kept in this browser). Full mode
// and WebGPU need an adapter; without one the debug view on WebGL2, and why.
async function pictureBackend() {
  let backend = params.get('backend') || $('backend').value;
  let why = '';
  if (backend !== 'webgl') {
    const adapter = navigator.gpu ? await navigator.gpu.requestAdapter().catch(() => null) : null;
    if (!adapter) {
      if (backend === 'full') why = 'Full mode needs WebGPU, and this browser offers none: the debug view draws instead (WebGL2).';
      backend = 'webgl';
    }
  }
  state.pictureWhy = why;
  return { backend, why };
}

// Full mode: lists the folder's parts AC's renderer reads (a pack lists what it holds already)
async function listPictureFiles(models) {
  if (!state.source.listMore) return;
  const parts = [
    ['system/shaders/win', 0, []],
    ['content/weather', 1, []],
    // (the driver's and the crew's suits, helmets and gloves: content/texture/<kind>/<name>/<colour>/_nm)
    ['content/texture', 4, []],
    ['content/driver', 1, []],
    ['content/fonts', 0, []],
    ['content/objects3D', 0, []],
    ...models.map((model) => [`content/cars/${model}`, 3, ['sfx', 'data', 'ui']]),
  ];
  await state.source.listMore(parts);
}

// `launch`: a drive AC's launcher asked for ({ car, track, layout, race, assists }), else the
// menu's choice
async function drive(launch) {
  const car = launch ? launch.car : $('car').value;
  const [track, layout] = launch ? [launch.track, launch.layout] : $('track').value.split('|');
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
    // (and the files of race.ini's other cars, which AI drivers drive)
    const models = [...new Set([car, ...(launch && launch.race ? raceModels(launch.race) : [])])];
    const picture = await pictureBackend();
    const filesWanted = picture.backend === 'full' ? rac.files_wanted_full : rac.files_wanted;
    if (picture.backend === 'full') await listPictureFiles(models);
    const wantedNow = () => [...new Set(models.flatMap((model) => filesWanted(model, track, layout).split('\n').filter(Boolean)))];
    const fetchWanted = async (wanted) => {
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
    };
    for (let round = 0; round < 8; round++) {
      const wanted = wantedNow();
      if (!wanted.length) break;
      await fetchWanted(wanted);
    }
    if (launch) {
      // what the session reads of the game's folder besides the car and the track
      // (src/race.rs: OVER_TIME_MULT, the pit stop's settings); without them the game's defaults
      await loadFiles(['system/cfg/assetto_corsa.ini', 'system/cfg/pitstop.ini'], () => {}).catch((error) => console.warn(error));
    }
    const fileSeconds = (performance.now() - started) / 1000;
    $('progress').hidden = true;
    setStatus('building the track and the car (a large track takes some seconds; the page stands still meanwhile) ...');
    await nextPaint();
    resizeCanvas();
    const built = performance.now();
    const backend = picture.backend;
    // Task 28: ?sessions=p10,q5,r3 runs the drive under the referee (F7 next session, F8 restart)
    if (params.get('sessions')) rac.set_sessions(params.get('sessions'));
    // (the game seeds its rand() with the clock's milliseconds; ?seed=<n>, a test hook: n,
    // as rustyac.exe --race-seed n --ai-seed n-28000)
    const seed = (params.has('seed') ? Number(params.get('seed')) : Date.now()) >>> 0;
    const create = () =>
      launch
        ? rac.Game.create_race(canvas, launch.race, launch.assists, seed, backend, Number(params.get('tex') || 0), Number(params.get('msaa') || 4))
        : rac.Game.create(canvas, car, track, layout, spawn, $('auto-shifter').checked, backend, Number(params.get('tex') || 0), Number(params.get('msaa') || 4));
    let game = await create();
    // Full mode: the renderer names files of its own as it loads (the driver's model, a skin's
    // textures); those, and the game made again
    for (let round = 0; backend === 'full' && round < 6; round++) {
      const wanted = game.files_missing().split('\n').filter(Boolean);
      if (!wanted.length) break;
      await fetchWanted(wanted);
      $('progress').hidden = true;
      game.free();
      game = await create();
    }
    if (launch) window.rustyac.drive = Object.fromEntries(game.drive().split('\n').filter(Boolean).map((line) => [line.slice(0, line.indexOf('=')), line.slice(line.indexOf('=') + 1)]));
    rac.fs_forget_models();
    state.game = game;
    game.set_autodrive(autodrive);
    const notes = `${state.pictureWhy ? state.pictureWhy + '\n' : ''}${megabytes(loaded)} of files in ${fileSeconds.toFixed(1)} s, built in ${((performance.now() - built) / 1000).toFixed(1)} s\n${game.notes()}`;
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
        .concat(
          (params.get('hold') || '')
            .split(',')
            .filter(Boolean)
            .flatMap((item) => {
              const [code, span] = item.split('@');
              const [from, to] = (span || '').split('-');
              const at = (text) => Math.min(steps, /^\d+$/.test(text || '') ? Number(text) : 0);
              return [
                { hold: Number(code), down: true, at: at(from) },
                { hold: Number(code), down: false, at: at(to) },
              ];
            }),
        )
        .sort((a, b) => a.at - b.at);
      const presses = [];
      const before = performance.now();
      state.testing = true;
      $('hud').hidden = false;
      let done = 0;
      for (const { key, at, hold, down } of script) {
        // (a key at the step of the one before it comes a step later: that step has been run)
        game.run_steps(Math.max(0, at - done));
        done = Math.max(at, done);
        if (hold !== undefined) {
          game.key(hold, down);
          continue;
        }
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
  $('mp-join').disabled = !server || !state.source || state.loading || state.running || !!state.online;
}

const isLocal = () => ['127.0.0.1', 'localhost', '[::1]'].includes(location.hostname);
// the on-demand servers the server panel started (its router behind net/x/*); a test on
// this computer names the router's address with ?ondemand=http://127.0.0.1:<port>
const ondemandBase = () => (isLocal() && params.get('ondemand')) || '';
// from the site's root: the page sits in /play/app/ but the router answers at /net/x/*
const ondemandUrl = () => `${ondemandBase()}/net/x/servers.json`;
function onDemandServers(text) {
  const base = ondemandBase();
  return (JSON.parse(text).servers || [])
    .filter((s) => s && /^x-[a-z0-9_-]+$/.test(s.id) && /^\/net\/x\/[a-z0-9_-]+$/.test(s.url))
    .map((s) => ({ ...s, ondemand: true, url: base ? base.replace(/^http/, 'ws') + s.url : s.url }));
}

function showServers(wanted) {
  $('mp-server').replaceChildren(...state.servers.map((s) => new Option(s.name || s.id, s.id)));
  $('mp-server').disabled = state.servers.length === 0;
  if (wanted && state.servers.some((s) => s.id === wanted)) $('mp-server').value = wanted;
}

// The theme asks for its server list each time its online page opens or refreshes, and the
// panel's servers come and go: they are read again then. Synchronous, as the frame's answer is.
function reloadOnDemand() {
  let fresh;
  try {
    const xhr = new XMLHttpRequest();
    xhr.open('GET', ondemandUrl(), false);
    xhr.send();
    if (xhr.status !== 200) return;
    fresh = onDemandServers(xhr.responseText);
  } catch {
    return; // no panel on this site, or it did not answer: the list as it was
  }
  const fixed = state.servers.filter((s) => !s.ondemand && s.id !== 'address');
  state.servers = [...fixed, ...fresh, ...state.servers.filter((s) => s.id === 'address')];
  showServers($('mp-server').value);
  markServers();
}

async function loadServers() {
  let servers = [];
  try {
    const response = await fetch('servers.json', { cache: 'no-cache' });
    if (response.ok) servers = (await response.json()).servers || [];
  } catch {
    // a site without the file has no server list
  }
  try {
    const response = await fetch(ondemandUrl(), { cache: 'no-cache' });
    if (response.ok) servers.push(...onDemandServers(await response.text()));
  } catch {
    // no panel on this site: the fixed servers only
  }
  // (a server from the address bar is a test hook: only on this computer, so that a link
  // to the real site cannot send a visitor's player id to somebody's gateway)
  if (params.get('server') && isLocal()) {
    const cars = (params.get('servercars') || '').split(',').filter(Boolean);
    servers.push({ id: 'address', name: `the address bar's server (${params.get('server')})`, url: params.get('server'), cars, track: params.get('servertrack') || '', password: params.get('serverpass') === '1' });
  }
  state.servers = servers.filter((s) => s && s.id && s.url);
  showServers(params.get('join'));
  if (params.get('join') && $('mp-server').value !== params.get('join')) setMultiplayer(`server ${params.get('join')} is not running now (not in the server list)`);
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

// Why a WebSocket closed, in words: the gateway's own reason when it gave one.
function closeReason(event) {
  if (event.reason) return `${event.reason} (code ${event.code})`;
  if (event.code === 1006) return 'the connection to the gateway broke without a goodbye (code 1006: the network, a proxy, or the computer went to sleep)';
  return `the gateway closed the connection (code ${event.code})`;
}

// The files of these cars on this track (as many rounds as loading turns up more), and the
// checksum files: the bytes that came over.
async function loadServerFiles(cars, track, checksumFiles) {
  let loaded = 0;
  for (let round = 0; round < 8; round++) {
    const wanted = new Set();
    const full = (await pictureBackend()).backend === 'full';
    if (full && round === 0) await listPictureFiles(cars);
    const filesWanted = full ? rac.files_wanted_full : rac.files_wanted;
    for (const car of cars) for (const path of filesWanted(car, track.track, track.layout).split('\n')) if (path) wanted.add(path);
    for (const path of checksumFiles) if (!rac.fs_has(path)) wanted.add(path);
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
  return loaded;
}

// Joins the chosen server: the WebSocket to its gateway, the handshake and the car list (the
// wasm side's client, the desktop's own), the files it runs (fetched
// before the connection when servers.json names them), then the drive.
async function joinServer() {
  const server = state.servers.find((s) => s.id === $('mp-server').value);
  if (!server || state.loading || state.online) return;
  const name = $('mp-name').value.trim() || 'rustyAC driver';
  try {
    localStorage.setItem('rustyac.player-name', name);
  } catch {
    // not kept, then
  }
  if (state.game) {
    // a drive is loaded already (its picture and memory are not taken apart): the page
    // starts afresh and joins
    const next = new URLSearchParams(params);
    for (const key of ['go', 'car', 'track', 'layout']) next.delete(key);
    next.set('join', server.id);
    if ($('mp-car').value) next.set('mpcar', $('mp-car').value);
    location.search = next.toString();
    return;
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
  const started = performance.now();
  let loaded = 0;
  try {
    const url = gatewayUrl(server.url);
    // what servers.json says the server runs is fetched (and checked) before the connection:
    // the server holds a slot only for the drive, never for a download that may break off
    const listedTrack = state.tracks.find((t) => server.track && t.track.toLowerCase() === server.track.toLowerCase() && (t.layout || '') === (server.layout || ''));
    const listedCars = (server.cars || []).map((m) => state.cars.find((c) => c.id.toLowerCase() === m.toLowerCase() && !c.refused)).filter(Boolean).map((c) => c.id);
    if (listedTrack && !listedTrack.refused && listedCars.length) loaded += await loadServerFiles(listedCars, listedTrack, []);
    // one try: a WebSocket to the gateway, the handshake for this model, the car list
    const attempt = async (car) => {
      setMultiplayer(`connecting to ${server.name || server.id} ...`);
      ws = new WebSocket(url);
      ws.binaryType = 'arraybuffer';
      const socket = ws;
      // (a car update is a datagram: when the connection has stalled and they pile up, the
      // new ones are dropped, as UDP would have lost them; the other lane is never dropped)
      const made = new rac.NetLink((bytes) => {
        if (bytes[0] === 1 && socket.bufferedAmount > 16384) return;
        socket.send(bytes);
      });
      if (params.has('arrivals')) made.record(true);
      socket.onmessage = (event) => {
        if (event.data instanceof ArrayBuffer) made.push(new Uint8Array(event.data));
      };
      socket.onclose = (event) => {
        const why = closeReason(event);
        diag('net', `${url}: closed, code ${event.code}${event.reason ? ` (${event.reason})` : ''}, ${event.wasClean ? 'a clean close' : 'not a clean close'}`);
        made.lost(why);
        if (state.online && state.online.ws === socket) {
          state.online.closed = why;
          setMultiplayer(`${state.online.server.name || state.online.server.id}: connection lost: ${why}`);
        }
      };
      await new Promise((resolve, reject) => {
        socket.onopen = resolve;
        socket.onerror = () => reject(new Error(`no connection to ${url}: the gateway is not running there, or it does not let this page's address in`));
      });
      diag('net', `${url}: open, asking for a ${car}`);
      setMultiplayer(`asking ${server.name || server.id} for a ${car} ...`);
      join = rac.NetJoin.start(made, playerId(), name, car, $('mp-password').value);
      for (;;) {
        const status = JSON.parse(join.status());
        if (status.stage === 'done') {
          link = made;
          diag('net', `joined ${status.server}: slot ${status.slot}, ${status.car} at ${status.track}, cars ${status.cars.join(', ')}`);
          return status;
        }
        if (status.stage === 'error') {
          diag('net', `join refused: ${status.error}`);
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
    // checksums of (they are worked out here, from the same files); normally all here already
    const cars = models.map((m) => state.cars.find((c) => c.id.toLowerCase() === m.toLowerCase()).id);
    loaded += await loadServerFiles(cars, track, status.checksum_files);
    const fileSeconds = (performance.now() - started) / 1000;
    $('progress').hidden = true;
    setStatus('building the track and the cars (a large track takes some seconds; the page stands still meanwhile) ...');
    await nextPaint();
    resizeCanvas();
    const built = performance.now();
    const { backend } = await pictureBackend();
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
    const notes = `${state.pictureWhy ? state.pictureWhy + '\n' : ''}${megabytes(loaded)} of files in ${fileSeconds.toFixed(1)} s, built in ${((performance.now() - built) / 1000).toFixed(1)} s\n${game.notes()}`;
    window.rustyac.notes = notes;
    $('notes').textContent = notes;
    $('notes').hidden = false;
    window.rustyac.bindings = game.bindings();
    state.loading = false;
    startDriving();
  } catch (error) {
    state.loading = false;
    $('progress').hidden = true;
    if (join) {
      diag('net', `leaving before the drive: ${error && error.message ? error.message : error}`);
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
function leaveServer(reload, why = 'the page left') {
  const online = state.online;
  if (!online) return;
  state.online = null;
  diag('net', `leaving ${online.server.id}: ${why}`);
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

// Task 28: what the referee says (the session, flags, penalties, results), in words.
function showReferee(referee) {
  window.rustyac.referee = referee;
  const boxes = [['hud-referee', 'top'], ['hud-referee-centre', 'centre'], ['hud-referee-results', 'results']];
  for (const [id, key] of boxes) {
    const lines = referee && !state.online ? referee[key] : [];
    $(id).hidden = lines.length === 0;
    $(id).textContent = lines.join('\n');
  }
}

function showOnline(online) {
  window.rustyac.online = online;
  const box = $('hud-online');
  box.hidden = !online;
  if (!online) return;
  const lines = [`${online.server}   ${online.session}   ping ${online.ping} ms${online.associated ? '' : '   (waiting for the server)'}`];
  for (const d of online.drivers) {
    lines.push(`${d.me ? '>' : ' '} ${(d.name || '?').padEnd(18).slice(0, 18)} ${d.car.padEnd(17).slice(0, 17)} laps ${String(d.laps).padEnd(3)} best ${time(d.best_ms)}${d.me ? '' : `  ${d.ping} ms`}`);
  }
  if (window.rustyac.referee) lines.push(...window.rustyac.referee.top);
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

// ---------------------------------------------------------------- the Controls panel

// The values AC's pad and keyboard classes read from controls.ini (crates/rustyac-web/src/
// controls.rs has the list). The settings are a controls.ini on the wasm side; the page shows
// them, sends one change at a time, and keeps the file's text in localStorage. The classes are
// built again from every change, at once: nothing is reloaded.
const CONTROLS_KEY = 'rustyac.controls';
let controlsNow = null; // what `rac.controls_json()` said last
let controlsStored = true; // false once the browser refused to keep the settings
const controlRows = new Map(); // "SECTION.KEY" -> { box, inputs }

function controlsKept() {
  try {
    const kept = JSON.parse(localStorage.getItem(CONTROLS_KEY) || 'null');
    return kept && typeof kept.text === 'string' ? kept : null;
  } catch {
    // no storage here (a private window, a blocked site): the built-in layout
    return null;
  }
}

function keepControls() {
  try {
    const text = rac.controls_text();
    if (text) localStorage.setItem(CONTROLS_KEY, JSON.stringify({ origin: controlsNow.origin, text }));
    else localStorage.removeItem(CONTROLS_KEY);
    return true;
  } catch {
    return false;
  }
}

// New settings from the wasm side: to the running car, to the storage, to the panel.
function takeControls(json, keep) {
  controlsNow = JSON.parse(json);
  window.rustyac.controls = controlsNow;
  drivingKeys = null;
  if (state.game) {
    state.game.controls_changed();
    window.rustyac.bindings = state.game.bindings();
  }
  if (keep) controlsStored = keepControls();
  showControls();
  if (!$('keys').hidden) $('keys').textContent = keyList();
}

function controlsError(text) {
  $('controls-error').hidden = !text;
  $('controls-error').textContent = text || '';
}

function changeControl(setting, value) {
  try {
    controlsError('');
    takeControls(rac.controls_set(setting.section, setting.key, String(value)), true);
  } catch (error) {
    controlsError(String(error && error.message ? error.message : error));
  }
}

function buildControls() {
  const list = $('controls-list');
  list.replaceChildren();
  controlRows.clear();
  for (const setting of controlsNow.settings) {
    const box = document.createElement('div');
    box.className = 'control';
    const row = document.createElement('label');
    row.className = 'control-row';
    const name = document.createElement('span');
    name.textContent = setting.label;
    row.append(name);
    const inputs = [];
    if (setting.kind === 'number') {
      const slider = document.createElement('input');
      slider.type = 'range';
      const exact = document.createElement('input');
      exact.type = 'number';
      for (const input of [slider, exact]) {
        input.min = setting.min;
        input.max = setting.max;
        input.step = setting.step;
        inputs.push(input);
      }
      // the slider works while it is dragged; the keys go back to the car when it is let go
      slider.addEventListener('input', () => changeControl(setting, slider.value));
      slider.addEventListener('change', () => slider.blur());
      exact.addEventListener('change', () => {
        if (exact.value === '') return;
        const typed = exact.value;
        exact.blur();
        changeControl(setting, typed);
      });
      row.append(slider, exact);
    } else if (setting.kind === 'choice') {
      const select = document.createElement('select');
      select.replaceChildren(...setting.choices.map((choice) => new Option(choice.toLowerCase(), choice)));
      select.addEventListener('change', () => {
        changeControl(setting, select.value);
        select.blur();
      });
      inputs.push(select);
      row.append(select);
    } else {
      const check = document.createElement('input');
      check.type = 'checkbox';
      check.addEventListener('change', () => {
        changeControl(setting, check.checked ? '1' : '0');
        check.blur();
      });
      inputs.push(check);
      row.append(check);
    }
    const builtIn = document.createElement('span');
    builtIn.className = 'default';
    const shown = (value) => (typeof value === 'boolean' ? (value ? 'on' : 'off') : String(value).toLowerCase());
    builtIn.textContent = `default ${shown(setting.default)}`;
    row.append(builtIn);
    const what = document.createElement('p');
    what.className = 'hint';
    what.textContent = `[${setting.section}] ${setting.key}: ${setting.what}`;
    box.append(row, what);
    list.append(box);
    controlRows.set(`${setting.section}.${setting.key}`, { box, inputs });
  }
}

function showControls() {
  if (!controlsNow) return;
  if (controlRows.size !== controlsNow.settings.length) buildControls();
  for (const setting of controlsNow.settings) {
    const { box, inputs } = controlRows.get(`${setting.section}.${setting.key}`);
    box.classList.toggle('changed', setting.value !== setting.default);
    for (const input of inputs) {
      // (the field somebody is typing in keeps its text)
      if (input.type === 'checkbox') input.checked = !!setting.value;
      else if (input !== document.activeElement || input.type === 'range') input.value = setting.value;
    }
  }
  const from = controlsNow.custom ? `In use: ${controlsNow.origin}.` : "In use: the built-in layout (AC's default keys plus WASD, the pad numbers below).";
  $('controls-origin').textContent = from + (!controlsStored ? ' This browser lets the page keep nothing: the settings last until the page is closed.' : controlsNow.custom ? ' Kept in this browser.' : '');
  $('controls-line').textContent = controlsNow.custom ? controlsNow.origin : 'the built-in layout';
  $('controls-layout').replaceChildren(
    ...[{ what: '', keys: 'keyboard', pad: 'Xbox pad' }, ...controlsNow.layout].map((line) => {
      const row = document.createElement('tr');
      for (const text of [line.what, line.keys, line.pad]) {
        const cell = document.createElement('td');
        cell.textContent = text;
        row.append(cell);
      }
      return row;
    }),
  );
  $('controls-not-used').textContent = `Also read by AC's keyboard class and not used here (mouse steering is not in rustyAC): ${controlsNow.not_used.join(', ')}.`;
}

function openControls() {
  showControls();
  $('controls').hidden = false;
  showStorage();
}

// ---------------------------------------------------------------- downloaded data

const DOCS_STORE = 'rustyac-documents-1'; // launcher.js: the player's own launcher settings

// What this browser keeps: the pack's files (under their hashes) by car, track, launcher
// theme and the rest, the player's launcher settings, and what the browser says it allows.
async function storedGroups() {
  const groups = new Map();
  const add = (key, label, size, remove) => {
    const group = groups.get(key) || { key, label, bytes: 0, files: 0, removes: [] };
    group.bytes += size;
    group.files += 1;
    group.removes.push(remove);
    groups.set(key, group);
  };
  const root = navigator.storage && navigator.storage.getDirectory ? await navigator.storage.getDirectory().catch(() => null) : null;
  if (!root) return [];
  const manifest = state.source && state.source.kind === 'pack' ? state.source.manifest : null;
  const byHash = new Map(manifest ? manifest.files.map((f) => [f.sha256, f.path]) : []);
  const theme = new Set(manifest ? manifest.launcher || [] : []);
  const pack = await root.getDirectoryHandle('rustyac-pack-1').catch(() => null);
  if (pack) {
    for await (const [name, handle] of pack.entries()) {
      const size = (await handle.getFile().catch(() => ({ size: 0 }))).size;
      const path = byHash.get(name) || '';
      const parts = path.split('/');
      let key = 'other';
      let label = 'other files';
      if (/^content\/cars\//i.test(path)) [key, label] = [`car:${parts[2]}`, `car ${parts[2]}`];
      else if (/^content\/tracks\//i.test(path)) [key, label] = [`track:${parts[2]}`, `track ${parts[2]}`];
      else if (theme.has(path)) [key, label] = ['theme', 'launcher theme'];
      else if (!path) [key, label] = manifest ? ['old', 'files of an older pack'] : ['old', 'files downloaded from a pack'];
      add(key, label, size, () => pack.removeEntry(name));
    }
  }
  const docs = await root.getDirectoryHandle(DOCS_STORE).catch(() => null);
  if (docs) {
    for await (const [name, handle] of docs.entries()) {
      const size = (await handle.getFile().catch(() => ({ size: 0 }))).size;
      add('documents', 'your launcher settings (Documents)', size, () => docs.removeEntry(name));
    }
  }
  const order = (g) => (g.key === 'documents' ? 2 : g.key.startsWith('car:') || g.key.startsWith('track:') ? 0 : 1);
  return [...groups.values()].sort((a, b) => order(a) - order(b) || a.label.localeCompare(b.label));
}

let storageShown = [];

async function showStorage() {
  storageShown = await storedGroups();
  const total = storageShown.reduce((sum, g) => sum + g.bytes, 0);
  const estimate = navigator.storage && navigator.storage.estimate ? await navigator.storage.estimate().catch(() => null) : null;
  $('storage-total').textContent =
    `${megabytes(total)} kept by this page` + (estimate ? `; the browser counts ${megabytes(estimate.usage || 0)} of ${megabytes(estimate.quota || 0)} allowed for this site` : '');
  $('storage-list').replaceChildren(
    ...storageShown.map((g, index) => {
      const row = document.createElement('tr');
      const tick = document.createElement('input');
      tick.type = 'checkbox';
      tick.dataset.index = String(index);
      tick.setAttribute('aria-label', g.label);
      const cells = [tick, g.label, `${g.files} file${g.files === 1 ? '' : 's'}`, megabytes(g.bytes)].map((content) => {
        const cell = document.createElement('td');
        cell.append(content);
        return cell;
      });
      row.append(...cells);
      return row;
    }),
  );
  if (!storageShown.length) $('storage-list').replaceChildren(Object.assign(document.createElement('caption'), { textContent: 'nothing is kept' }));
}

async function clearStorage(groups) {
  for (const group of groups) {
    for (const remove of group.removes) await remove().catch(() => {});
    diag('cache', `cleared ${group.label} (${megabytes(group.bytes)})`);
  }
  await showStorage();
}

function closeControls() {
  $('controls').hidden = true;
  if (document.activeElement && document.activeElement.closest && document.activeElement.closest('#controls')) document.activeElement.blur();
}

async function loadControlsFile(file) {
  try {
    controlsError('');
    takeControls(rac.controls_load(new Uint8Array(await file.arrayBuffer()), file.name), true);
  } catch (error) {
    controlsError(`${file.name}: ${String(error && error.message ? error.message : error)}`);
  }
}

function startControls() {
  // what an earlier visit kept; a text that no longer reads is dropped
  const kept = controlsKept();
  let json = null;
  if (kept && !params.has('nocontrols')) {
    try {
      json = rac.controls_load(new TextEncoder().encode(kept.text), kept.origin || 'kept in this browser');
    } catch {
      json = null;
    }
  }
  takeControls(json || rac.controls_json(), false);
  $('controls-open').addEventListener('click', openControls);
  $('controls-close').addEventListener('click', closeControls);
  $('controls-load').addEventListener('click', () => $('controls-file').click());
  $('controls-file').addEventListener('change', async (event) => {
    if (event.target.files[0]) await loadControlsFile(event.target.files[0]);
    event.target.value = '';
  });
  $('controls-reset').addEventListener('click', () => {
    controlsError('');
    takeControls(rac.controls_reset(), true);
  });
  $('storage-clear-selected').addEventListener('click', () =>
    clearStorage([...$('storage-list').querySelectorAll('input:checked')].map((tick) => storageShown[Number(tick.dataset.index)])),
  );
  // (read again: the list on show may still be filling)
  $('storage-clear-all').addEventListener('click', async () => clearStorage((await storedGroups()).filter((g) => g.key !== 'documents')));
  $('copy-diagnostics').addEventListener('click', async () => {
    const text = diagnosticsText();
    window.rustyac.diagnostics = text;
    const ok = await navigator.clipboard.writeText(text).then(() => true, () => false);
    $('diagnostics-copied').textContent = ok ? `copied (${text.split('\n').length} lines)` : 'the browser refused the clipboard: the log is in window.rustyac.diagnostics';
  });
}

// A key while the panel is open: Esc closes it (and does not end the drive), and a key typed
// into one of its fields is the field's, not the car's. True when the key was the panel's.
function panelKey(event) {
  if ($('controls').hidden) return false;
  if (event.code === 'Escape') {
    closeControls();
    event.preventDefault();
    return true;
  }
  return !!(event.target && event.target.closest && event.target.closest('#controls') && /^(INPUT|SELECT)$/.test(event.target.tagName));
}

// Test hooks: a whole controls.ini as text, one value, the built-in layout again.
window.rustyac.loadControls = (text, origin) => takeControls(rac.controls_load(new TextEncoder().encode(text), origin || 'a test'), true);
window.rustyac.setControl = (section, key, value) => takeControls(rac.controls_set(section, key, String(value)), true);
window.rustyac.resetControls = () => takeControls(rac.controls_reset(), true);

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
  'gearbox G   pause P   menu Esc   controls O   this list H';
let keysTimer = 0;

function keyList() {
  if (!controlsNow || !controlsNow.custom) return KEY_LIST;
  const width = Math.max(...controlsNow.layout.map((line) => line.what.length));
  return (
    controlsNow.layout.map((line) => `${line.what.padEnd(width)}   ${line.keys}   |   ${line.pad}`).join('\n') +
    '\nABS up, down    Y, Shift+Y   Back + traction-control buttons\n' +
    'pits R   track Shift+R   view F1 / C   new car N\n' +
    'gearbox G   pause P   menu Esc   controls O   this list H'
  );
}

function showKeys(forMs) {
  $('keys').textContent = keyList();
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
  if (state.launch) {
    returnToLauncher().catch(fail);
    return;
  }
  if (state.online) {
    // a car that stands still in the menu would stay on the server's track: Esc leaves
    state.running = false;
    $('menu').hidden = false;
    $('hud').hidden = true;
    $('keys').hidden = true;
    $('chat').hidden = true;
    setMultiplayer(`you left ${state.online.server.name || state.online.server.id}`);
    window.rustyac.state = 'left';
    leaveServer(true, 'the player left (Esc)');
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
  showReferee(hud.referee);
  showOnline(hud.online);
}

// What the display says about the aids right now (a test reads it).
const aidsShown = () => `${$('hud-tc').textContent} | ${$('hud-abs').textContent} | ${$('hud-bias').textContent}`;

// A drive on a server that broke (the connection is gone, the picture failed): the server
// is left properly and the page says how to go on.
function failOnline(error) {
  let text = String(error && error.message ? error.message : error);
  // (the game only knows the connection is gone: the socket knows why)
  if (state.online && state.online.closed) text += `: ${state.online.closed}`;
  leaveServer(false, text);
  $('hud').hidden = true;
  $('chat').hidden = true;
  setMultiplayer('');
  fail(new Error(`${text}. You are off the server: reload the page to join again.`));
  window.rustyac.state = 'error';
}

function frame(now) {
  if (!state.running) return;
  resizeCanvas();
  pollPad();
  if (!state.paused) {
    // (Task 38's frame-rate check: a slow machine, as busy work in every frame)
    for (const until = performance.now() + (window.rustyac.slowFrameMs || 0); performance.now() < until; );
    try {
      state.game.frame(now);
    } catch (error) {
      state.running = false;
      $('menu').hidden = false;
      if (state.online) failOnline(error);
      else fail(error);
      return;
    }
  }
  let hud;
  try {
    hud = JSON.parse(state.game.hud());
  } catch (error) {
    state.running = false;
    $('menu').hidden = false;
    if (state.online) failOnline(error);
    else fail(error);
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
// the rest of what a controls.ini can name
for (let i = 0; i < 10; i++) VK[`Numpad${i}`] = 0x60 + i;
Object.assign(VK, {
  NumpadMultiply: 0x6a, NumpadAdd: 0x6b, NumpadSubtract: 0x6d, NumpadDecimal: 0x6e, NumpadDivide: 0x6f, CapsLock: 0x14,
  Semicolon: 0xba, Equal: 0xbb, Comma: 0xbc, Minus: 0xbd, Period: 0xbe, Slash: 0xbf, Backquote: 0xc0, Backslash: 0xdc, Quote: 0xde,
});

// `rustyac_game::input_file::event`: commands that run before the next physics step.
const EVENT = { RESET: 1, REBUILD: 2, TC_UP: 4, TC_DN: 8, ABS_UP: 16, ABS_DN: 32, AUTO_SHIFTER: 64, TO_TRACK: 256, NEXT_SESSION: 1 << 18, RESTART_SESSION: 1 << 19, PIT_LEFT: 1 << 20, PIT_UP: 1 << 21, PIT_RIGHT: 1 << 22, PIT_DOWN: 1 << 23 };

// The page's own keys. With the keys the built-in layout drives with (asked of the wasm side
// once a car is there) they are the only keys the page keeps from the browser.
const PAGE_KEYS = ['Escape', 'Pause', 'KeyP', 'F1', 'KeyC', 'KeyR', 'KeyN', 'KeyG', 'KeyH', 'KeyT', 'KeyY', 'BracketLeft', 'BracketRight', 'Enter', 'NumpadEnter', 'KeyO', 'F7', 'F8', 'Numpad4', 'Numpad8', 'Numpad6', 'Numpad2'];
// The page's keys that give way to the layout: a key the player's controls.ini drives with
// keeps that job (the desktop's rule for its own plain keys).
const GIVE_WAY = ['KeyT', 'KeyY', 'BracketLeft', 'BracketRight', 'KeyO', 'Numpad4', 'Numpad8', 'Numpad6', 'Numpad2'];
let drivingKeys = null;

function layoutUses(event) {
  if (!drivingKeys) drivingKeys = new Set(state.game.keys_used());
  return drivingKeys.has(VK[event.code]);
}

function pageUses(event) {
  return PAGE_KEYS.includes(event.code) || layoutUses(event);
}

window.addEventListener('keydown', (event) => {
  if (panelKey(event)) return;
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
  switch (GIVE_WAY.includes(event.code) && layoutUses(event) ? '' : event.code) {
    case 'KeyO':
      if ($('controls').hidden) openControls();
      else closeControls();
      return;
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
    // the referee's two buttons, and the pit menu (the number pad's 4 8 6 2)
    case 'F7':
      state.game.request(EVENT.NEXT_SESSION);
      break;
    case 'F8':
      state.game.request(EVENT.RESTART_SESSION);
      break;
    case 'Numpad4':
      state.game.request(EVENT.PIT_LEFT);
      break;
    case 'Numpad8':
      state.game.request(EVENT.PIT_UP);
      break;
    case 'Numpad6':
      state.game.request(EVENT.PIT_RIGHT);
      break;
    case 'Numpad2':
      state.game.request(EVENT.PIT_DOWN);
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
  if (launcherMode && !state.starting) {
    // the player's own folder from now on (the source switch under the theme goes back)
    try {
      localStorage.setItem('rustyac.source', state.fromList ? 'filelist' : 'folder');
    } catch {
      // for this visit only, then
    }
    await launcherStart();
  }
}

// ---------------------------------------------------------------- AC's launcher

// AC's launcher theme (web/launcher.js) is the page. The URL test hooks of a drive without it
// (?go=1, ?join, ?selftest, ?launcher=0) still use the page's own controls, kept hidden.
const launcherMode = params.get('launcher') !== '0' && !['go', 'join', 'selftest'].some((key) => params.has(key));

// The launcher on the source in state.source (its drives run in this page).
async function launcherStart() {
  document.body.classList.add('has-source');
  await openLauncher();
}

// The bar under the theme: the controls page (a gear) and the switch between the site's pack
// and the player's own folder.
function showCorner() {
  const button = $('source-switch');
  const onPack = state.source.kind === 'pack';
  button.hidden = !onPack && !state.packAvailable;
  button.textContent = onPack ? 'Open your Assetto Corsa folder' : 'Use the preview pack';
  button.onclick = onPack
    ? pickFolder
    : () => {
        try {
          localStorage.setItem('rustyac.source', 'pack');
        } catch {
          // nothing kept: the pack is the default anyway
        }
        location.reload();
      };
  // (the picker refuses a folder under Program Files: the file-list way beside it, as on the
  // start page)
  $('source-list').hidden = !onPack || !window.showDirectoryPicker;
  $('source-list').onclick = () => $('folder-input').click();
  $('gear').onclick = openControls;
  $('corner').hidden = false;
}

// The theme's online page in the browser: this site's servers (servers.json) are its lobby's
// entries, Kunos' lobby is never asked. A server's own HTTP port (details, CANJOIN, booking) is
// answered here too: the gateway carries the game's protocol only. Every server is in pickup
// mode (no booking), its player count unknown (0) but for the panel's on-demand servers.
const lobbyIp = (index) => `10.37.0.${index + 1}`;
function launcherLobby() {
  const entry = (s, i) => ({
    ip: lobbyIp(i), port: 9600 + i, cport: 8081 + i, tport: 9600 + i, name: s.name || s.id, clients: s.players || 0, maxclients: s.maxPlayers || Math.max(1, (s.cars || []).length),
    track: s.track ? (s.layout ? `${s.track}-${s.layout}` : s.track) : '', cars: s.cars || [], session: 0, sessiontypes: ['1'], durations: ['0'], timeleft: 0,
    timestamp: 0, lastupdate: 0, country: ['', ''], pass: !!s.password, pickup: true, timed: false, extra: false, l: false, inverted: false, pit: false, timeofday: 0,
  });
  const json = (value) => ({ status: 200, text: JSON.stringify(value), mime: 'application/json' });
  const text = (value) => ({ status: 200, text: value, mime: 'text/plain' });
  const serverAt = (url) => state.servers.findIndex((s, i) => url.startsWith(`http://${lobbyIp(i)}:`));
  return {
    answers: (host) => ['getserverlist', 'getlanservers', 'getserver', 'pingserver'].includes(host),
    request(host, url) {
      if (host === 'getserverlist') {
        reloadOnDemand();
        return json(state.servers.map(entry));
      }
      if (host === 'getlanservers') return json([]);
      const ip = new URLSearchParams(url.slice(url.indexOf('?') + 1)).get('ip');
      const i = state.servers.findIndex((s, n) => lobbyIp(n) === ip);
      if (host === 'getserver') return json(i < 0 ? null : entry(state.servers[i], i));
      // (no ping through the gateway: "n/a", as the theme shows a server that does not answer)
      return json(['-1']);
    },
    owns: (url) => serverAt(url) >= 0,
    http(url) {
      const server = state.servers[serverAt(url)];
      let what = url.slice(url.indexOf('/', 'http://'.length) + 1);
      try {
        what = decodeURIComponent(what);
      } catch {
        // as it is
      }
      if (what.startsWith('JSON')) return json({ Cars: (server.cars || []).map((model) => ({ Model: model, Skin: '', DriverName: '', DriverTeam: '', DriverNation: '', IsConnected: false, IsRequestedGUID: false, IsEntryList: true })) });
      if (what.startsWith('CANJOIN')) return text('YES');
      if (what.startsWith('SUB') || what.startsWith('UNSUB')) return text('OK,0');
      return { status: 404, text: '', mime: 'text/plain' };
    },
  };
}

function launcherReport(kind, text) {
  const l = window.rustyac.launcher;
  if (kind === 'progress') $('content-line').textContent = text;
  else if (kind === 'state') window.rustyac.state = 'launcher';
  else if (kind === 'error') {
    console.error(`launcher: ${text}`);
    l.errors.push(text);
  } else (l[kind] || (l[kind] = [])).push(text);
}

async function openLauncher() {
  if (!navigator.serviceWorker) throw new Error('this browser has no service workers: AC\'s launcher cannot run in it');
  window.rustyac.launcher = { errors: [], blocked: [], notice: [], content: [], starts: 0 };
  await navigator.serviceWorker.register('sw.js');
  await navigator.serviceWorker.ready;
  const host = $('launcher');
  const name = params.get('name') || localStorage.getItem('rustyac.player-name') || 'rustyAC driver';
  state.theme = await startLauncher({
    rac,
    source: state.source,
    frameHost: host,
    player: { name, id: playerId() },
    // the theme's Start: the player's documents are saved, the theme waits behind the drive
    // (in this page: nothing is reloaded) and is told the game runs, as the desktop shell
    // tells it. race.ini's other cars get AI drivers, as on the desktop; Time Attack, Drift
    // and Drag Race are refused before this, in theme_glue.js.
    onStart: async (api) => {
      window.rustyac.launcher.starts++;
      window.rustyac.state = 'launching';
      await api.saveNow();
      api.trigger('ACRunningEvent');
      host.hidden = true;
      $('corner').hidden = true;
      const race = new TextDecoder().decode(api.launcher.read_doc('cfg\\race.ini') || new Uint8Array(0));
      // the game picks skins among a car's skin folders (as the desktop does), the player's
      // and the AI cars': the game's own files hold only what driving reads, so the names
      // go along (only the names are looked at)
      for (const model of raceModels(race)) {
        for (const [skin, dir] of api.list(`content/cars/${model}/skins`)) if (dir) rac.fs_put(`content/cars/${model}/skins/${skin}/ui_skin.json`, new Uint8Array(0));
      }
      driveFromLauncher(api).catch(fail);
    },
    onControls: openControls,
    report: launcherReport,
    lobby: launcherLobby(),
  });
  $('menu').hidden = true;
  host.hidden = false;
  showCorner();
}

// The drive AC's launcher started (its Start, see openLauncher): race.ini and assists.ini as
// the launcher wrote them into the player's documents.
async function driveFromLauncher(api) {
  const text = (rel) => {
    const bytes = api.launcher.read_doc(rel.replace(/\//g, '\\'));
    return bytes ? new TextDecoder().decode(bytes) : null;
  };
  freshCanvas();
  const race = text('cfg/race.ini');
  if (!race) throw new Error("AC's launcher wrote no race.ini");
  const key = (section, name) => {
    const part = race.split(/^\[/m).find((p) => p.toUpperCase().startsWith(`${section}]`)) || '';
    const line = part.split(/\r?\n/).find((l) => l.split('=')[0].trim().toUpperCase() === name);
    return line ? line.slice(line.indexOf('=') + 1).trim() : '';
  };
  window.rustyac.launch = { race_ini: race };
  if (key('REMOTE', 'ACTIVE') === '1') {
    // the theme's Join: the server of [REMOTE] is one of servers.json's (see launcherLobby),
    // joined as the page's own multiplayer join does it
    const index = state.servers.findIndex((s, i) => lobbyIp(i) === key('REMOTE', 'SERVER_IP'));
    if (index < 0) throw new Error(`race.ini's server ${key('REMOTE', 'SERVER_IP')} is not one of this site's servers`);
    state.launch = { online: true };
    $('mp-server').value = state.servers[index].id;
    $('mp-name').value = key('REMOTE', 'NAME');
    $('mp-password').value = key('REMOTE', 'PASSWORD');
    markServers();
    $('mp-car').value = key('REMOTE', 'REQUESTED_CAR');
    await joinServer();
    return;
  }
  state.launch = { car: key('RACE', 'MODEL'), track: key('RACE', 'TRACK'), layout: key('RACE', 'CONFIG_TRACK'), race, assists: text('cfg/assists.ini') };
  await drive(state.launch);
}

// Back to AC's launcher after a drive it started: the referee's race_out.json into the player's
// out folder (as the game writes it when it closes), then the launcher again, which is told
// the game stopped and shows the results.
async function returnToLauncher() {
  state.running = false;
  window.rustyac.state = 'returning';
  const json = state.game && !state.online ? state.game.finish_race() : null;
  leaveServer(false);
  freshCanvas();
  for (const id of ['hud', 'keys', 'chat']) $(id).hidden = true;
  state.launch = null;
  const api = state.theme;
  if (json) api.launcher.write_doc('out\\race_out.json', json);
  await api.saveNow();
  $('launcher').hidden = false;
  $('corner').hidden = false;
  window.rustyac.state = 'launcher';
  api.trigger('ACStoppedEvent');
}

async function main() {
  const wasm = await init();
  window.rustyac.memory = () => ({ wasm_mb: wasm.memory.buffer.byteLength / 2 ** 20, files_mb: rac.fs_megabytes() });
  $('version').textContent = rac.version();
  rac.full_proof_next(Number(params.get('fullproof')) || 0);
  startControls();
  for (const item of (params.get('set') || '').split(',').filter(Boolean)) {
    const [name, value] = [item.slice(0, item.indexOf('=')), item.slice(item.indexOf('=') + 1)];
    controlsNow = JSON.parse(rac.controls_set(name.slice(0, name.indexOf('.')), name.slice(name.indexOf('.') + 1), value));
    window.rustyac.controls = controlsNow;
    showControls();
  }
  $('open-folder').addEventListener('click', pickFolder);
  // Chrome and Edge have the folder picker, and it refuses a folder under Program Files
  // (where Steam puts the game): the file-list way is offered beside it
  if (window.showDirectoryPicker) {
    $('open-folder-list').hidden = false;
    $('folder-list-hint').hidden = false;
    $('open-folder-list').addEventListener('click', () => $('folder-input').click());
  }
  $('folder-input').addEventListener('change', async (event) => {
    try {
      if (!event.target.files.length) return;
      const started = performance.now();
      $('content-line').textContent = `reading the folder: ${event.target.files.length} files listed ...`;
      await nextPaint();
      const folder = folderFromFiles(event.target.files);
      state.fromList = true;
      window.rustyac.listed.tree_ms = performance.now() - started;
      await useFolder(folder, false);
      window.rustyac.listed.ready_ms = performance.now() - started;
    } catch (error) {
      fail(error);
    }
  });
  $('car').addEventListener('change', markRefused);
  $('track').addEventListener('change', markRefused);
  $('car-search').addEventListener('input', searchLists);
  $('track-search').addEventListener('input', searchLists);
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
  try {
    const kept = localStorage.getItem('rustyac.picture');
    if (kept && [...$('backend').options].some((option) => option.value === kept)) $('backend').value = kept;
  } catch (error) {
    console.warn(error);
  }
  $('backend').addEventListener('change', () => {
    try {
      localStorage.setItem('rustyac.picture', $('backend').value);
    } catch (error) {
      console.warn(error);
    }
  });
  if (['webgl', 'full'].includes(params.get('backend'))) $('backend').value = params.get('backend');
  if (params.get('spawn')) $('spawn').value = params.get('spawn');

  let ready = false;
  // (useFolder opens the launcher by itself only for a folder picked later)
  state.starting = true;
  if (launcherMode) document.body.classList.add('launcher-page');
  const base = (params.get('pack') || 'preview/').replace(/([^/])$/, '$1/');
  // the player chose their own folder over the site's pack before: that folder, while the
  // browser still lets the page read it
  let remembered = window.showDirectoryPicker ? await rememberedFolder().catch(() => null) : null;
  let wantFolder = false;
  try {
    wantFolder = launcherMode && localStorage.getItem('rustyac.source') === 'folder';
  } catch {
    // no storage: the pack
  }
  if (params.has('testfolder')) {
    // the folder picker's path without the picker (it needs a click a test cannot give)
    await useFolder(new HttpFolder(params.get('testfolder'), 'assettocorsa (test hook)'), false);
    ready = true;
    state.packAvailable = await fetch(`${base}manifest.json`, { method: 'HEAD' }).then((r) => r.ok).catch(() => false);
  } else if (wantFolder && remembered && (await remembered.queryPermission({ mode: 'read' })) === 'granted') {
    await useFolder(remembered, false).catch(fail);
    ready = !!state.source;
    remembered = null;
    state.packAvailable = await fetch(`${base}manifest.json`, { method: 'HEAD' }).then((r) => r.ok).catch(() => false);
  }
  if (!ready) {
    const pack = await openPack(base).catch((error) => {
      console.warn(error);
      return null;
    });
    if (pack) {
      state.packAvailable = true;
      await dropStaleCopies(pack.manifest);
      rac.fs_list(pack.manifest.files.map((f) => f.path).join('\n'));
      const few = pack.cars.length + pack.tracks.length <= 8;
      const line = few
        ? `preview pack: ${pack.cars.map((c) => `${c.name} (${megabytes(c.bytes)})`).join(', ')}; ${pack.tracks.map((t) => `${t.name} (${megabytes(t.bytes)})`).join(', ')}`
        : `content pack: ${pack.cars.length} cars, ${pack.tracks.length} tracks and layouts`;
      $('pack-line').hidden = false;
      $('pack-line').textContent = `A ${few ? 'preview' : 'content'} pack is on this server (${megabytes(pack.manifest.bytes)} in all; only the chosen car and track are fetched, and kept in this browser for the next visit).`;
      await useSource(pack, line);
      ready = true;
    } else {
      const handle = remembered;
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
        // the start page: no pack, no folder
        window.rustyac.state = 'menu';
        $('content-line').textContent = 'no preview pack on this server: open your own Assetto Corsa folder';
      }
    }
  }
  await loadServers();
  state.starting = false;
  if (launcherMode) {
    if (ready) await launcherStart();
    return;
  }
  if (ready && params.get('join') && $('mp-server').value === params.get('join') && !$('mp-join').disabled) {
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
window.rustyac.leave = () => (state.online || state.launch) && stopDriving();

// Task 37c's proof: a race.ini drive with the inputs of a drive the desktop recorded (`tape`, a
// .ryin's bytes as base64), on the page's files, as `examples/race_proof` runs it natively.
window.rustyac.raceProof = async (race, assists, seed, tape) => {
  await loadRaceFiles(race);
  const bytes = Uint8Array.from(atob(tape), (c) => c.charCodeAt(0));
  const started = performance.now();
  const result = JSON.parse(rac.race_proof(race, assists, seed >>> 0, bytes));
  result.seconds = (performance.now() - started) / 1000;
  return result;
};

// Task 37c's speed test: `steps` physics steps of a race.ini drive (the player on the
// autodriver) as fast as they go, without drawing: how many times real time that is.
window.rustyac.raceSpeed = async (race, assists, seed, skip, steps) => {
  await loadRaceFiles(race);
  const { cars, ms } = JSON.parse(rac.race_speed(race, assists, seed >>> 0, skip, steps));
  return { cars, steps, ms_per_step: ms / steps, real_time_factor: (steps * 3) / ms };
};

// Task 38's frame-rate check: the drive of `race` (seeded by the address's ?seed) in the page's
// own frame loop, every frame made at least `slowMs` long by busy work (0: as fast as the
// display goes), playing `tape` (a .ryin as base64) from the session's first step or, with no
// tape, the autodriver (open the page with ?autodrive=1) until session step `steps`. When the
// loop stopped: the player's car state hash, the state hash of every car
// chained over every step, race_out.json and the frame rate.
window.rustyac.frameProof = async (race, assists, tape, steps, slowMs) => {
  await drive({ car: raceKey(race, 'MODEL'), track: raceKey(race, 'TRACK'), layout: raceKey(race, 'CONFIG_TRACK'), race, assists });
  if (!state.game) throw new Error('the drive did not start');
  await loadRaceFiles(race);
  window.rustyac.slowFrameMs = slowMs;
  if (tape) state.game.play_tape(race, assists, Number(params.get('seed')) >>> 0, Uint8Array.from(atob(tape), (c) => c.charCodeAt(0)));
  else {
    state.game.set_autodrive(true);
    state.game.stop_at_step(steps);
  }
  const started = performance.now();
  let frames = 0;
  while (!JSON.parse(state.game.check_state()).stopped) {
    await new Promise((resolve) => requestAnimationFrame(resolve));
    frames++;
  }
  const seconds = (performance.now() - started) / 1000;
  window.rustyac.slowFrameMs = 0;
  return { ...JSON.parse(state.game.check_state()), race_out: ((bytes) => (bytes ? new TextDecoder().decode(bytes) : null))(state.game.finish_race()), fps: frames / seconds, seconds };
};

// Full mode's proof (?fullproof=<frames>, Game.full_proof): the drive's fixed frame stays on the
// canvas; with nothing over it, its kgl call log
window.rustyac.fullProof = async () => {
  const text = state.game && state.game.full_proof();
  if (!text) throw new Error('this drive is not a Full proof (?fullproof=<frames>&backend=full)');
  for (const node of document.body.children) if (node.id !== 'view') node.style.visibility = 'hidden';
  await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const [size, ...log] = text.split('\n');
  return { size, log: log.join('\n') };
};

// Task 38j's test hooks: the drive of this race.ini (and assists.ini) after the last one, as the
// launcher's Start makes it; the wasm memory and the files in it, MB
window.rustyac.driveRace = (race, assists) => freshCanvas() || drive({ car: raceKey(race, 'MODEL'), track: raceKey(race, 'TRACK'), layout: raceKey(race, 'CONFIG_TRACK'), race, assists });

// a [RACE] key of a race.ini
function raceKey(race, name) {
  const part = race.split(/^\[/m).find((p) => /^RACE\]/i.test(p)) || '';
  return ((part.split(/\r?\n/).find((l) => l.split('=')[0].trim().toUpperCase() === name) || '=').split('=')[1] || '').trim();
}

async function loadRaceFiles(race) {
  const [track, layout] = [raceKey(race, 'TRACK'), raceKey(race, 'CONFIG_TRACK')];
  const models = raceModels(race);
  for (let round = 0; round < 8; round++) {
    const wanted = [...new Set(models.flatMap((model) => rac.files_wanted(model, track, layout).split('\n').filter(Boolean)))];
    if (!wanted.length) break;
    await loadFiles(wanted, () => {});
  }
  await loadFiles(['system/cfg/assetto_corsa.ini', 'system/cfg/pitstop.ini'], () => {}).catch(() => {});
}

main().catch(fail);
