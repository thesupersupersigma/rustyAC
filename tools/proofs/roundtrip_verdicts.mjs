// SPDX-License-Identifier: GPL-3.0-or-later
// Task W1: the verdicts of a roundtrip_flow.py run (web/check.mjs --flow's report on stdin),
// one `name|IDENTICAL or DIFFERENT|what` line each, for tools/proofs/browser.sh.
//   - every step done (no hang: check.mjs bounds each step)
//   - every drive had its sound, and no FMOD bank file is left once back in the launcher
//   - the GPU process back in the launcher stays under GPU_MB (before Task W1 it grew by about
//     200 MB a drive: 2.9 to 3.3 GB after the third)
//   - every reload came back to the launcher
const GPU_MB = 1500;
let text = '';
process.stdin.on('data', (d) => (text += d)).on('end', () => {
  const say = (name, ok, what) => console.log(`${name}|${ok ? 'IDENTICAL' : 'DIFFERENT'}|${what}`);
  let r;
  try {
    r = JSON.parse(text);
  } catch {
    say('Esc round trips', false, `no report: ${text.slice(0, 200)}`);
    return;
  }
  const failed = r.flow.find((s) => s.failed);
  const trips = r.flow.filter((s) => typeof s.value === 'string' && s.value.includes('"trip":true')).map((s) => JSON.parse(s.value));
  const sounds = r.flow.filter((s) => typeof s.value === 'string' && s.value.startsWith('sound '));
  const gpu = r.flow.filter((s) => s.processes).map((s) => s.processes['gpu-process']);
  say('Esc round trips back to the launcher', !failed && trips.length > 0 && trips.every((t) => t.state === 'launcher'),
    failed ? `${failed.failed} (at ${failed.at} s)` : `${trips.length} trips, js MB ${trips.map((t) => t.js_mb).join(' ')}, wasm MB ${trips.map((t) => t.wasm_mb).join(' ')}`);
  say('every drive with sound, no bank file left', sounds.length === trips.length && sounds.every((s) => s.value === 'sound true') && trips.every((t) => t.bank_files === 0),
    `sound ${sounds.map((s) => s.value.slice(6)).join(' ')}, bank files left ${trips.map((t) => t.bank_files).join(' ')}`);
  say(`GPU process under ${GPU_MB} MB after each trip`, gpu.length === trips.length && gpu.every((mb) => mb < GPU_MB), `MB ${gpu.join(' ')}`);
  const reloads = r.flow.filter((s) => s.reload).length;
  say('reloads from the launcher and from a drive', !failed && reloads > 0, `${reloads} reloads, back in ${r.flow.filter((s) => s.until === 'launcher').slice(-reloads).map((s) => s.seconds.toFixed(1)).join(' ')} s`);
});
