// check.mjs --script for multiplayer runs: the page's multiplayer line, its log's last lines,
// the online part of the display and this visit's diagnostics (when the page keeps them)
const visits = JSON.parse(localStorage.getItem('rustyac.diagnostics') || '[]');
const events = (visits.pop() || { events: [] }).events.filter((e) => !/^\S+ download: .*downloaded$/.test(e));
const hud = window.rustyac.hud && window.rustyac.hud.online;
return {
  mp: document.getElementById('mp-status').textContent,
  refusal: document.getElementById('refusal').hidden ? '' : document.getElementById('refusal').textContent,
  log: window.rustyac.log.slice(-3),
  online: hud ? JSON.stringify(hud).slice(0, 400) : null,
  diag: events,
};
