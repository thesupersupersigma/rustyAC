// SPDX-License-Identifier: MIT OR Apache-2.0
// The panel page (public/index.html): the running servers, the presets and the preset form.
import { api, idFromName } from './form.js';

const $ = (id) => document.getElementById(id);
const el = (tag, { dataset, ...props } = {}, ...kids) => {
  const e = Object.assign(document.createElement(tag), props);
  Object.assign(e.dataset, dataset || {});
  e.append(...kids);
  return e;
};
let state = null;
let editing = null; // the preset in the form
let chosen = new Map(); // model -> { count, skins: Set, random } of the form
let idTouched = false; // the id was typed under "advanced": the name no longer makes it
let refreshFailed = false; // the error shown is the refresh's own: the next good one clears it

function error(text) {
  $('error').hidden = !text;
  $('error').textContent = text || '';
}
const act = (path, body = {}) => api(path, body).then(refresh).catch((e) => error(e.message));
const trackName = (t) => state.pack.tracks.find((x) => x.track === t.track && x.layout === t.layout)?.name || (t.layout ? `${t.track} / ${t.layout}` : t.track);
const carName = (model) => state.pack.cars.find((c) => c.model === model)?.name || model;
const minutes = (s) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;

function showRunning() {
  const box = $('running');
  box.replaceChildren(...state.running.map((r) => {
    const logs = el('pre', { hidden: true });
    return el('div', { className: 'card' },
      el('div', { className: 'row' },
        el('b', { textContent: r.name }), el('span', { className: r.status === 'running' ? 'ok' : 'bad', textContent: r.status }),
        el('span', { textContent: `${trackName(r)} · ${r.cars.map(carName).join(', ')}` }),
        el('span', { textContent: `players ${r.players ? r.players.length : '?'} / ${r.maxPlayers}${r.players && r.players.length ? ': ' + r.players.join(', ') : ''}` }),
        el('span', { className: 'mute', textContent: `up ${minutes(Math.round((Date.now() - r.started) / 60000))} h · stops in ${minutes(r.stopsInSeconds)} min if nobody comes` }),
        el('button', { className: 'stop', textContent: 'Stop', onclick: () => confirm(`Stop ${r.name}?`) && act(`/api/stop/${r.id}`) }),
        el('button', { textContent: 'Log', onclick: async () => {
          logs.hidden = !logs.hidden;
          if (!logs.hidden) {
            try {
              const l = await api(`/api/logs/${r.id}`);
              logs.textContent = `acServer:\n${l.acserver.join('\n')}\n\ngateway:\n${l.gateway.join('\n')}`;
            } catch (e) { error(e.message); }
          }
        } })),
      el('div', { className: 'mute' }, 'browser: ', el('a', { href: r.browser, textContent: r.browser, target: '_blank' })),
      el('div', { className: 'mute' }, 'desktop: ', el('code', { textContent: r.desktop })),
      el('div', { className: 'mute', textContent: `ports: game ${r.ports.game} (TCP+UDP), HTTP ${r.ports.http}, gateway ${r.ports.gateway} · admin password ${r.admin} · ${r.password ? 'a password to join' : 'no password to join'}` }),
      // a server runs as its preset was when it started: a saved change waits for the next start
      ...(!!state.presets.find((p) => p.id === r.id)?.password !== r.password ? [el('div', { className: 'mute', textContent: 'the preset now says otherwise about the password: that takes effect when the server starts again' })] : []),
      logs);
  }));
  if (!state.running.length) box.replaceChildren(el('div', { className: 'card mute', textContent: 'none' }));
}

function showPresets() {
  const runs = new Set(state.running.map((r) => r.id));
  $('presets').replaceChildren(...state.presets.map((p) => el('div', { className: 'card row' },
    el('b', { textContent: p.name }), el('code', { textContent: `net/x/${p.id}` }),
    el('span', { textContent: `${trackName(p)} · ${p.cars.map((c) => `${c.count}× ${carName(c.model)}`).join(', ')}${p.password ? ' · password' : ''}` }),
    el('button', { className: 'go', textContent: 'Start', disabled: runs.has(p.id) || state.running.length >= state.maxServers, onclick: () => act(`/api/start/${p.id}`) }),
    el('button', { textContent: 'Edit', onclick: () => edit(p) }),
    el('button', { textContent: 'Duplicate', onclick: () => edit({ ...structuredClone(p), id: '', name: `${p.name} copy`.slice(0, 120) }) }),
    el('button', { textContent: 'Delete', onclick: () => confirm(`Delete the preset ${p.name}?`) && act(`/api/delete/${p.id}`) }))));
  $('copies').replaceChildren(...state.fixed.map((f) => el('button', { textContent: `New from the fixed server ${f}`, onclick: () => act(`/api/copy/${f}`) })));
}

// the track list: one entry per layout, the layouts of a track under its name
function trackOptions() {
  const groups = new Map();
  for (const t of state.pack.tracks) groups.set(t.track, [...(groups.get(t.track) || []), t]);
  const option = (t) => el('option', { value: `${t.track}|${t.layout}`, textContent: t.pitboxes ? `${t.name} (${t.pitboxes} pit boxes)` : t.name });
  return [...groups.values()].sort((a, b) => a[0].name.localeCompare(b[0].name)).map((layouts) => {
    if (layouts.length === 1) return option(layouts[0]);
    const common = layouts.map((t) => t.name).reduce((a, b) => { let i = 0; while (i < a.length && a[i] === b[i]) i++; return a.slice(0, i); });
    return el('optgroup', { label: common.replace(/[\s\-–:(]+$/, '') || layouts[0].track }, ...layouts.map(option));
  });
}

function showCars() {
  const query = $('carSearch').value.trim().toLowerCase();
  const total = [...chosen.values()].reduce((n, c) => n + c.count, 0);
  const track = state.pack.tracks.find((t) => `${t.track}|${t.layout}` === $('editor').track.value);
  $('carTotal').textContent = `${total} cars${track?.pitboxes ? ` of ${track.pitboxes} pit boxes` : ''}`;
  $('carTotal').className = track?.pitboxes && total > track.pitboxes ? 'bad' : 'mute';
  const shown = state.pack.cars.filter((c) => chosen.has(c.model) || !query || c.name.toLowerCase().includes(query) || c.model.includes(query));
  $('cars').replaceChildren(...shown.sort((a, b) => chosen.has(b.model) - chosen.has(a.model)).map((car) => {
    const c = chosen.get(car.model);
    const row = el('div', { className: 'car' },
      el('div', { className: 'row' },
        el('input', { type: 'number', min: 0, max: 64, value: c ? c.count : 0, title: 'how many of this car', onchange: (e) => {
          const count = Math.max(0, Math.round(Number(e.target.value)) || 0);
          if (count) chosen.set(car.model, { skins: new Set(), random: false, ...c, count });
          else chosen.delete(car.model);
          showCars();
        } }),
        el('b', { textContent: car.name }), el('code', { className: 'mute', textContent: car.model })));
    if (c && car.skins.length) {
      const tick = (skin) => el('label', { className: 'tick' }, el('input', { type: 'checkbox', checked: c.skins.has(skin), onchange: (e) => { c.skins[e.target.checked ? 'add' : 'delete'](skin); } }), skin);
      row.append(el('div', { className: 'row skins' },
        el('button', { type: 'button', textContent: 'all skins', onclick: () => { car.skins.forEach((s) => c.skins.add(s)); showCars(); } }),
        el('button', { type: 'button', textContent: 'none', onclick: () => { c.skins.clear(); showCars(); } }),
        el('label', { className: 'tick' }, el('input', { type: 'checkbox', checked: c.random, onchange: (e) => { c.random = e.target.checked; } }), 'random (each car a random one of the ticked skins)'),
        ...car.skins.map(tick)));
    }
    return row;
  }));
}

function edit(p) {
  editing = p;
  idTouched = false;
  const f = $('editor');
  f.hidden = false;
  f.track.replaceChildren(...trackOptions());
  const weathers = state.pack.weather.length ? state.pack.weather : [{ id: p.weather.graphics, name: p.weather.graphics }];
  if (!weathers.some((w) => w.id === p.weather.graphics)) weathers.push({ id: p.weather.graphics, name: `${p.weather.graphics} (not in the pack)` });
  f.graphics.replaceChildren(...weathers.map((w) => el('option', { value: w.id, textContent: w.name })));
  const set = { presetId: p.id, presetName: p.name, track: `${p.track}|${p.layout}`, time: p.time, password: p.password, maxPlayers: p.maxPlayers, pickup: p.pickup ? '1' : '0', idleMinutes: p.idleMinutes,
    practice: p.practice ? p.practice.minutes : 0, qualify: p.qualify ? p.qualify.minutes : 0, raceLaps: p.race ? p.race.laps : 0, raceMinutes: p.race ? p.race.minutes : 0,
    raceWait: p.race ? p.race.waitSeconds : 60, raceOpen: p.race ? p.race.open : 1, ...p.weather, ...p.dynamicTrack,
    rules: Object.entries(p.rules || {}).map(([k, v]) => `${k}=${v}`).join('\n') };
  for (const [k, v] of Object.entries(set)) if (f[k]) f[k].value = v;
  if (!p.id) f.presetId.value = idFromName(p.name, state.presets.map((x) => x.id));
  chosen = new Map(p.cars.map((c) => [c.model, { count: c.count, skins: new Set(c.skins), random: !!c.random }]));
  $('carSearch').value = '';
  showCars();
  f.scrollIntoView();
}

$('editor').presetName.oninput = (e) => {
  if (!editing.id && !idTouched) $('editor').presetId.value = idFromName(e.target.value, state.presets.map((x) => x.id));
};
$('editor').presetId.oninput = () => { idTouched = true; };
$('editor').track.onchange = showCars;
$('carSearch').oninput = showCars;

$('editor').onsubmit = (event) => {
  event.preventDefault();
  const f = $('editor');
  const n = (k) => Number(f[k].value);
  const [track, layout] = f.track.value.split('|');
  const cars = [...chosen].map(([model, c]) => ({ model, count: c.count, skins: [...c.skins], ...(c.random ? { random: true } : {}) }));
  const rules = Object.fromEntries(f.rules.value.split('\n').map((l) => l.trim()).filter(Boolean).map((l) => [l.slice(0, l.indexOf('=')).trim(), l.slice(l.indexOf('=') + 1).trim()]));
  const preset = { id: f.presetId.value, name: f.presetName.value, track, layout, cars,
    practice: n('practice') ? { minutes: n('practice'), open: editing.practice ? editing.practice.open : 1 } : null,
    qualify: n('qualify') ? { minutes: n('qualify'), open: editing.qualify ? editing.qualify.open : 1 } : null,
    race: n('raceLaps') || n('raceMinutes') ? { laps: n('raceLaps'), minutes: n('raceLaps') ? 0 : n('raceMinutes'), waitSeconds: n('raceWait'), open: n('raceOpen') } : null,
    time: f.time.value, password: f.password.value, maxPlayers: n('maxPlayers'), pickup: f.pickup.value === '1', idleMinutes: n('idleMinutes'),
    weather: Object.fromEntries(['ambient', 'road', 'variationAmbient', 'variationRoad', 'windMin', 'windMax', 'windDirection', 'windVariation'].map((k) => [k, n(k)]).concat([['graphics', f.graphics.value]])),
    dynamicTrack: { start: n('start'), randomness: n('randomness'), transfer: n('transfer'), lapGain: n('lapGain') }, rules };
  api('/api/presets', preset).then(() => { f.hidden = true; error(''); refresh(); }).catch((e) => error(e.message));
};
$('cancel').onclick = () => { $('editor').hidden = true; };
$('new').onclick = () => {
  const t = state.pack.tracks[0] || { track: '', layout: '' };
  const clear = state.pack.weather.find((w) => w.id === '3_clear') || state.pack.weather[0] || { id: '3_clear' };
  edit({ id: '', name: 'New server', track: t.track, layout: t.layout, cars: [],
    practice: { minutes: 20, open: 1 }, qualify: { minutes: 10, open: 1 }, race: { laps: 10, minutes: 0, waitSeconds: 60, open: 1 },
    time: '14:00', password: '', maxPlayers: 12, pickup: true, idleMinutes: 30,
    weather: { graphics: clear.id, ambient: 22, road: 8, variationAmbient: 1, variationRoad: 1, windMin: 0, windMax: 8, windDirection: 30, windVariation: 15 },
    dynamicTrack: { start: 96, randomness: 1, transfer: 80, lapGain: 30 }, rules: {} });
};

// every 5 s; every second while a server is starting, to show it running as soon as it is
async function refresh() {
  try {
    state = await api('/api/state');
    showRunning();
    showPresets();
    if (refreshFailed) error('');
    refreshFailed = false;
  } catch (e) {
    error(e.message);
    refreshFailed = true;
  }
}
async function loop() {
  await refresh();
  setTimeout(loop, state?.running.some((r) => r.status === 'starting') ? 1000 : 5000);
}
loop();
