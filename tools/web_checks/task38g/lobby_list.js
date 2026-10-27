// check.mjs --script (with --launcher): the theme's server list (ac://getserverlist, the request
// its online page makes each time it opens) before and after the panel starts a server. The
// "ready" request tells e2e.sh (serve.py's log) to start it now; the page is not reloaded.
const frame = document.querySelector('#launcher iframe') || document.querySelector('iframe');
const list = () => {
  const x = new frame.contentWindow.XMLHttpRequest();
  x.open('GET', 'ac://getserverlist', false);
  x.send();
  return JSON.parse(x.responseText).map((s) => s.name);
};
const before = list();
await fetch('/ready-38g.txt').catch(() => {});
const t0 = Date.now();
let after = before;
while (Date.now() - t0 < 120000) {
  await new Promise((r) => setTimeout(r, 2000));
  after = list();
  if (after.length > before.length) break;
}
return { before, after, seconds: Math.round((Date.now() - t0) / 1000) };
