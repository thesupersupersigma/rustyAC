// node direct_probe.mjs: probe2's steps on the FMOD that sound.js loaded
import fs from 'node:fs';
import { createRequire } from 'node:module';
globalThis.require = createRequire(import.meta.url);
globalThis.__dirname = process.cwd();
const { loadSound } = await import('./site/sound.js');
const AC = 'C:/Program Files (x86)/Steam/steamapps/common/assettocorsa/';
const F = 'ignored/fmod/fmodstudioapi20315html5/fmodstudioapi20315html5/api/studio/lib/wasm/';
const sound = await loadSound(fs.readFileSync(F + 'fmodstudio.js', 'utf8'), fs.readFileSync(F + 'fmodstudio.wasm'), {}, { window: null });
const FMOD = sound.FMOD;
const o = {};
FMOD.Studio_System_Create(o); const sys = o.val;
sys.getCoreSystem(o); const core = o.val; core.setOutput(FMOD.OUTPUTTYPE_NOSOUND_NRT);
console.log('init', sys.initialize(2048, 4, 4, null));
const keep = [];
for (const line of fs.readFileSync('re/scratch/task41/plugins.jsonl', 'utf8').trim().split('\n')) {
  const d = JSON.parse(line);
  if (process.env.ONLY) d.params = d.params.filter((p) => p.name === process.env.ONLY);
  if (process.env.PLUG && !d.name.includes(process.env.PLUG)) continue;
  const desc = FMOD.DSP_DESCRIPTION(); keep.push(desc);
  desc.pluginsdkversion = 110; desc.name = d.name; desc.version = d.version; desc.numinputbuffers = d.inputs; desc.numoutputbuffers = d.outputs;
  desc.numparameters = d.params.length;
  const M = process.env.M || '';
  if (M.includes('n')) { desc.numparameters = 0; } else desc.paramdesc = d.params.map((p) => {
    const q = FMOD.DSP_PARAMETER_DESC();
    q.type = p.type; q.name = p.name; q.label = p.label; q.description = M.includes('d') ? '' : p.description;
    if (p.type === 0) { q.floatdesc.min = p.min; q.floatdesc.max = p.max; q.floatdesc.defaultval = p.default; q.floatdesc.mapping = { type: 1 }; }
    else if (p.type === 2) q.booldesc.defaultval = false;
    else if (p.type === 3) q.datadesc.datatype = p.word;
    return q;
  });
  desc.create = () => { (globalThis.CREATES ||= {})[d.name] = (globalThis.CREATES[d.name] || 0) + 1; return 0; }; desc.release = () => 0; desc.reset = () => 0; desc.shouldiprocess = () => 0; desc.read = () => 0;
  desc.setparameterfloat = () => 0; desc.getparameterfloat = (a, b, v) => ((v.val = 0), 0);
  if (d.bool) { desc.setparameterbool = () => 0; desc.getparameterbool = (a, b, v) => ((v.val = false), 0); }
  if (d.data) { desc.setparameterdata = () => 0; desc.getparameterdata = () => 31; }
  if (process.env.LOGCB) for (const k of Object.keys(desc)) if (typeof desc[k] === 'function') { const f = desc[k]; desc[k] = (...x) => { const r = f(...x); fs.writeSync(1, `cb ${d.name} ${k} ${x.slice(1).map((v) => typeof v === 'object' ? JSON.stringify(v) : v).join(' ')} -> ${r}
`); return r; }; }
  if (M.includes('p')) continue;
  if (process.env.CORE) { const h = {}; console.log('core register', d.name, core.registerDSP(desc, h), h.val); } else console.log('register', d.name, sys.registerPlugin(desc));
}
if (process.env.CHURN) for (let i = 0; i < 20; i++) { const dd = FMOD.DSP_DESCRIPTION(); dd.name = 'junk' + i; dd.version = 1; dd.numparameters = 3; dd.paramdesc = [0, 1, 2].map(() => { const q = FMOD.DSP_PARAMETER_DESC(); q.type = 0; q.name = 'zzzzzzzzzzzzzz'; q.label = 'yyyyyyyyyyyy'; q.description = 'x'.repeat(80); q.floatdesc.min = 0; q.floatdesc.max = 1; q.floatdesc.defaultval = 0; q.floatdesc.mapping = { type: 0 }; return q; }); const h = {}; core.createDSP(dd, h); h.val?.release(); }
if (process.env.CORE) console.log('core registerDSP exists', typeof core.registerDSP);
const BN = process.env.BNAME || 'b.bank';
FMOD.FS_createDataFile('/', BN, fs.readFileSync(AC + 'content/sfx/common.bank'), true, false);
console.log('load', sys.loadBankFile('/' + BN, 0, o));

let bank = o.val;
if (process.env.CAR) { FMOD.FS_createDataFile('/', 'car.bank', fs.readFileSync(AC + 'content/cars/' + process.env.CAR + '/sfx/' + process.env.CAR + '.bank'), true, false); { const r = sys.loadBankFile('/car.bank', 0, o); console.log('car load', r, FMOD.ErrorString(r)); } bank = o.val; }
const n = {}; bank.getEventCount(n); const list = {}; bank.getEventList(list, n.val, {});
let started = 0;
for (const e of list.val || []) { const i = {}; if (e.createInstance(i) === 0) { i.val.start(); started++; } }
for (let k = 0; k < 10; k++) sys.update();
console.log('events', n.val, 'started', started, 'creates', JSON.stringify(globalThis.CREATES || {}));
