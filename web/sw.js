// SPDX-License-Identifier: GPL-3.0-or-later
//
// The page's service worker: it serves the files of the game's folder and of the player's folder
// to AC's launcher theme, which runs in a frame at `acfs/ac/launcher/themes/<theme>/index.html`
// as the desktop launcher runs it from `file:///<game>/launcher/themes/<theme>/index.html`.
// The bytes come from the page that opened the frame (the preview pack, or the player's own
// folder: nothing of Kunos' is on this site's server). Everything else passes through.
//
//   acfs/ac/<path>     a file of the game's folder
//   acfs/docs/<path>   a file of the player's folder (Documents\Assetto Corsa)

const MIME = {
  html: 'text/html; charset=utf-8', htm: 'text/html; charset=utf-8', js: 'text/javascript; charset=utf-8', css: 'text/css; charset=utf-8',
  json: 'application/json; charset=utf-8', ini: 'text/plain; charset=utf-8', txt: 'text/plain; charset=utf-8', png: 'image/png',
  jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', svg: 'image/svg+xml', wav: 'audio/wav', ogg: 'audio/ogg', mp3: 'audio/mpeg',
  ogv: 'video/ogg', woff: 'font/woff', woff2: 'font/woff2', ttf: 'font/ttf', otf: 'font/otf', ico: 'image/x-icon', dds: 'image/vnd-ms.dds',
};

// The theme reaches nothing outside this site: no other server, ever (its news feed, Kunos'
// lobby, a server's own HTTP port: the glue answers what the theme needs of them).
const THEME_POLICY = "default-src 'self' 'unsafe-inline' 'unsafe-eval' blob: data:; connect-src 'self'; frame-src 'self'";

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (event) => event.waitUntil(self.clients.claim()));

function mimeOf(path) {
  const dot = path.lastIndexOf('.');
  return MIME[dot < 0 ? '' : path.slice(dot + 1).toLowerCase()] || 'application/octet-stream';
}

// The page holding the files: a window of this site that is not itself a theme frame.
async function owner() {
  const all = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
  return all.find((c) => !new URL(c.url).pathname.includes('/acfs/')) || null;
}

async function ask(path) {
  const page = await owner();
  if (!page) return null;
  const channel = new MessageChannel();
  const answer = new Promise((resolve) => {
    const timer = setTimeout(() => resolve(null), 60000);
    channel.port1.onmessage = (event) => {
      clearTimeout(timer);
      resolve(event.data && event.data.bytes ? event.data.bytes : null);
    };
  });
  page.postMessage({ kind: 'rustyac-acfs', path }, [channel.port2]);
  return answer;
}

async function serve(request, path) {
  const bytes = await ask(path);
  if (!bytes) return new Response(`${path}: no such file`, { status: 404, headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
  const headers = { 'Content-Type': mimeOf(path), 'Cache-Control': 'no-store' };
  if (/^ac\/launcher\/themes\/[^/]+\/index\.html?$/i.test(path)) {
    // the glue goes first into the theme's page, before any of the theme's own markup
    const glue = new URL('theme_glue.js', self.registration.scope).href;
    let text = new TextDecoder().decode(bytes);
    text = text.replace(/<head[^>]*>/i, (head) => `${head}<script src="${glue}"></script>`);
    headers['Content-Security-Policy'] = THEME_POLICY;
    return new Response(text, { status: 200, headers });
  }
  return new Response(bytes, { status: 200, headers });
}

self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin) return;
  const base = new URL('acfs/', self.registration.scope).pathname;
  if (!url.pathname.startsWith(base)) return;
  let path;
  try {
    path = decodeURIComponent(url.pathname.slice(base.length));
  } catch {
    path = url.pathname.slice(base.length);
  }
  event.respondWith(serve(event.request, path));
});
