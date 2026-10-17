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

  document.addEventListener('DOMContentLoaded', () => {
    const style = document.createElement('style');
    style.id = 'rustyac-drive-only';
    style.textContent = DRIVE_ONLY;
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

  window.__rustyac = { trigger, exec, notice, route };
})();
