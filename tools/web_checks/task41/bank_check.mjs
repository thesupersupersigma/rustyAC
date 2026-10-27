// node bank_check.mjs <ac root> <out.tsv> [pack manifest.json]
// Every sound bank of the install (or only the pack's) in FMOD Studio 2.03 HTML5 (the wasm the
// page uses): loads it twice, first without ALLOW_MISSING_PLUGINS (does it name a plug-in
// FMOD does not have: AC's own two), then as the page does; then every event's sample data,
// an instance, a start and a few updates. One row per bank.
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
globalThis.require = createRequire(import.meta.url);
globalThis.__dirname = process.cwd();

const [ac, out, manifest] = process.argv.slice(2);
const F = 'ignored/fmod/fmodstudioapi20315html5/fmodstudioapi20315html5/api/studio/lib/wasm/';
const FMODModule = new Function(fs.readFileSync(F + 'fmodstudio.js', 'utf8') + String.fromCharCode(10) + 'return FMODModule;')();
const FMOD = { wasmBinary: fs.readFileSync(F + 'fmodstudio.wasm') };
await new Promise((resolve, reject) => {
  FMOD.onRuntimeInitialized = resolve;
  FMODModule(FMOD).catch(reject);
});
const err = (r) => (r === FMOD.OK ? 'ok' : `${r} ${FMOD.ErrorString(r)}`);

let banks;
if (manifest) banks = JSON.parse(fs.readFileSync(manifest, 'utf8')).files.map((f) => f.path).filter((p) => p.toLowerCase().endsWith('.bank'));
else {
  banks = fs.readdirSync(path.join(ac, 'content/sfx')).filter((n) => n.endsWith('.bank')).map((n) => 'content/sfx/' + n);
  for (const car of fs.readdirSync(path.join(ac, 'content/cars')).sort()) {
    const dir = path.join(ac, 'content/cars', car, 'sfx');
    if (fs.existsSync(dir)) for (const n of fs.readdirSync(dir)) if (n.toLowerCase().endsWith('.bank')) banks.push(`content/cars/${car}/sfx/${n}`);
  }
}

let file = 0;
function system(flags) {
  const o = {};
  FMOD.Studio_System_Create(o);
  const sys = o.val;
  sys.getCoreSystem(o);
  o.val.setOutput(FMOD.OUTPUTTYPE_NOSOUND_NRT);
  const r = sys.initialize(256, FMOD.STUDIO_INIT_SYNCHRONOUS_UPDATE | flags, 4, null);
  if (r !== FMOD.OK) throw new Error('initialize: ' + err(r));
  return sys;
}
function load(sys, rel) {
  const name = `b${file++}.bank`;
  FMOD.FS_createDataFile('/', name, fs.readFileSync(path.join(ac, rel)), true, false);
  const o = {};
  const r = sys.loadBankFile('/' + name, FMOD.STUDIO_LOAD_BANK_NORMAL, o);
  return { r, bank: o.val, name };
}

const rows = ['bank\tstrict load\tload\tevents\tfailed events\tfirst failure'];
let ok = 0;
for (const rel of banks) {
  const strict = system(0);
  const s = load(strict, rel);
  strict.release();
  FMOD.FS_unlink('/' + s.name);
  const sys = system(FMOD.STUDIO_INIT_ALLOW_MISSING_PLUGINS);
  // (a car bank's events can use common.bank's buses)
  if (rel !== 'content/sfx/common.bank' && fs.existsSync(path.join(ac, 'content/sfx/common.bank'))) load(sys, 'content/sfx/common.bank');
  const { r, bank, name } = load(sys, rel);
  let events = 0, failed = 0, first = '';
  if (r === FMOD.OK) {
    const n = {};
    bank.getEventCount(n);
    events = n.val;
    const list = {}, got = {};
    bank.getEventList(list, events, got);
    for (const e of list.val || []) {
      const steps = [];
      let rr = e.loadSampleData();
      steps.push(['loadSampleData', rr]);
      if (rr === FMOD.OK) steps.push(['flushSampleLoading', (rr = sys.flushSampleLoading())]);
      const i = {};
      if (rr === FMOD.OK) steps.push(['createInstance', (rr = e.createInstance(i))]);
      if (rr === FMOD.OK) steps.push(['start', (rr = i.val.start())]);
      for (let k = 0; rr === FMOD.OK && k < 5; k++) steps.push(['update', (rr = sys.update())]);
      if (i.val) i.val.release();
      const bad = steps.find(([, x]) => x !== FMOD.OK);
      if (bad) {
        failed++;
        if (!first) {
          const p = {};
          e.getPath(p, 256, {});
          first = `${p.val || '?'}: ${bad[0]} ${err(bad[1])}`;
        }
      }
    }
  }
  sys.release();
  FMOD.FS_unlink('/' + name);
  if (s.r === FMOD.OK && r === FMOD.OK && !failed) ok++;
  rows.push(`${rel}\t${err(s.r)}\t${err(r)}\t${events}\t${failed}\t${first}`);
  process.stdout.write(`\r${rows.length - 1} of ${banks.length}`);
}
fs.writeFileSync(out, rows.join('\n') + '\n');
console.log(`\n${banks.length} banks, ${ok} load and play every event with no plug-in missing; table: ${out}`);
