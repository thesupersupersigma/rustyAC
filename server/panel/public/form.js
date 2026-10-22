// SPDX-License-Identifier: MIT OR Apache-2.0
// The panel page's helpers that need no page: its tests import them (test/panel.test.ts).

// A preset id from its name: lower case, a dash for anything else, at most 32 characters; with
// -2, -3 ... when `taken` (the other presets' ids) has it.
export function idFromName(name, taken = []) {
  const base = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 28).replace(/-+$/, '') || 'server';
  let id = base;
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`;
  return id;
}

// fetch for the panel's API. Behind Cloudflare Access an expired login answers with a redirect
// to the login page (or with that page): the page then reloads once, which logs in again;
// when that did not help, a plain message. Never the browser's own "Failed to fetch".
/** @param {string} path @param {unknown} [body] @param {any} [env] the browser's window; the tests' stand-in */
export async function api(path, body, env = globalThis) {
  let res;
  try {
    res = await env.fetch(path, {
      method: body === undefined ? 'GET' : 'POST', credentials: 'same-origin', redirect: 'manual', cache: 'no-store',
      ...(body === undefined ? {} : { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }),
    });
  } catch {
    throw new Error('the panel did not answer: the network is down or the panel is restarting');
  }
  const json = (res.headers.get('content-type') || '').startsWith('application/json');
  if (res.type === 'opaqueredirect' || res.redirected || !json) {
    if (!env.sessionStorage.getItem('panel-reloaded')) {
      env.sessionStorage.setItem('panel-reloaded', '1');
      env.location.reload();
      throw new Error('your Cloudflare Access login expired: reloading');
    }
    throw new Error('your Cloudflare Access login expired and reloading did not log in again: reload the page');
  }
  env.sessionStorage.removeItem('panel-reloaded');
  const data = await res.json();
  if (!res.ok) throw new Error(data.error || `the panel answered ${res.status}`);
  return data;
}
