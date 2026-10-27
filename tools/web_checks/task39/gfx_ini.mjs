// Task 39: the video.ini the page writes for a ?graphics= spec (the page's own helpers, cut out
// of web/app.js), for the native full_frame runs.  usage: node gfx_ini.mjs <spec> <out file>
import { readFileSync, writeFileSync } from 'node:fs';
const app = readFileSync('web/app.js', 'utf8');
const code = app.slice(app.indexOf('const OFF_TO_ULTRA'), app.indexOf('// The page\'s video.ini: the launcher'));
const [spec, out] = process.argv.slice(2);
const make = new Function('spec', `${code}
const [name, ...keys] = spec.split(',');
const preset = GFX_PRESETS[name] ? Object.fromEntries(GFX_PRESET_KEYS.map((key) => [key, GFX_PRESETS[name][key]])) : {};
const { SCALE, ...file } = { ...GFX_HIGH, ...preset, ...Object.fromEntries(keys.map((pair) => pair.split('='))) };
return iniWith('', file);`);
writeFileSync(out, make(spec));
