// SPDX-License-Identifier: GPL-3.0-or-later
//
// The glue the service worker puts first into AC's launcher theme (web/sw.js). It does in the
// page what the desktop launcher's shell does around the theme (crates/rustyac-launcher/src/
// shell.rs): `ac://` and `http://ac/` requests go to the launcher's bridge (the page's wasm
// Launcher, through window.parent.rustyacTheme), the shared scripts and the theme's own are run
// after the page has loaded (JSIncludeDefaultLibs), and the page raises the theme's events.
//
// The browser shows only the Drive tile: the other tiles of the theme's left column are hidden,
// and the modes a browser cannot run show the theme's own notice. Nothing reaches the network
// outside this site: a request elsewhere fails here, before it is made.
(function () {
  'use strict';
  const T = window.parent && window.parent !== window ? window.parent.rustyacTheme : null;
  if (!T) return;

  const acfs = new URL(T.acfsBase, location.href);

  // ------------------------------------------------------------ hidden behind a drive

  // Task 42c: while a drive hides the frame (`pause(true)`) the theme's timers stop: its
  // intervals (the Special Events page asks `ac://eventcount` every second, jQuery's
  // animations) are cleared and made again when the frame shows; a timeout that comes due
  // meanwhile runs then. AC's own launcher window keeps running behind the game; a page has
  // one thread for both.
  let hidden = false;
  const held = [];
  const intervals = new Map(); // the theme's id -> [the browser's id, fn, ms]
  const real = { setTimeout: window.setTimeout, setInterval: window.setInterval, clearInterval: window.clearInterval };
  window.setTimeout = function (fn, ms, ...args) {
    if (typeof fn !== 'function') return real.setTimeout.call(window, fn, ms, ...args);
    return real.setTimeout.call(window, () => (hidden ? held.push(() => fn(...args)) : fn(...args)), ms);
  };
  let nextInterval = 1;
  window.setInterval = function (fn, ms, ...args) {
    const tick = typeof fn === 'function' ? () => fn(...args) : fn;
    const id = nextInterval++;
    intervals.set(id, [hidden ? null : real.setInterval.call(window, tick, ms), tick, ms]);
    return id;
  };
  window.clearInterval = function (id) {
    const it = intervals.get(id);
    if (!it) return;
    if (it[0] !== null) real.clearInterval.call(window, it[0]);
    intervals.delete(id);
  };
  function pause(on) {
    if (on === hidden) return;
    hidden = on;
    for (const it of intervals.values()) {
      if (on) real.clearInterval.call(window, it[0]);
      it[0] = on ? null : real.setInterval.call(window, it[1], it[2]);
    }
    if (!on) for (const f of held.splice(0)) f();
  }

  // ------------------------------------------------------------ requests

  // Where a request goes: the bridge, a file the page already holds, the page's online answers,
  // the network of this site, or nowhere.
  function route(url) {
    let u;
    try {
      u = new URL(url, location.href);
    } catch {
      return { kind: 'blocked' };
    }
    if (u.protocol === 'ac:' || u.href.startsWith('http://ac/')) return { kind: 'bridge' };
    if (T.isOnline(u.href)) return { kind: 'online' };
    if (u.origin !== location.origin) return { kind: 'blocked' };
    if (u.pathname.startsWith(acfs.pathname)) {
      let path = u.pathname.slice(acfs.pathname.length);
      try {
        path = decodeURIComponent(path);
      } catch {
        // as it is
      }
      return { kind: 'file', path };
    }
    return null;
  }

  function answer(where, method, url, body, sync) {
    if (where.kind === 'bridge') return T.request(url, method, body);
    if (where.kind === 'online') return T.online(url, method, body);
    if (where.kind === 'file') {
      const text = T.text(where.path);
      if (text !== null) return { status: 200, text, mime: T.mimeOf(where.path) };
      return sync ? { status: 404, text: '', mime: 'text/plain' } : null;
    }
    T.report('blocked', `${method} ${url}`);
    return { status: 0, text: '', mime: 'text/plain' };
  }

  // A stylesheet the theme links from the bridge (`<link href="ac://generatecss">`, the skins'
  // previews): WebView2 loads it through the ac scheme, a page cannot; its text goes into a
  // <style> in its place.
  function bridgeStyles(nodes) {
    for (const node of nodes) {
      if (node.nodeName !== 'LINK' || !/^(ac:|http:\/\/ac\/)/i.test(node.getAttribute('href') || '')) continue;
      const a = T.request(node.getAttribute('href'), 'GET');
      const style = document.createElement('style');
      style.textContent = a.text;
      node.replaceWith(style);
    }
  }
  new MutationObserver((list) => list.forEach((m) => bridgeStyles(m.addedNodes))).observe(document, { childList: true, subtree: true });

  const Real = window.XMLHttpRequest;
  class Xhr extends Real {
    open(method, url, async) {
      this._m = String(method || 'GET').toUpperCase();
      this._u = String(url);
      this._a = async !== false;
      this._w = route(this._u);
      this._r = null;
      this._rs = 1;
      if (this._w && this._w.kind === 'file' && this._a) {
        // a file the page holds is answered here; any other goes to the service worker
        if (T.text(this._w.path) === null) this._w = null;
      }
      if (!this._w) return super.open(...arguments);
    }
    setRequestHeader(name, value) {
      if (!this._w) super.setRequestHeader(name, value);
    }
    send(body) {
      if (!this._w) return super.send(body);
      const done = () => {
        if (this._aborted) return;
        let a;
        try {
          a = answer(this._w, this._m, this._u, body === undefined ? null : body, !this._a);
        } catch (e) {
          T.report('error', `${this._u}: ${e}`);
          a = { status: 0, text: '', mime: 'text/plain' };
        }
        this._r = a;
        this._rs = 4;
        this.dispatchEvent(new Event('readystatechange'));
        this.dispatchEvent(new ProgressEvent(a.status ? 'load' : 'error'));
        this.dispatchEvent(new ProgressEvent('loadend'));
      };
      if (this._a) setTimeout(done, 0);
      else done();
    }
    abort() {
      if (!this._w) return super.abort();
      this._aborted = true;
    }
    get readyState() {
      return this._w ? this._rs : super.readyState;
    }
    get status() {
      return this._w ? (this._r ? this._r.status : 0) : super.status;
    }
    get statusText() {
      return this._w ? (this._r && this._r.status === 200 ? 'OK' : '') : super.statusText;
    }
    get responseText() {
      return this._w ? (this._r ? this._r.text : '') : super.responseText;
    }
    get response() {
      return this._w ? (this._r ? this._r.text : '') : super.response;
    }
    get responseURL() {
      return this._w ? this._u : super.responseURL;
    }
    getResponseHeader(name) {
      if (!this._w) return super.getResponseHeader(name);
      return this._r && String(name).toLowerCase() === 'content-type' ? `${this._r.mime}; charset=utf-8` : null;
    }
    getAllResponseHeaders() {
      if (!this._w) return super.getAllResponseHeaders();
      return this._r ? `content-type: ${this._r.mime}; charset=utf-8\r\n` : '';
    }
  }
  window.XMLHttpRequest = Xhr;

  const realFetch = window.fetch.bind(window);
  window.fetch = function (input, init) {
    const url = typeof input === 'string' ? input : input && input.url;
    const where = route(url);
    if (where && where.kind !== 'file') {
      const a = answer(where, (init && init.method) || 'GET', url, init && init.body ? String(init.body) : null, false);
      return a.status ? Promise.resolve(new Response(a.text, { status: a.status, headers: { 'Content-Type': a.mime } })) : Promise.reject(new TypeError('blocked'));
    }
    return realFetch(input, init);
  };
  // the theme opens nothing in another window or tab
  window.open = function (url) {
    T.report('blocked', `window.open ${url}`);
    return null;
  };

  window.addEventListener('error', (e) => T.report('error', `${e.message} (${e.filename}:${e.lineno})`));

  // ------------------------------------------------------------ the shell's part

  function exec(src, name) {
    try {
      (0, eval)(src);
    } catch (e) {
      T.report('error', `${name}: ${e}`);
    }
  }

  // JSIncludeDefaultLibs: the shared scripts, then the theme's (theme.ini's `scripts`)
  function includeDefaultLibs() {
    const base = 'ac/launcher/themes/.base/';
    for (const name of ['init.js', 'jquery-1.9.1.js', 'jquery.easing.js', 'jquery.flot.js', 'jquery.selectbox.js', 'jquery.stickyfloat.js', 'jquery.ui.js', 'ac.utils.js', '__ACClasses.js', '__AC.js', '__ACUI.js']) {
      exec(T.text(base + name) || '', `ac://js/${name}`);
    }
    for (const script of T.theme.scripts) {
      const file = `${T.theme.folder}/js/${script}.js`;
      const text = T.text(file);
      exec(text === null ? `console.log('${script} not found at ${file}' );` : text, `ac://theme/${T.theme.id}/js/${script}.js`);
    }
  }

  // GetJSEvent
  function trigger(name, args) {
    if (typeof window.$ !== 'undefined') window.$(document).trigger(name, args || []);
  }

  // ------------------------------------------------------------ Drive only

  const DRIVE_ONLY = `
#menu ul li:not([data-panel=drive]):not(.dummy) { display: none !important; }
#menu a.home { pointer-events: none !important; }
`;
  // the modes of Drive a browser cannot run yet: the theme's own notice says so
  const NOT_HERE = { timeattack: 'Time Attack', drift: 'Drift', dragrace: 'Drag Race' };

  function notice(text) {
    // (reported too, for the checks)
    T.report('notice', text);
    if (window.UI && typeof window.UI.displayDialog === 'function') window.UI.displayDialog('.notice', text, null, null, 'OK');
  }

  document.addEventListener(
    'click',
    (e) => {
      const target = e.target && e.target.closest ? e.target.closest('[data-mode], [data-panel], a.home') : null;
      if (!target) return;
      // (the left column's tiles are the menu's `li`s; buttons inside Drive have a data-panel too)
      const tile = target.matches('#menu li[data-panel]');
      if ((tile && target.getAttribute('data-panel') !== 'drive') || target.matches('a.home')) {
        e.stopImmediatePropagation();
        e.preventDefault();
        return;
      }
      const mode = target.getAttribute('data-mode');
      if (mode && NOT_HERE[mode] && target.closest('.drive')) {
        e.stopImmediatePropagation();
        e.preventDefault();
        notice(`${NOT_HERE[mode]} is not available in the browser version yet.`);
      }
    },
    true,
  );

  // As the desktop launcher (crates/rustyac-launcher shell.rs WIZARD_JS): the theme's own
  // first-run wizard asks the language while launcher.ini [WINDOW] LANGUAGE is empty
  document.onloadModules = function () {
    window.__AC.loadModule('FirstRunWizard', 'body > .modules', 'en', null, true);
  };

  // The theme's fonts are Windows' "Segoe UI" and "Segoe UI Light". A system without them gets
  // Selawik, Microsoft's open look-alike, from this site under those names (local() still
  // first); a system with them is left as it is. (?font=selawik, a test hook: Selawik anyway.)
  const forceSelawik = new URLSearchParams(window.parent.location.search).get('font') === 'selawik';
  const hasFont = (name) => {
    const c = document.createElement('canvas').getContext('2d');
    const text = 'mmmmmmmmmmlli1WQ@#';
    return ['monospace', 'serif'].some((base) => {
      c.font = `72px ${base}`;
      const width = c.measureText(text).width;
      c.font = `72px "${name}", ${base}`;
      return c.measureText(text).width !== width;
    });
  };
  const fonts = new URL('../fonts/', acfs).href;
  const face = (family, local, file, weight) =>
    `@font-face { font-family: "${family}"; src: ${forceSelawik ? '' : `local("${local}"), `}url("${fonts}selawik-${file}.ttf") format("truetype"); font-weight: ${weight}; }`;
  const selawik = () =>
    [
      face('Segoe UI', 'Segoe UI', 'regular', 400),
      face('Segoe UI', 'Segoe UI Light', 'light', 300),
      face('Segoe UI', 'Segoe UI Bold', 'bold', 700),
      face('Segoe UI Light', 'Segoe UI Light', 'light', 400),
    ].join('\n');

  document.addEventListener('DOMContentLoaded', () => {
    const style = document.createElement('style');
    style.id = 'rustyac-drive-only';
    const fallback = forceSelawik || !hasFont('Segoe UI');
    T.fonts = fallback ? 'selawik' : 'system';
    style.textContent = DRIVE_ONLY + (fallback ? '\n' + selawik() : '');
    document.head.appendChild(style);
  });
  window.addEventListener('load', () => {
    if (T.theme.loadDefault) includeDefaultLibs();
    // The other tiles and the logo's way back to the intro page stop every click on themselves,
    // jQuery's too (.click() from a script never reaches the capture listener above): a
    // handler on the element runs before the theme's, which all listen on the document
    if (typeof window.$ !== 'undefined') {
      const stop = (e) => {
        e.stopImmediatePropagation();
        e.preventDefault();
        return false;
      };
      window.$('#menu li[data-panel]').not('[data-panel=drive]').addClass('disabled').on('click', stop);
      window.$('.home').off('click').on('click', stop);
    }
    T.loaded();
  });

  window.__rustyac = { trigger, exec, notice, route, pause };
})();
