// node race_node.mjs <site> <ac root> <race.ini> <assists.ini> <seed> <tape> <camera> <focus> <log out> [wav out]
// race_sound_proof (the page's wasm + sound.js + FMOD 2.03 HTML5) in node: every car of race.ini.
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';
globalThis.require = createRequire(import.meta.url);
globalThis.__dirname = process.cwd();

const [site, ac, raceIni, assistsIni, seed, tape, camera, focus, logOut, wavOut] = process.argv.slice(2);
const rac = await import(pathToFileURL(path.join(site, 'pkg/rustyac_web.js')).href);
rac.initSync({ module: fs.readFileSync(path.join(site, 'pkg/rustyac_web_bg.wasm')) });
rac.start();
const { loadSound } = await import(pathToFileURL(path.join(site, 'sound.js')).href);
const fmod = 'ignored/fmod/fmodstudioapi20315html5/fmodstudioapi20315html5/api/studio/lib/wasm/';
const sound = await loadSound(fs.readFileSync(fmod + 'fmodstudio.js', 'utf8'), fs.readFileSync(fmod + 'fmodstudio.wasm'), rac, { window: null });
sound.state.drivers = 5;

// every file of a folder; models and textures empty (folder listings stay as they are), but the track's own models
let bytes = 0;
const putTree = (rel, keep = () => false) => {
  const p = path.join(ac, rel);
  if (!fs.existsSync(p)) return;
  for (const e of fs.readdirSync(p, { withFileTypes: true })) {
    const r = rel + '/' + e.name;
    if (e.isDirectory()) putTree(r, keep);
    else {
      const big = /\.(kn5|dds|png|jpg|jpeg|knh|bin|vao-patch)$/i.test(e.name);
      const data = big && !keep(r) ? new Uint8Array(0) : fs.readFileSync(path.join(ac, r));
      bytes += data.length;
      rac.fs_put(r, data);
    }
  }
};
const race = fs.readFileSync(raceIni, 'utf8');
const models = new Set([...race.matchAll(/^MODEL=(.+)$/gm)].map((m) => m[1].trim()).filter((m) => m && m !== '-'));
const track = (race.match(/^TRACK=(.+)$/m) || [])[1].trim();
putTree('system');
for (const n of ['GUIDs.txt', 'common.bank']) rac.fs_put('content/sfx/' + n, fs.readFileSync(path.join(ac, 'content/sfx', n)));
for (const car of models) putTree(`content/cars/${car}`, (r) => /collider.kn5$/i.test(r));
putTree(`content/tracks/${track}`, (r) => r.split('/').length === 4);
console.log(`mounted ${(bytes / 1e6).toFixed(0)} MB: cars ${[...models].join(', ')}; track ${track}`);

const t0 = Date.now();
const assists = assistsIni === '-' ? undefined : fs.readFileSync(assistsIni, 'utf8');
const log = rac.race_sound_proof(race, assists, Number(seed), fs.readFileSync(tape), camera, Number(focus));
console.log(`race_sound_proof: ${((Date.now() - t0) / 1000).toFixed(1)} s, ${log.split('\n').length} lines`);
fs.writeFileSync(logOut, log);
if (wavOut) {
  const b = sound.wav();
  if (b) fs.writeFileSync(wavOut, b);
}
