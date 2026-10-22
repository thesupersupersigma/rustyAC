// SPDX-License-Identifier: MIT OR Apache-2.0
// Presets of on-demand servers: what the pack offers, checking a preset from the page, and the
// server_cfg.ini / entry_list.ini a preset makes (written the way server/spa/*.ini are).
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

// skins: the slots take them in turn (random: a random one each); none: acServer's default skin
export interface Car { model: string; count: number; skins: string[]; random?: boolean }
export interface Session { minutes: number; open: number }
export interface Race { laps: number; minutes: number; waitSeconds: number; open: number }
export interface Weather {
  graphics: string; ambient: number; road: number; variationAmbient: number; variationRoad: number;
  windMin: number; windMax: number; windDirection: number; windVariation: number;
}
export interface DynamicTrack { start: number; randomness: number; transfer: number; lapGain: number }
export interface Preset {
  id: string; name: string; track: string; layout: string; cars: Car[];
  practice: Session | null; qualify: Session | null; race: Race | null;
  time: string; // time of day, HH:MM (08:00 to 18:00), written as SUN_ANGLE
  weather: Weather; dynamicTrack: DynamicTrack;
  password: string; maxPlayers: number; pickup: boolean; idleMinutes: number;
  rules: Record<string, string>; // the other [SERVER] keys
}
export interface Ports { game: number; http: number }
export interface Pack {
  cars: { model: string; name: string; skins: string[] }[];
  tracks: { track: string; layout: string; name: string; pitboxes: number }[]; // pitboxes 0: not known
  weather: { id: string; name: string }[];
}

// The other [SERVER] keys in the order of the fixed servers' files, with their values there.
// (the plugin addresses, the lobby and the ports are not the page's to set)
export const RULES: [string, string][] = [
  ['MAX_BALLAST_KG', '0'], ['QUALIFY_MAX_WAIT_PERC', '120'], ['RACE_PIT_WINDOW_START', '0'], ['RACE_PIT_WINDOW_END', '0'],
  ['REVERSED_GRID_RACE_POSITIONS', '0'], ['LOCKED_ENTRY_LIST', '0'], ['PICKUP_MODE_ENABLED', ''], ['LOOP_MODE', '1'], ['SLEEP_TIME', '1'],
  ['CLIENT_SEND_INTERVAL_HZ', '18'], ['SEND_BUFFER_SIZE', '0'], ['RECV_BUFFER_SIZE', '0'], ['RACE_OVER_TIME', '120'],
  ['KICK_QUORUM', '85'], ['VOTING_QUORUM', '80'], ['VOTE_DURATION', '20'], ['BLACKLIST_MODE', '1'], ['FUEL_RATE', '100'],
  ['DAMAGE_MULTIPLIER', '50'], ['TYRE_WEAR_RATE', '100'], ['ALLOWED_TYRES_OUT', '2'], ['ABS_ALLOWED', '1'], ['TC_ALLOWED', '1'],
  ['START_RULE', '1'], ['RACE_GAS_PENALTY_DISABLED', '0'], ['TIME_OF_DAY_MULT', '1'], ['RESULT_SCREEN_TIME', '30'],
  ['MAX_CONTACTS_PER_KM', '-1'], ['STABILITY_ALLOWED', '0'], ['AUTOCLUTCH_ALLOWED', '1'], ['TYRE_BLANKETS_ALLOWED', '1'],
  ['FORCE_VIRTUAL_MIRROR', '0'], ['REGISTER_TO_LOBBY', ''], ['MAX_CLIENTS', ''], ['NUM_THREADS', '2'], ['UDP_PLUGIN_LOCAL_PORT', ''],
  ['UDP_PLUGIN_ADDRESS', ''], ['AUTH_PLUGIN_ADDRESS', ''], ['LEGAL_TYRES', ''], ['RACE_EXTRA_LAP', '0'], ['WELCOME_MESSAGE', ''],
];
// keys of RULES the preset sets itself (a value of '' in RULES): never from `rules`
const OWN = new Set(['PICKUP_MODE_ENABLED', 'REGISTER_TO_LOBBY', 'MAX_CLIENTS', 'UDP_PLUGIN_LOCAL_PORT', 'UDP_PLUGIN_ADDRESS', 'AUTH_PLUGIN_ADDRESS']);
const TEXT_RULES = new Set(['LEGAL_TYRES', 'WELCOME_MESSAGE']);

export const NAME = /^[a-z0-9][a-z0-9_-]{0,31}$/; // preset ids: container names and URL paths are made of them
const FOLDER = /^[A-Za-z0-9_.-]{1,64}$/; // car, track, layout, skin and weather folder names
const TEXT = /^[\x20-\x7e]{0,120}$/; // one line of plain text (no line breaks, nothing for the ini to choke on)

// What the pack (server/make_server_pack.py's folder) has: its index.json (menu names, skins,
// pit boxes, weathers); a pack made before there was one: cars with a data.acd, tracks and
// layouts with a data/surfaces.ini, under their folder names.
export function readPack(dir: string): Pack {
  if (existsSync(join(dir, 'index.json'))) {
    const index = JSON.parse(readFileSync(join(dir, 'index.json'), 'utf8'));
    return { cars: index.cars, tracks: index.tracks, weather: index.weather };
  }
  const dirs = (path: string) => (existsSync(path) ? readdirSync(path, { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name).sort() : []);
  const cars = dirs(join(dir, 'content', 'cars')).filter((c) => existsSync(join(dir, 'content', 'cars', c, 'data.acd'))).map((model) => ({ model, name: model, skins: [] }));
  const tracks: Pack['tracks'] = [];
  for (const track of dirs(join(dir, 'content', 'tracks'))) {
    const at = join(dir, 'content', 'tracks', track);
    if (existsSync(join(at, 'data', 'surfaces.ini'))) tracks.push({ track, layout: '', name: track, pitboxes: 0 });
    for (const layout of dirs(at)) if (existsSync(join(at, layout, 'data', 'surfaces.ini'))) tracks.push({ track, layout, name: `${track} / ${layout}`, pitboxes: 0 });
  }
  return { cars, tracks, weather: [] };
}

// SUN_ANGLE of a time of day as AC's launcher writes it (13:00 is 0, 16 degrees an hour).
export const sunAngle = (time: string) => {
  const [h, m] = time.split(':').map(Number);
  return Math.round(((h * 60 + m - 780) * 16) / 60);
};
export const timeOfDay = (angle: number) => {
  const minutes = Math.round(780 + (angle * 60) / 16);
  return `${String(Math.floor(minutes / 60)).padStart(2, '0')}:${String(minutes % 60).padStart(2, '0')}`;
};

// The preset as the page sent it, checked field by field against the pack: a list of what is
// wrong (empty when it can be saved). Nothing unchecked reaches a file, Docker or a shell.
export function checkPreset(p: Preset, pack: Pack): string[] {
  const bad: string[] = [];
  const int = (v: unknown, lo: number, hi: number, what: string) => {
    if (!Number.isInteger(v) || (v as number) < lo || (v as number) > hi) bad.push(`${what}: a whole number from ${lo} to ${hi}`);
  };
  if (typeof p !== 'object' || !p) return ['no preset'];
  if (!NAME.test(String(p.id))) bad.push('id: lower-case letters, digits, - and _ (up to 32)');
  if (typeof p.name !== 'string' || !TEXT.test(p.name) || !p.name.trim()) bad.push('name: one line of plain text');
  const track = pack.tracks.find((t) => t.track === p.track && t.layout === p.layout);
  if (!track) bad.push(`track: ${p.track}${p.layout ? ' / ' + p.layout : ''} is not in the pack`);
  if (!Array.isArray(p.cars) || !p.cars.length) bad.push('cars: at least one');
  else {
    for (const c of p.cars) {
      const known = pack.cars.find((k) => k.model === c?.model);
      if (!known) bad.push(`car ${c?.model}: not in the pack`);
      int(c?.count, 1, 64, `car ${c?.model}: count`);
      if (!Array.isArray(c?.skins) || !c.skins.every((s) => typeof s === 'string' && FOLDER.test(s))) bad.push(`car ${c?.model}: skins are folder names`);
      else if (known?.skins.length && !c.skins.every((s) => known.skins.includes(s))) bad.push(`car ${c?.model}: a skin it does not have`);
      if (c?.random !== undefined && typeof c.random !== 'boolean') bad.push(`car ${c?.model}: random is yes or no`);
      else if (c?.random && !c.skins?.length) bad.push(`car ${c?.model}: random needs the skins to pick from`);
    }
    if (new Set(p.cars.map((c) => c?.model)).size !== p.cars.length) bad.push('cars: each model once');
    const slots = p.cars.reduce((n, c) => n + (Number(c?.count) || 0), 0);
    if (track?.pitboxes && slots > track.pitboxes) bad.push(`cars: ${slots}, but the track has ${track.pitboxes} pit boxes`);
  }
  for (const s of ['practice', 'qualify'] as const) {
    if (p[s] !== null) {
      int(p[s]?.minutes, 1, 600, `${s} minutes`);
      int(p[s]?.open, 0, 1, `${s} open`);
    }
  }
  if (p.race !== null) {
    int(p.race?.laps, 0, 500, 'race laps');
    int(p.race?.minutes, 0, 600, 'race minutes');
    if (!p.race?.laps === !p.race?.minutes) bad.push('race: laps or minutes, one of the two');
    int(p.race?.waitSeconds, 0, 600, 'race wait seconds');
    int(p.race?.open, 0, 2, 'race open');
  }
  if (!p.practice && !p.qualify && !p.race) bad.push('sessions: at least one');
  if (!/^([01]\d|2[0-3]):[0-5]\d$/.test(String(p.time)) || Math.abs(sunAngle(p.time)) > 80) bad.push('time: HH:MM from 08:00 to 18:00');
  const w = p.weather;
  if (!w || !FOLDER.test(String(w.graphics)) || (pack.weather.length && !pack.weather.some((x) => x.id === w.graphics))) bad.push('weather graphics: a weather in the pack');
  else {
    int(w.ambient, -20, 50, 'ambient temperature'); int(w.road, -20, 50, 'road temperature (over ambient)');
    int(w.variationAmbient, 0, 20, 'ambient variation'); int(w.variationRoad, 0, 20, 'road variation');
    int(w.windMin, 0, 100, 'wind min'); int(w.windMax, 0, 100, 'wind max');
    int(w.windDirection, 0, 359, 'wind direction'); int(w.windVariation, 0, 180, 'wind direction variation');
  }
  const d = p.dynamicTrack;
  if (!d) bad.push('dynamic track: missing');
  else {
    int(d.start, 0, 100, 'grip at start'); int(d.randomness, 0, 100, 'grip randomness');
    int(d.transfer, 0, 100, 'grip kept between sessions'); int(d.lapGain, 0, 1000, 'laps for 1% grip');
  }
  if (typeof p.password !== 'string' || !/^[\x21-\x7e]{0,32}$/.test(p.password)) bad.push('password: up to 32 printable characters, no spaces');
  int(p.maxPlayers, 1, 64, 'max players');
  if (typeof p.pickup !== 'boolean') bad.push('pickup: yes or no');
  int(p.idleMinutes, 1, 1440, 'idle minutes');
  for (const [k, v] of Object.entries(p.rules ?? {})) {
    if (!RULES.some(([r]) => r === k) || OWN.has(k)) bad.push(`rule ${k}: not one the panel sets`);
    else if (typeof v !== 'string' || !(TEXT_RULES.has(k) ? TEXT.test(v) : /^-?\d{1,6}$/.test(v))) bad.push(`rule ${k}: ${TEXT_RULES.has(k) ? 'one line of plain text' : 'a whole number'}`);
  }
  return bad;
}

const section = (name: string, keys: [string, string | number][]) => `[${name}]\n${keys.map(([k, v]) => `${k}=${v}\n`).join('')}`;

// server_cfg.ini of a preset. The passwords stay out: the entrypoint fills them in from the
// container's environment, as for the fixed servers.
export function serverCfg(p: Preset, ports: Ports): string {
  const own: Record<string, string> = {
    PICKUP_MODE_ENABLED: p.pickup ? '1' : '0', REGISTER_TO_LOBBY: '0', MAX_CLIENTS: String(p.maxPlayers),
    UDP_PLUGIN_LOCAL_PORT: '0', UDP_PLUGIN_ADDRESS: '', AUTH_PLUGIN_ADDRESS: '',
  };
  const server: [string, string | number][] = [
    ['NAME', p.name], ['CARS', p.cars.map((c) => c.model).join(';')], ['CONFIG_TRACK', p.layout], ['TRACK', p.track],
    ['SUN_ANGLE', sunAngle(p.time)], ['PASSWORD', ''], ['ADMIN_PASSWORD', '@ADMIN_PASSWORD@'],
    ['UDP_PORT', ports.game], ['TCP_PORT', ports.game], ['HTTP_PORT', ports.http],
    ...RULES.map(([k, v]): [string, string] => [k, own[k] ?? p.rules?.[k] ?? v]),
  ];
  const parts = [section('SERVER', server)];
  if (p.practice) parts.push(section('PRACTICE', [['NAME', 'Practice'], ['TIME', p.practice.minutes], ['IS_OPEN', p.practice.open]]));
  if (p.qualify) parts.push(section('QUALIFY', [['NAME', 'Qualify'], ['TIME', p.qualify.minutes], ['IS_OPEN', p.qualify.open]]));
  if (p.race) parts.push(section('RACE', [['NAME', 'Race'], p.race.laps ? ['LAPS', p.race.laps] : ['TIME', p.race.minutes], ['WAIT_TIME', p.race.waitSeconds], ['IS_OPEN', p.race.open]]));
  const d = p.dynamicTrack;
  parts.push(section('DYNAMIC_TRACK', [['SESSION_START', d.start], ['RANDOMNESS', d.randomness], ['SESSION_TRANSFER', d.transfer], ['LAP_GAIN', d.lapGain]]));
  const w = p.weather;
  parts.push(section('WEATHER_0', [
    ['GRAPHICS', w.graphics], ['BASE_TEMPERATURE_AMBIENT', w.ambient], ['BASE_TEMPERATURE_ROAD', w.road],
    ['VARIATION_AMBIENT', w.variationAmbient], ['VARIATION_ROAD', w.variationRoad], ['WIND_BASE_SPEED_MIN', w.windMin],
    ['WIND_BASE_SPEED_MAX', w.windMax], ['WIND_BASE_DIRECTION', w.windDirection], ['WIND_VARIATION_DIRECTION', w.windVariation],
  ]));
  return parts.join('\n');
}

// entry_list.ini: the models take turns (as in the fixed servers' lists), each model's skins in
// turn or, with random, a random one of them each; no skins: SKIN= empty (acServer's default).
export function entryList(p: Preset, random = Math.random): string {
  const left = p.cars.map((c) => c.count);
  const used = p.cars.map(() => 0);
  const out: string[] = [];
  while (left.some((n) => n > 0)) {
    p.cars.forEach((c, i) => {
      if (!left[i]) return;
      left[i]--;
      const skin = !c.skins.length ? '' : c.random ? c.skins[Math.floor(random() * c.skins.length)] : c.skins[used[i]++ % c.skins.length];
      out.push(`${section(`CAR_${out.length}`, [['MODEL', c.model], ['SKIN', skin], ['SPECTATOR_MODE', 0], ['DRIVERNAME', ''], ['TEAM', ''], ['GUID', ''], ['BALLAST', 0], ['RESTRICTOR', 0]])}\n`);
    });
  }
  return out.join('');
}

// A preset made from a fixed server's server_cfg.ini and entry_list.ini (the panel's "copy of spa").
export function presetFromCfg(id: string, cfgText: string, entryText: string): Preset {
  const ini = (text: string) => {
    const out: Record<string, Record<string, string>> = {};
    let at = '';
    for (const raw of text.split(/\r?\n/)) {
      const line = raw.trim();
      if (!line || line.startsWith(';')) continue;
      if (line.startsWith('[')) out[(at = line.slice(1, line.indexOf(']')).toUpperCase())] = {};
      else if (at && line.includes('=')) out[at][line.slice(0, line.indexOf('=')).trim().toUpperCase()] = line.slice(line.indexOf('=') + 1).trim();
    }
    return out;
  };
  const cfg = ini(cfgText);
  const entries = ini(entryText);
  const s = cfg.SERVER ?? {};
  const n = (v: string | undefined, d = 0) => (v === undefined || v === '' || Number.isNaN(Number(v)) ? d : Math.round(Number(v)));
  const cars: Car[] = [];
  for (const e of Object.keys(entries).filter((k) => k.startsWith('CAR_')).sort((a, b) => n(a.slice(4)) - n(b.slice(4))).map((k) => entries[k])) {
    let car = cars.find((c) => c.model === e.MODEL);
    if (!car) cars.push((car = { model: e.MODEL, count: 0, skins: [] }));
    car.count++;
    if (e.SKIN && !car.skins.includes(e.SKIN)) car.skins.push(e.SKIN);
  }
  const session = (x?: Record<string, string>) => (x ? { minutes: n(x.TIME), open: n(x.IS_OPEN, 1) } : null);
  const w = cfg.WEATHER_0 ?? {};
  const d = cfg.DYNAMIC_TRACK ?? {};
  const rules: Record<string, string> = {};
  for (const [k, v] of RULES) if (!OWN.has(k) && s[k] !== undefined && s[k] !== v) rules[k] = s[k]; // only what differs from the defaults
  return {
    id, name: s.NAME ?? id, track: s.TRACK ?? '', layout: s.CONFIG_TRACK ?? '', cars,
    practice: session(cfg.PRACTICE), qualify: session(cfg.QUALIFY),
    race: cfg.RACE ? { laps: n(cfg.RACE.LAPS), minutes: n(cfg.RACE.LAPS) ? 0 : n(cfg.RACE.TIME), waitSeconds: n(cfg.RACE.WAIT_TIME), open: n(cfg.RACE.IS_OPEN, 1) } : null,
    time: timeOfDay(n(s.SUN_ANGLE)),
    weather: {
      graphics: w.GRAPHICS ?? '3_clear', ambient: n(w.BASE_TEMPERATURE_AMBIENT, 22), road: n(w.BASE_TEMPERATURE_ROAD, 8),
      variationAmbient: n(w.VARIATION_AMBIENT, 1), variationRoad: n(w.VARIATION_ROAD, 1), windMin: n(w.WIND_BASE_SPEED_MIN),
      windMax: n(w.WIND_BASE_SPEED_MAX), windDirection: n(w.WIND_BASE_DIRECTION), windVariation: n(w.WIND_VARIATION_DIRECTION),
    },
    dynamicTrack: { start: n(d.SESSION_START, 96), randomness: n(d.RANDOMNESS, 1), transfer: n(d.SESSION_TRANSFER, 80), lapGain: n(d.LAP_GAIN, 30) },
    password: '', maxPlayers: n(s.MAX_CLIENTS, cars.reduce((a, c) => a + c.count, 0)), pickup: s.PICKUP_MODE_ENABLED !== '0',
    idleMinutes: 30, rules,
  };
}
