// node node_proof.mjs <site> <ac root> <car> <track> <tape> <camera> <log out> [wav out]
// sound_proof (the page's wasm + sound.js + FMOD 2.03 HTML5) in node.
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';
// FMOD's JS asks node for these
globalThis.require = createRequire(import.meta.url);
globalThis.__dirname = process.cwd();

const [site, ac, car, trackArg, tape, camera, logOut, wavOut] = process.argv.slice(2);
const rac = await import(pathToFileURL(path.join(site, 'pkg/rustyac_web.js')).href);
rac.initSync({ module: fs.readFileSync(path.join(site, 'pkg/rustyac_web_bg.wasm')) });
rac.start();
const { loadSound } = await import(pathToFileURL(path.join(site, 'sound.js')).href);
const fmod = 'ignored/fmod/fmodstudioapi20315html5/fmodstudioapi20315html5/api/studio/lib/wasm/';
const sound = await loadSound(fs.readFileSync(fmod + 'fmodstudio.js', 'utf8'), fs.readFileSync(fmod + 'fmodstudio.wasm'), rac, { window: null });
{ const c = globalThis.rustyacSound.call; globalThis.rustyacSound.call = (n, t, a) => { if (process.env.TRACE) console.log(n, t, Array.from(a).map((w) => w.toString(16)).join(' ')); const r = c(n, t, a); if (process.env.TRACE) console.log(' ->', Array.from(r.slice(0, 5)).map((w) => w.toString(16)).join(' ')); return r; }; }
globalThis.TRACE_DSP = process.env.TRACE_DSP && ((t) => fs.writeSync(2, t + String.fromCharCode(10)));
globalThis.PRE = !!process.env.PRE;
globalThis.ONCE = !!process.env.ONCE;
globalThis.STUB = !!process.env.STUB;
globalThis.NODESC = !!process.env.NODESC;
globalThis.NOADV = process.env.NOADV;
globalThis.NOPLUG = process.env.NOPLUG;
if (process.env.BLOCKS) { const c = globalThis.rustyacSound.call; const hist = new Map(); let last = 0, calls = 0; globalThis.rustyacSound.call = (n, t, a) => { const r = c(n, t, a); if (n === 'studio_update' || n === 'studio_flush_commands') { const now = (sound.state.capture || []).length; const k = n + ':' + (now - last) + 'x' + ((sound.state.capture || [])[now - 1]?.length || 0); hist.set(k, (hist.get(k) || 0) + 1); last = now; } return r; }; process.on('exit', () => console.log([...hist].join(' '))); }
sound.state.drivers = Number(process.env.DRIVERS ?? 5);

const put = (rel) => {
  const p = path.join(ac, rel);
  if (fs.existsSync(p) && fs.statSync(p).isFile()) rac.fs_put(rel, fs.readFileSync(p));
};
const putDir = (rel) => {
  const p = path.join(ac, rel);
  if (!fs.existsSync(p)) return;
  for (const n of fs.readdirSync(p)) put(rel + '/' + n);
};
putDir('system/cfg');
put('system/data/surfaces.ini');
put('content/sfx/GUIDs.txt');
put('content/sfx/common.bank');
putDir(`content/cars/${car}/sfx`);
put(`content/cars/${car}/data.acd`);
putDir(`content/cars/${car}/data`);
// the tape's track and layout (its header)
const head = fs.readFileSync(tape).subarray(0, 2048).toString('latin1');
const field = (k) => (head.match(new RegExp('^' + k + '=(.*)$', 'm')) || [])[1]?.trim() || '';
const track = trackArg === '-' ? field('track') : trackArg;
const layout = field('track_layout');
putDir(`content/tracks/${track}/data`);
if (layout) putDir(`content/tracks/${track}/${layout}/data`);
// the track's models: Scene::load finds the reverb nodes and occluders in them
for (const n of fs.readdirSync(path.join(ac, `content/tracks/${track}`))) if (/\.(kn5|ini)$/i.test(n)) put(`content/tracks/${track}/${n}`);
rac.fs_put('proof.audiotape', fs.readFileSync(tape));

const t0 = Date.now();
const log = rac.sound_proof('proof.audiotape', camera, !!wavOut);
console.log(`sound_proof: ${((Date.now() - t0) / 1000).toFixed(1)} s, ${log.split('\n').length} lines`);
fs.writeFileSync(logOut, log);
if (wavOut) {
  const bytes = sound.wav();
  if (bytes) fs.writeFileSync(wavOut, bytes);
}
