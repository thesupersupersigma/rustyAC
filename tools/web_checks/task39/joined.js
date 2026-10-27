// check.mjs --script for e2e.sh's joins (driving, or --menu once the join is over): the car
// the page asked the server for, what it drives (its display), the multiplayer line, the refusal
await new Promise((r) => setTimeout(r, 3000)); // (with --menu: the join may not have begun)
const t0 = Date.now();
// (the listed car and track download first)
const over = () => window.rustyac.state === 'driving' || window.rustyac.error || window.rustyac.log.some((l) => /is not running now/.test(l));
while (!over() && Date.now() - t0 < 380000) await new Promise((r) => setTimeout(r, 500));
const hud = window.rustyac.hud;
return {
  state: window.rustyac.state,
  asked: window.rustyac.log.filter((l) => /^asking /.test(l)).slice(-2),
  hud: hud ? JSON.stringify({ car: hud.car, track: hud.track, online: hud.online && { session: hud.online.session, cars: hud.online.cars } }).slice(0, 300) : null,
  mp: document.getElementById('mp-status').textContent,
  server: document.getElementById('mp-server').value,
  refusal: document.getElementById('refusal').hidden ? '' : document.getElementById('refusal').textContent.slice(0, 200),
  log: window.rustyac.log.slice(-3),
};
