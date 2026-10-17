// SPDX-License-Identifier: GPL-3.0-or-later
//
// AC's launcher in the page: the theme of the player's game (from the preview pack, or from the
// player's own Assetto Corsa folder) in a frame, its `ac://` requests answered by the desktop
// launcher's own bridge built for the browser (rustyac_web's Launcher), the player's documents
// (Documents\Assetto Corsa: cfg, out, launcherdata ...) kept in this browser's storage.
//
// The frame's files come through the page's service worker (sw.js) from here; the glue
// (theme_glue.js) runs in the frame and calls window.rustyacTheme.

const TEXT = /\.(ini|json|js|html?|css|txt|less|lut|acd_?)$/i;
const DOCS_STORE = 'rustyac-documents-1';
const enc = new TextEncoder();
const dec = new TextDecoder();

const MIME = {
  html: 'text/html', htm: 'text/html', js: 'text/javascript', css: 'text/css', json: 'application/json', ini: 'text/plain', txt: 'text/plain',
  png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', svg: 'image/svg+xml', wav: 'audio/wav',
};
export function mimeOf(path) {
  const dot = path.lastIndexOf('.');
  return MIME[dot < 0 ? '' : path.slice(dot + 1).toLowerCase()] || 'application/octet-stream';
}

// ---------------------------------------------------------------- the game's folder, for the bridge

// What the launcher looks at in the game's folder: Map(lower-case path -> entry), the folder
// itself under ''. An entry: { name, dir, size, mtime, children: Set of lower-case paths }.
class Tree {
  constructor() {
    this.entries = new Map([['', { name: '', dir: true, size: 0, mtime: 0, children: new Set() }]]);
    this.bytes = new Map();
  }

  folder(path) {
    const lower = path.toLowerCase();
    if (this.entries.has(lower)) return this.entries.get(lower);
    const cut = path.lastIndexOf('/');
    const parent = this.folder(cut < 0 ? '' : path.slice(0, cut));
    const entry = { name: path.slice(cut + 1), dir: true, size: 0, mtime: 0, children: new Set() };
    this.entries.set(lower, entry);
    parent.children.add(lower);
    return entry;
  }

  file(path, size, mtime) {
    const lower = path.toLowerCase();
    const cut = path.lastIndexOf('/');
    const parent = this.folder(cut < 0 ? '' : path.slice(0, cut));
    this.entries.set(lower, { name: path.slice(cut + 1), dir: false, size, mtime: mtime || 0, path });
    parent.children.add(lower);
  }
}

// The parts of a player's folder the launcher reads or shows, walked through a directory
// handle (the folder picker's, or the test hook's HttpFolder).
async function walkFolder(root, tree, report) {
  const children = async (handle) => {
    const out = [];
    for await (const [name, child] of handle.entries()) out.push([name, child]);
    return out;
  };
  const child = async (handle, name) => {
    for (const [n, c] of await children(handle)) if (n.toLowerCase() === name) return c;
    return null;
  };
  let files = 0;
  const walk = async (handle, path, depth, keep) => {
    for (const [name, c] of await children(handle)) {
      const p = path ? `${path}/${name}` : name;
      if (c.kind === 'directory') {
        tree.folder(p);
        if (depth > 0) await walk(c, p, depth - 1, keep);
      } else if (keep(p.toLowerCase())) {
        tree.file(p, -1, 0);
        tree.handles.set(p.toLowerCase(), c);
        if (++files % 200 === 0) report(`reading the folder for the launcher: ${files} files`);
      }
    }
  };
  tree.handles = new Map();
  const all = () => true;
  const under = async (path, depth, keep) => {
    let handle = root;
    for (const part of path.split('/')) {
      handle = await child(handle, part.toLowerCase());
      if (!handle || handle.kind !== 'directory') return;
    }
    tree.folder(path);
    await walk(handle, path, depth, keep);
  };
  await under('cfg', 4, all);
  await under('system/cfg', 2, all);
  await under('launcher/themes', 8, (p) => !p.endsWith('.ogv'));
  await under('content/weather', 1, (p) => p.endsWith('.ini'));
  await under('content/showroom', 2, (p) => p.endsWith('.json'));
  await under('apps/python', 0, () => false);
  // every car: its menu entry and pictures, its skins' entries and pictures
  const cars = await child(root, 'content').then((c) => (c ? child(c, 'cars') : null));
  if (cars) {
    tree.folder('content/cars');
    for (const [name, car] of await children(cars)) {
      if (car.kind !== 'directory') continue;
      const base = `content/cars/${name}`;
      tree.folder(base);
      for (const [n, c] of await children(car)) {
        const lower = n.toLowerCase();
        if (c.kind !== 'directory') {
          tree.file(`${base}/${n}`, -1, 0);
          tree.handles.set(`${base}/${n}`.toLowerCase(), c);
        } else if (lower === 'ui') {
          tree.folder(`${base}/${n}`);
          await walk(c, `${base}/${n}`, 3, all);
        } else if (lower === 'skins') {
          tree.folder(`${base}/${n}`);
          await walk(c, `${base}/${n}`, 1, (p) => /\/(ui_skin\.json|preview\.jpg|livery\.png)$/.test(p));
        } else {
          tree.folder(`${base}/${n}`);
        }
      }
    }
  }
  const tracks = await child(root, 'content').then((c) => (c ? child(c, 'tracks') : null));
  if (tracks) {
    tree.folder('content/tracks');
    for (const [name, track] of await children(tracks)) {
      if (track.kind !== 'directory') continue;
      tree.folder(`content/tracks/${name}`);
      const ui = await child(track, 'ui');
      if (ui && ui.kind === 'directory') {
        tree.folder(`content/tracks/${name}/ui`);
        await walk(ui, `content/tracks/${name}/ui`, 3, (p) => !p.endsWith('.ogv'));
      }
    }
  }
  // the bridge reads every text file it looks at (menu entries, cfg, the theme's scripts)
  const wanted = [...tree.handles.keys()].filter((p) => TEXT.test(p) || p.endsWith('/module.ini'));
  let read = 0;
  for (const path of wanted) {
    const file = await tree.handles.get(path).getFile();
    const entry = tree.entries.get(path);
    entry.size = file.size;
    entry.mtime = file.lastModified || 0;
    tree.bytes.set(path, new Uint8Array(await file.arrayBuffer()));
    if (++read % 100 === 0) report(`reading the folder for the launcher: ${read} of ${wanted.length} small files`);
  }
}

// The launcher's part of a preview pack: what tools/web_pack put in `launcher`.
async function packTree(pack, tree, report) {
  const launcher = new Set(pack.manifest.launcher || []);
  pack.manifest.files.forEach((f, i) => {
    tree.file(f.path, f.bytes, 0);
    if (launcher.has(i) && TEXT.test(f.path)) tree.wanted = (tree.wanted || []).concat(f.path);
  });
  let read = 0;
  const wanted = tree.wanted || [];
  // (a few at a time: they are small, and many)
  for (let at = 0; at < wanted.length; at += 16) {
    await Promise.all(
      wanted.slice(at, at + 16).map(async (path) => {
        tree.bytes.set(path.toLowerCase(), await readAll(pack, path));
      }),
    );
    read = Math.min(wanted.length, at + 16);
    report(`the launcher: ${read} of ${wanted.length} files`);
  }
}

async function readAll(source, path) {
  const file = await source.open(path);
  const reader = file.stream.getReader();
  const parts = [];
  let keep = file.keep || null;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    parts.push(value);
    if (keep) await keep.write(value).catch(() => (keep = null));
  }
  if (keep) await keep.close().catch(() => {});
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

// ---------------------------------------------------------------- the player's documents

async function docsFile(create) {
  if (!navigator.storage || !navigator.storage.getDirectory) return null;
  try {
    const root = await navigator.storage.getDirectory();
    const folder = await root.getDirectoryHandle(DOCS_STORE, { create });
    return await folder.getFileHandle('documents.bin', { create });
  } catch {
    return null;
  }
}

export async function loadDocuments() {
  const handle = await docsFile(false);
  if (!handle) return new Uint8Array(0);
  return new Uint8Array(await (await handle.getFile()).arrayBuffer());
}

async function saveDocuments(bytes) {
  const handle = await docsFile(true);
  if (!handle) return;
  const out = await handle.createWritable();
  await out.write(bytes);
  await out.close();
}

// ---------------------------------------------------------------- the launcher

// env: { rac, source, frameHost (element), player: {name, id}, onStart(launcher),
//        onControls(), report(kind, text), lobby (online answers, or null) }
export async function startLauncher(env) {
  const { rac, source } = env;
  const tree = new Tree();
  const say = (text) => env.report('progress', text);
  if (source.kind === 'pack') await packTree(source, tree, say);
  else await walkFolder(source.root, tree, say);

  const misses = [];
  const syncUrl = source.kind === 'pack' ? source.urlOf : source.root && source.root.url ? (p) => source.root.url + p.split('/').map(encodeURIComponent).join('/') : null;
  const bytesOf = (rel) => {
    const lower = rel.toLowerCase();
    if (tree.bytes.has(lower)) return tree.bytes.get(lower);
    const entry = tree.entries.get(lower);
    if (!entry || entry.dir) return undefined;
    // a file not read before the theme started: over HTTP it can still be read at once
    if (syncUrl) {
      const xhr = new XMLHttpRequest();
      xhr.open('GET', syncUrl(entry.path), false);
      xhr.overrideMimeType('text/plain; charset=x-user-defined');
      xhr.send();
      if (xhr.status === 200) {
        const text = xhr.responseText;
        const out = new Uint8Array(text.length);
        for (let i = 0; i < text.length; i++) out[i] = text.charCodeAt(i) & 0xff;
        tree.bytes.set(lower, out);
        return out;
      }
    }
    misses.push(rel);
    return undefined;
  };
  const base = {
    kind: (rel) => {
      const e = tree.entries.get(rel.toLowerCase());
      return e ? (e.dir ? 2 : 1) : 0;
    },
    list: (rel) => {
      const e = tree.entries.get(rel.toLowerCase());
      if (!e || !e.dir) return [];
      return [...e.children].map((c) => {
        const x = tree.entries.get(c);
        return [x.name, x.dir];
      });
    },
    read: (rel) => bytesOf(rel),
    size: (rel) => {
      const e = tree.entries.get(rel.toLowerCase());
      if (!e || e.dir) return -1;
      if (e.size < 0 && tree.bytes.has(rel.toLowerCase())) e.size = tree.bytes.get(rel.toLowerCase()).length;
      return e.size;
    },
    modified: (rel) => {
      const e = tree.entries.get(rel.toLowerCase());
      return e ? e.mtime : -1;
    },
  };

  const snapshot = await loadDocuments();
  const launcher = new rac.Launcher(base, { name: env.player.name, id: env.player.id, width: screen.width, height: screen.height }, snapshot);
  let saving = null;
  const save = () => {
    clearTimeout(saving);
    saving = setTimeout(() => saveDocuments(launcher.snapshot()).catch((e) => env.report('error', `documents: ${e}`)), 300);
  };
  const saveNow = async () => {
    clearTimeout(saving);
    await saveDocuments(launcher.snapshot());
  };

  // the theme: the one launcher.ini names when the game's folder has it, else the default
  let ini = dec.decode(launcher.read_doc('cfg\\launcher.ini') || new Uint8Array(0));
  // Drive opens on the mode it was left on ([SAVED] DRIVE): one a browser cannot run is not
  // reopened, Practice is (what the theme opens when the key is not there)
  const saved = /^(\s*DRIVE\s*=\s*)(timeattack|drift|dragrace)\s*$/im;
  if (saved.test(ini)) {
    ini = ini.replace(saved, '$1practice');
    launcher.write_doc('cfg\\launcher.ini', enc.encode(ini));
    save();
  }
  const named = (/^\s*theme\s*=\s*(.*)$/im.exec(ini.split(/^\[/m).find((s) => /^window\]/i.test(s)) || '') || [])[1];
  const themeOf = (id) => {
    const folder = `launcher/themes/${id}`;
    const text = tree.bytes.get(`${folder}/theme.ini`.toLowerCase());
    if (!text || !tree.entries.has(`${folder}/index.html`)) return null;
    const t = dec.decode(text);
    const key = (k) => (new RegExp(`^\\s*${k}\\s*=\\s*(.*?)\\s*$`, 'im').exec(t) || [])[1] || '';
    return { id, folder: `ac/${folder}`, loadDefault: key('loaddefault') === 'true', scripts: key('scripts').split(',') };
  };
  const theme = (named && named.trim() && themeOf(named.trim())) || themeOf('default');
  if (!theme) throw new Error('no launcher theme in the game files (launcher/themes/default)');

  const acfsBase = new URL('acfs/', location.href).href;
  const roots = rac.Launcher.roots();
  // (net.rs uri_path: of these two names only the space is encoded)
  const uri = (path) => 'file:///' + path.replace(/\\/g, '/').replace(/ /g, '%20');
  const prefixes = [
    [uri(roots[0]) + '/', `${acfsBase}ac/`],
    [uri(roots[1]) + '/', `${acfsBase}docs/`],
  ];
  const rewrite = (text) => {
    for (const [from, to] of prefixes) if (text.includes(from)) text = text.split(from).join(to);
    return text;
  };

  const frame = document.createElement('iframe');
  frame.id = 'theme';
  frame.title = 'Assetto Corsa';
  const call = (fn) => {
    try {
      const w = frame.contentWindow;
      if (w && w.__rustyac) fn(w.__rustyac);
    } catch (e) {
      env.report('error', String(e));
    }
  };
  const trigger = (name, args) => call((g) => g.trigger(name, args));
  const enterTheme = (event) => {
    trigger(event);
    trigger('ACVisible');
    // the browser has only the Drive tile: it is opened at once
    setTimeout(() => call((g) => g.exec("$('#menu li[data-panel=drive]').click();", 'rustyac: drive')), 0);
    if (env.afterIntro) setTimeout(() => env.afterIntro(api), 400);
  };

  const hostOf = (url) => {
    const rest = url.replace(/^ac:\/\//i, '');
    return rest.split(/[/?]/)[0].toLowerCase();
  };
  const paramOf = (url, key) => {
    const q = url.indexOf('?');
    return q < 0 ? null : new URLSearchParams(url.slice(q + 1)).get(key);
  };

  const api = {
    acfsBase,
    theme,
    mimeOf,
    launcher,
    trigger,
    frame,
    misses,
    save,
    saveNow,
    report: (kind, text) => env.report(kind, text),
    isOnline: (url) => !!(env.lobby && env.lobby.owns(url)),
    online: (url, method, body) => env.lobby.http(url, method, body),
    text(path) {
      const lower = path.toLowerCase();
      if (lower.startsWith('ac/')) {
        const b = tree.bytes.get(lower.slice(3));
        return b ? dec.decode(b) : null;
      }
      if (lower.startsWith('docs/')) {
        const b = launcher.read_doc(path.slice(5).replace(/\//g, '\\'));
        return b ? dec.decode(b) : null;
      }
      return null;
    },
    request(url, method, body) {
      const posted = url.startsWith('http://ac/');
      const acUrl = posted ? url.replace('http://ac/', 'ac://') : url;
      const post = posted || method === 'POST' ? String(body ?? '') : undefined;
      const host = hostOf(acUrl);
      if (env.lobby && env.lobby.answers(host)) return env.lobby.request(host, acUrl, post);
      const a = launcher.request(acUrl, post);
      const text = rewrite(dec.decode(a.body));
      save();
      // the shell's part, after the answer (SchemeEvent, the bridge's events)
      setTimeout(() => {
        if (host === 'uiready' || host === 'forcetotheme') enterTheme('ACIntroEvent');
        if (host === 'messagebox') call((g) => g.notice(paramOf(acUrl, 'msg') || ''));
        for (const name of a.events) {
          if (name === 'Start') env.onStart(api);
          else if (name === 'Home') frame.src = frame.src;
          else if (name === 'OpenControls') env.onControls();
          else if (name.startsWith('ContentErrors:')) env.report('content', name.slice(14));
        }
      }, 0);
      return { status: 200, text, mime: a.mime };
    },
    loaded() {
      env.report('state', 'theme');
    },
  };
  window.rustyacTheme = api;

  // the service worker asks for the frame's files here
  navigator.serviceWorker.addEventListener('message', async (event) => {
    const data = event.data;
    if (!data || data.kind !== 'rustyac-acfs' || !event.ports[0]) return;
    const port = event.ports[0];
    const path = String(data.path);
    const lower = path.toLowerCase();
    try {
      let bytes = null;
      if (lower.startsWith('ac/')) {
        const rel = lower.slice(3);
        bytes = tree.bytes.get(rel) || null;
        const entry = tree.entries.get(rel);
        if (!bytes && entry && !entry.dir) {
          if (tree.handles && tree.handles.has(rel)) bytes = new Uint8Array(await (await tree.handles.get(rel).getFile()).arrayBuffer());
          else if (source.kind === 'pack') bytes = await readAll(source, entry.path);
        }
      } else if (lower.startsWith('docs/')) {
        bytes = launcher.read_doc(path.slice(5).replace(/\//g, '\\')) || null;
      }
      port.postMessage({ bytes });
    } catch (e) {
      env.report('error', `${path}: ${e}`);
      port.postMessage({ bytes: null });
    }
  });
  navigator.serviceWorker.startMessages();

  env.frameHost.replaceChildren(frame);
  frame.src = `${acfsBase}${theme.folder}/index.html`;
  return api;
}

export { enc, dec };

// ---------------------------------------------------------------- the documents snapshot

// The snapshot's files (Launcher::snapshot: `D <path>` lines, `F <path>\t<ms>\t<length>` lines
// each followed by the bytes): [{ path, ms, bytes }] and the folder lines as they are.
function parseSnapshot(snap) {
  const files = [];
  const folders = [];
  let at = 0;
  while (at < snap.length) {
    const end = snap.indexOf(10, at);
    if (end < 0) break;
    const line = dec.decode(snap.subarray(at, end));
    at = end + 1;
    if (line.startsWith('D ')) folders.push(line.slice(2));
    else if (line.startsWith('F ')) {
      const [path, ms, len] = line.slice(2).split('\t');
      const n = Number(len);
      files.push({ path, ms: Number(ms), bytes: snap.subarray(at, at + n) });
      at += n;
    } else break;
  }
  return { files, folders };
}

function sameDoc(path, rel) {
  const norm = (p) => p.replace(/\//g, '\\').toLowerCase();
  return norm(path).endsWith('\\' + norm(rel));
}

// A file of the player's folder in a snapshot (`cfg/race.ini`), or null.
export function snapshotFile(snap, rel) {
  const file = parseSnapshot(snap).files.find((f) => sameDoc(f.path, rel));
  return file ? file.bytes : null;
}

// The snapshot with a file of the player's folder written (its folder must be in it).
export function snapshotWith(snap, root, rel, bytes, ms) {
  const { files, folders } = parseSnapshot(snap);
  const kept = files.filter((f) => !sameDoc(f.path, rel));
  kept.push({ path: `${root}\\${rel.replace(/\//g, '\\')}`, ms, bytes });
  const parts = folders.map((f) => enc.encode(`D ${f}\n`));
  for (const f of kept) parts.push(enc.encode(`F ${f.path}\t${f.ms}\t${f.bytes.length}\n`), f.bytes);
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

export { saveDocuments };
