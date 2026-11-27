// SPDX-License-Identifier: GPL-3.0-or-later
// check.mjs --script for browser.sh's partial-import cases (Task W5): the page's car and track
// lists and what does not load, and with the launcher which of the wanted ids the theme's own
// lists (ac://listcars, ac://listtracks) hold. `wanted`: set by the case (window.rustyacWanted).
const t0 = Date.now();
while (!window.rustyac.imports && !window.rustyac.error && Date.now() - t0 < 120000) await new Promise((r) => setTimeout(r, 250));
const values = (id) => [...document.querySelectorAll(`#${id} option`)].map((o) => o.value);
const out = { cars: values('car'), tracks: values('track'), imports: window.rustyac.imports || null, line: document.getElementById('content-line').textContent, refused: (window.rustyac.content || {}).refused || [], error: window.rustyac.error || null };
const frame = document.querySelector('#launcher iframe');
if (frame) {
  const ask = (url) => {
    const x = new frame.contentWindow.XMLHttpRequest();
    x.open('GET', url, false);
    x.send();
    return x.responseText;
  };
  const listed = ask('ac://listcars') + ask('ac://listtracks');
  out.theme = ['ks_mazda_miata', 'abarth500', 'bmw_1m', 'ks_toyota_gt86', 'magione', 'drift', 'bmw_z4_gt3'].filter((id) => listed.includes(`"${id}"`) || listed.includes(`/${id}`));
}
return out;
