// SPDX-License-Identifier: GPL-3.0-or-later
// check.mjs --script for browser.sh's on-demand cases (Task 39): with the launcher, the theme's
// server list (ac://getserverlist); with ?join, what the page did with it once it gave up or
// failed to connect (no gateway behind ondemand_site.mjs).
const frame = document.querySelector('#launcher iframe');
if (frame) {
  const x = new frame.contentWindow.XMLHttpRequest();
  x.open('GET', 'ac://getserverlist', false);
  x.send();
  return JSON.parse(x.responseText).map((s) => ({ name: s.name, track: s.track, cars: s.cars, pass: s.pass }));
}
const t0 = Date.now();
// (a failed join ends in window.rustyac.error, an unlisted one in the log)
const done = () => window.rustyac.error || window.rustyac.log.some((l) => /is not running now/.test(l));
// the listed track loads first (Laguna is 843 MB)
while (!done() && Date.now() - t0 < 500000) await new Promise((r) => setTimeout(r, 500));
return { server: document.getElementById('mp-server').value, log: window.rustyac.log.filter((l) => /connecting|not running/i.test(l)).slice(-6), error: (window.rustyac.error || '').replace(/: the gateway.*/, '') };
