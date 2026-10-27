// node replay_probe.mjs: the traced setup calls (setup_calls.txt) through sound.js with a stand-in for the page's wasm
import fs from 'node:fs';
import { createRequire } from 'node:module';
globalThis.require = createRequire(import.meta.url);
globalThis.__dirname = process.cwd();
const { loadSound } = await import('./site/sound.js');
const AC = 'C:/Program Files (x86)/Steam/steamapps/common/assettocorsa/';
const plugins = fs.readFileSync('re/scratch/task41/plugins.jsonl', 'utf8').trim().split('\n');
let n = 0;
const rac = { sound_dsp_describe: () => plugins[n++ % 2], sound_read_file: (p) => fs.readFileSync(AC + p.replace(/^ac\//, '')) };
const F = 'ignored/fmod/fmodstudioapi20315html5/fmodstudioapi20315html5/api/studio/lib/wasm/';
const sound = await loadSound(fs.readFileSync(F + 'fmodstudio.js', 'utf8'), fs.readFileSync(F + 'fmodstudio.wasm'), rac, { window: null });
sound.state.drivers = 5;
globalThis.STUB = !!process.env.STUB;
const skip = process.env.SKIP ? new RegExp(process.env.SKIP) : null;
for (const line of fs.readFileSync(process.env.CALLS || 're/scratch/task41/setup_calls.txt', 'utf8').trim().split('\n')) {
  const parts = line.trim().split(/ +/);
  const name = parts[0];
  if (skip && skip.test(name)) continue;
  if (process.env.DIRECT && name === 'studio_load_bank_file') {
    const F2 = sound.FMOD; const sys = globalThis.OBJECTS.get(0x100); const o = {};
    F2.FS_createDataFile('/', process.env.BNAME || 'b.bank', fs.readFileSync(AC + 'content/sfx/common.bank'), true, false);
    console.log('direct load', sys.loadBankFile('/' + (process.env.BNAME || 'b.bank'), 0, o));
    continue;
  }
  let text = '', words = parts.slice(1);
  if (name === 'studio_load_bank_file') { text = words[0]; words = words.slice(1); }
  const out = new Uint32Array(16);
  const r = globalThis.rustyacSound.call(name, text, Uint32Array.from(words.map((w) => parseInt(w, 16))), out);
  fs.writeSync(1, `${name} -> ${r}\n`);
}
