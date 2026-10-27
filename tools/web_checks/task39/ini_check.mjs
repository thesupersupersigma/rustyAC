const OFF_TO_ULTRA = ['Off', 'Low', 'Medium', 'Normal', 'High', 'Ultra'].map((text, i) => [String(i), text]);
const GFX = [
  ['Resolution', 'VIDEO/WIDTH', 'resolution', false],
  ['Rendering Mode', 'CAMERA/MODE', [['DEFAULT', 'Single Screen'], ['TRIPLE', 'Triple Screen'], ['OCULUS', 'Oculus Rift'], ['OPENVR', 'OpenVR {early support}']], false],
  ['Fullscreen Rendering', 'VIDEO/FULLSCREEN', 'check', false],
  ['Vertical Sync', 'VIDEO/VSYNC', 'check', false],
  ['Framerate limit', 'VIDEO/FPS_CAP_MS', [29, 200, 'Off'], false],
  ['Anisotropic Filtering', 'VIDEO/ANISOTROPIC', [['0', 'Off'], ['2', '2x'], ['4', '4x'], ['8', '8x'], ['16', '16x']], true],
  ['Anti-Aliasing Samples', 'VIDEO/AASAMPLES', [['1', 'Off'], ['2', '2x'], ['4', '4x']], true],
  ['World detail', 'ASSETTOCORSA/WORLD_DETAIL', ['Minimum', 'Low', 'Medium', 'High', 'Very High', 'Maximum'].map((text, i) => [String(i), text]), true],
  ['Shadow Resolution', 'VIDEO/SHADOW_MAP_SIZE', [['512', 'Low'], ['1024', 'Medium'], ['2048', 'High'], ['4096', 'Ultra']], true],
  ['Smoke generation', 'EFFECTS/SMOKE', ['Off', 'Very Low', 'Low', 'Normal', 'High', 'Ultra'].map((text, i) => [String(i), text]), true],
  ['Show smoke in mirrors', 'EFFECTS/RENDER_SMOKE_IN_MIRROR', 'check', true],
  ['Enable post-processing effects', 'POST_PROCESS/ENABLED', 'check', true],
  ['Effect preset', 'POST_PROCESS/FILTER', 'filters', false],
  ['Overall effect quality', 'POST_PROCESS/QUALITY', ['Very Low', 'Low', 'Medium', 'Normal', 'High', 'Ultra'].map((text, i) => [String(i), text]), false],
  ['Glare quality', 'POST_PROCESS/GLARE', OFF_TO_ULTRA, false],
  ['Depth of field quality', 'POST_PROCESS/DOF', OFF_TO_ULTRA, false],
  ['Color saturation', 'SATURATION/LEVEL', [0, 120, '0 %'], false],
  ['Motion Blur', 'EFFECTS/MOTION_BLUR', [0, 12, 'Off'], true],
  ['Crepuscular rays', 'POST_PROCESS/RAYS_OF_GOD', 'check', false],
  ['Heat shimmering effects', 'POST_PROCESS/HEAT_SHIMMER', 'check', false],
  ['Fast Approximate Anti-Aliasing', 'POST_PROCESS/FXAA', 'check', true],
  ['Hide driver arms', 'ASSETTOCORSA/HIDE_ARMS', 'check', true],
  ['Lock virtual steering wheel', 'ASSETTOCORSA/LOCK_STEER', 'check', true],
  ['Hide virtual steering wheel', 'ASSETTOCORSA/HIDE_STEER', 'check', true],
  ['Mirror Resolution', 'MIRROR/SIZE', [['0', 'Off'], ['512', 'Normal'], ['1024', 'High']], true],
  ['High Quality Mirror Reflection', 'MIRROR/HQ', 'check', true],
  ['Reflection Quality', 'CUBEMAP/SIZE', [['0', 'None'], ['512', 'Low'], ['1024', 'Medium'], ['2048', 'High']], true],
  ['Reflection Rendering Frequency', 'CUBEMAP/FACES_PER_FRAME', ['Static', 'Very Low', 'Low', 'Medium', 'High', 'Very High', 'Ultra'].map((text, i) => [String(i), text]), true],
];
// High: what the game uses with no video.ini (VideoSettings's defaults), Full mode before 0.39.
// Medium is rustyAC's own; Low is the Hybrid one. SCALE: rustyAC's render scale, in %.
const GFX_HIGH = {
  'VIDEO/WIDTH': '1920', 'VIDEO/HEIGHT': '1080', 'CAMERA/MODE': 'DEFAULT', 'VIDEO/FULLSCREEN': '0', 'VIDEO/VSYNC': '0', 'VIDEO/FPS_CAP_MS': '0',
  'VIDEO/ANISOTROPIC': '4', 'VIDEO/AASAMPLES': '1', 'ASSETTOCORSA/WORLD_DETAIL': '5', 'VIDEO/SHADOW_MAP_SIZE': '2048',
  'EFFECTS/SMOKE': '3', 'EFFECTS/RENDER_SMOKE_IN_MIRROR': '0', 'POST_PROCESS/ENABLED': '0', 'POST_PROCESS/FILTER': 'default',
  'POST_PROCESS/QUALITY': '3', 'POST_PROCESS/GLARE': '3', 'POST_PROCESS/DOF': '3', 'SATURATION/LEVEL': '100', 'EFFECTS/MOTION_BLUR': '0',
  'POST_PROCESS/RAYS_OF_GOD': '0', 'POST_PROCESS/HEAT_SHIMMER': '0', 'POST_PROCESS/FXAA': '0',
  'ASSETTOCORSA/HIDE_ARMS': '0', 'ASSETTOCORSA/LOCK_STEER': '0', 'ASSETTOCORSA/HIDE_STEER': '0',
  'MIRROR/SIZE': '0', 'MIRROR/HQ': '0', 'CUBEMAP/SIZE': '512', 'CUBEMAP/FACES_PER_FRAME': '0', SCALE: '100',
};
const GFX_PRESETS = {
  high: GFX_HIGH,
  medium: { ...GFX_HIGH, 'VIDEO/SHADOW_MAP_SIZE': '1024', 'ASSETTOCORSA/WORLD_DETAIL': '3', 'VIDEO/ANISOTROPIC': '2', 'EFFECTS/SMOKE': '2' },
  low: { ...GFX_HIGH, 'VIDEO/SHADOW_MAP_SIZE': '512', 'ASSETTOCORSA/WORLD_DETAIL': '1', 'EFFECTS/RENDER_SMOKE_IN_MIRROR': '0', 'CUBEMAP/SIZE': '0', 'CUBEMAP/FACES_PER_FRAME': '0', 'POST_PROCESS/ENABLED': '0', 'VIDEO/AASAMPLES': '1', SCALE: '75' },
};
// (the keys a preset sets, which say whether the settings are one)
const GFX_PRESET_KEYS = Object.keys(GFX_HIGH).filter((key) => !['VIDEO/WIDTH', 'VIDEO/HEIGHT', 'VIDEO/FULLSCREEN'].includes(key));
const VIDEO_INI = 'cfg\\video.ini';

// video.ini as {SECTION/KEY: value} (upper case, as the bridge writes it)
function iniValues(text) {
  const values = {};
  let section = '';
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/;.*/, '').trim();
    const head = /^\[(.+)\]$/.exec(line);
    if (head) section = head[1].trim().toUpperCase();
    else if (line.includes('=')) values[`${section}/${line.slice(0, line.indexOf('=')).trim().toUpperCase()}`] = line.slice(line.indexOf('=') + 1).trim();
  }
  return values;
}

// The file's text with these keys set: its other lines kept, new sections at the end.
function iniWith(text, values) {
  const lines = text ? text.split(/\r?\n/) : [];
  for (const [path, value] of Object.entries(values)) {
    const [section, key] = path.split('/');
    let at = lines.findIndex((line) => line.trim().toUpperCase() === `[${section}]`);
    if (at < 0) {
      lines.push(`[${section}]`, `${key}=${value}`);
      continue;
    }
    let end = at + 1;
    while (end < lines.length && !/^\s*\[/.test(lines[end])) {
      if (lines[end].split('=')[0].trim().toUpperCase() === key) break;
      end++;
    }
    if (end < lines.length && !/^\s*\[/.test(lines[end])) lines[end] = `${key}=${value}`;
    else lines.splice(end, 0, `${key}=${value}`);
  }
  return lines.join('\r\n');
}
console.assert(iniValues(iniWith('[VIDEO]\r\nVSYNC=1\r\n[A]\r\nB=2', { 'VIDEO/VSYNC': '0', 'VIDEO/X': '3', 'C/D': '4' }))['VIDEO/X'] === '3', 'iniWith');

// The page's video.ini: the launcher's copy while it runs, else the player's folder in this browser.
function presetOf(values) {
  return Object.keys(GFX_PRESETS).find((name) => GFX_PRESET_KEYS.every((key) => String(GFX_PRESETS[name][key]) === String(values[key]))) || 'custom';
}
const { SCALE, ...low } = GFX_PRESETS.low;
const v = iniValues(iniWith('[VIDEO]\r\nWIDTH=1280\r\n; c\r\n[X]\r\nA=1', low));
console.log(presetOf({ ...GFX_HIGH, ...v, SCALE: '75' }), v['VIDEO/WIDTH'], v['X/A'], v['CUBEMAP/SIZE']);
console.log(presetOf({ ...GFX_HIGH, ...v, 'EFFECTS/SMOKE': '0' }), presetOf(GFX_HIGH), presetOf(GFX_PRESETS.medium));
console.log(iniWith('[VIDEO]\r\nWIDTH=1\r\n[MIRROR]\r\nSIZE=5', { 'VIDEO/VSYNC': '0', 'MIRROR/SIZE': '0' }).replace(/\r\n/g, '|'));
