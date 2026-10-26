// SPDX-License-Identifier: GPL-3.0-or-later

// AC's sound in the page: FMOD Studio 2.03 for HTML5 (Firelight's fmodstudio.js and .wasm, from
// the preview pack: never part of rustyAC) under the port's sound code. The wasm side
// (crates/rustyac-audio, fmod/web.rs) calls `globalThis.rustyacSound.call(name, text, args)`
// for every FMOD 1.08 function acs.exe uses: `args` are the call's 32-bit words (handles, ints,
// float bits, structures), `text` its one string; it gets the result and FMOD's words back. This file
// translates each into FMOD 2.03's JavaScript API; the call log above stays the desktop's.
//
// What 1.08 and 2.03 answer differently, and what is done about it:
//   getLowLevelSystem          -> getCoreSystem
//   setParameterValue          -> setParameterByName
//   getMaximumDistance         -> getMinMaxDistance (the max)
//   getVersion                 -> 0x10812, the version the game's code checks for
//   setListenerAttributes      -> the same with no attenuation position
//   getListenerAttributes      -> 1.08's default before the first set (2.03 flips its z)
//   setCallback                -> nothing (a C function cannot be handed to the JavaScript build)
//   Geometry::addPolygon       -> FMOD_ERR_UNSUPPORTED (not in the HTML5 build: no occluders)
//   setGeometrySettings        -> FMOD_OK (unsupported there, and nothing for it to size)
//   getGeometryOcclusion       -> 0, 0, answered in the wasm (no polygons, nothing occludes)
//   the DSP plug-ins           -> left out (see studio_register_plugin)
// Every object handed out gets a number of its own; the same FMOD object asked for again (the
// core system, the master group, an event description by its GUID) gets the same number, as
// its address on the desktop.

const f32 = new Float32Array(1);
const u32 = new Uint32Array(f32.buffer);
const F = (w) => ((u32[0] = w), f32[0]);
const W = (x) => ((f32[0] = x), u32[0]);
const V = (a, i) => ({ x: F(a[i]), y: F(a[i + 1]), z: F(a[i + 2]) });
const vw = (v) => [W(v.x), W(v.y), W(v.z)];
const A = (a, i) => ({ position: V(a, i), velocity: V(a, i + 3), forward: V(a, i + 6), up: V(a, i + 9) });
// (FMOD fills a 3D attributes object it is handed with keys like 'position.x')
const aw = (x) => ['position', 'velocity', 'forward', 'up'].flatMap((k) => ['x', 'y', 'z'].map((c) => W(x[`${k}.${c}`])));

// Loads FMOD (`js`: fmodstudio.js's text, `wasm`: fmodstudio.wasm's bytes) and puts the bridge
// up for `rac` (the page's wasm module). Resolves to the page's handle on the sound.
export async function loadSound(js, wasm, rac, { window: win = globalThis.window } = {}) {
  const FMODModule = new Function(js + '\nreturn FMODModule;')();
  const FMOD = { wasmBinary: wasm };
  if (win) FMOD.window = win;
  await new Promise((resolve, reject) => {
    FMOD.onRuntimeInitialized = resolve;
    FMODModule(FMOD).catch(reject);
  });

  const objects = new Map(); // number -> FMOD object
  const keyed = new Map(); // key -> number
  let next = 0x100;
  const give = (object, key) => {
    let word = key !== undefined ? keyed.get(key) : undefined;
    if (word === undefined) {
      word = next;
      next += 16;
      if (key !== undefined) keyed.set(key, word);
    }
    objects.set(word, object);
    return word;
  };
  const forget = (word) => {
    objects.delete(word);
    for (const [key, w] of keyed) if (w === word) keyed.delete(key);
  };
  const obj = (word) => objects.get(word);
  const handed = (r, o, key) => (r === FMOD.OK ? [r, give(o.val, key)] : [r]);
  const one = (r, o) => [r, o.val | 0];

  const state = { volume: 1, muted: false, gameVolume: 1, drivers: null, banks: 0, studio: null };
  const applyVolume = () => {
    const master = keyed.has('master') ? obj(keyed.get('master')) : null;
    if (master) master.setVolume(state.gameVolume * (state.muted ? 0 : state.volume));
  };

  const core = (a) => obj(a[0]);
  const T = {
    // --- FMOD::System ---
    system_set_driver: (a) => [core(a).setDriver(a[1])],
    // the game only looks for its audio.ini's device by name: a browser has one, the default
    system_get_driver_info: () => [FMOD.OK, 0, 0, 48000, 3, 2],
    system_set_software_channels: (a) => [core(a).setSoftwareChannels(a[1])],
    system_get_output: (a) => { const o = {}; return one(core(a).getOutput(o), o); },
    c_system_get_driver: (a) => { const o = {}; return one(core(a).getDriver(o), o); },
    system_get_driver: (a) => { const o = {}; return one(core(a).getDriver(o), o); },
    system_set_software_format: (a) => [core(a).setSoftwareFormat(a[1], a[2], a[3])],
    system_get_software_format: (a) => {
      const rate = {}, mode = {}, raw = {};
      return [core(a).getSoftwareFormat(rate, mode, raw), rate.val | 0, mode.val | 0, raw.val | 0];
    },
    system_get_version: () => [FMOD.OK, 0x10812],
    system_get_channels_playing: (a) => { const c = {}, r = {}; return [core(a).getChannelsPlaying(c, r), c.val | 0, r.val | 0]; },
    system_get_dsp_buffer_size: (a) => { const l = {}, n = {}; return [core(a).getDSPBufferSize(l, n), l.val | 0, n.val | 0]; },
    system_set_callback: () => [FMOD.OK],
    system_get_3d_settings: (a) => {
      const d = {}, s = {}, r = {};
      return [core(a).get3DSettings(d, s, r), W(d.val), W(s.val), W(r.val)];
    },
    system_get_num_drivers: (a) => { const o = {}; const r = core(a).getNumDrivers(o); return [r, state.drivers ?? (o.val | 0)]; },
    system_set_3d_settings: (a) => [core(a).set3DSettings(F(a[1]), F(a[2]), F(a[3]))],
    system_create_reverb_3d: (a) => { const o = {}; return handed(core(a).createReverb3D(o), o); },
    system_get_master_channel_group: (a) => {
      const o = {};
      const r = core(a).getMasterChannelGroup(o);
      if (r === FMOD.OK && state.capture && !keyed.has('master')) {
        const desc = FMOD.DSP_DESCRIPTION();
        desc.name = 'rustyAC capture';
        desc.numinputbuffers = 1;
        desc.numoutputbuffers = 1;
        desc.read = (dsp, inbuffer, outbuffer, length, inchannels, outchannels) => {
          const block = FMOD.HEAPF32.slice(inbuffer >> 2, (inbuffer >> 2) + length * inchannels);
          FMOD.HEAPF32.set(block, outbuffer >> 2);
          state.capture.push(block);
          state.channels = inchannels;
          return FMOD.OK;
        };
        const dsp = {};
        core(a).createDSP(desc, dsp);
        // (the master group mixes in 7.1; FMOD gives this DSP the stereo the output gets)
        dsp.val.setChannelFormat(0, 0, FMOD.SPEAKERMODE_STEREO);
        o.val.addDSP(FMOD.CHANNELCONTROL_DSP_HEAD, dsp.val);
      }
      return r === FMOD.OK ? [r, give(o.val, 'master')] : [r];
    },
    system_create_geometry: (a) => { const o = {}; return handed(core(a).createGeometry(a[1], a[2], o), o); },
    // (unsupported in the HTML5 build, which has no polygons to size the occlusion tree for)
    system_set_geometry_settings: () => [FMOD.OK],
    system_set_advanced_settings: (a) => {
      // 1.08's FMOD_ADVANCEDSETTINGS (0x78 bytes) as words: the game sets vol0virtualvol, a
      // clock-less run the random seed
      const s = FMOD.ADVANCEDSETTINGS();
      core(a).getAdvancedSettings(s);
      s.vol0virtualvol = F(a[1 + 0x44 / 4] ?? 0);
      s.randomSeed = a[1 + 0x74 / 4] ?? 0;
      return [core(a).setAdvancedSettings(s)];
    },
    // --- Reverb3D, Geometry, ChannelControl ---
    reverb_set_active: (a) => [obj(a[0]).setActive(!!a[1])],
    reverb_set_properties: (a) => {
      const p = FMOD.REVERB_PROPERTIES();
      ['DecayTime', 'EarlyDelay', 'LateDelay', 'HFReference', 'HFDecayRatio', 'Diffusion', 'Density', 'LowShelfFrequency', 'LowShelfGain', 'HighCut', 'EarlyLateMix', 'WetLevel'].forEach((k, i) => (p[k] = F(a[1 + i])));
      return [obj(a[0]).setProperties(p)];
    },
    reverb_get_3d_attributes: (a) => {
      const p = {}, min = {}, max = {};
      const r = obj(a[0]).get3DAttributes(p, min, max);
      return r === FMOD.OK ? [r, ...vw(p), W(min.val), W(max.val)] : [r];
    },
    reverb_set_3d_attributes: (a) => [obj(a[0]).set3DAttributes(V(a, 1), F(a[4]), F(a[5]))],
    reverb_release: (a) => { const r = obj(a[0]).release(); forget(a[0]); return [r]; },
    geometry_set_active: (a) => [obj(a[0]).setActive(!!a[1])],
    geometry_set_polygon_attributes: (a) => [obj(a[0]).setPolygonAttributes(a[1], F(a[2]), F(a[3]), !!a[4])],
    geometry_get_num_polygons: (a) => { const o = {}; return one(obj(a[0]).getNumPolygons(o), o); },
    geometry_release: (a) => { const r = obj(a[0]).release(); forget(a[0]); return [r]; },
    geometry_add_polygon: () => [FMOD.ERR_UNSUPPORTED],
    channel_set_reverb_properties: (a) => [obj(a[0]).setReverbProperties(a[1], F(a[2]))],
    channel_set_volume: (a) => {
      // the master group carries the page's own volume on top of the game's
      if (keyed.get('master') === a[0]) {
        state.gameVolume = F(a[1]);
        return [obj(a[0]).setVolume(state.gameVolume * (state.muted ? 0 : state.volume))];
      }
      return [obj(a[0]).setVolume(F(a[1]))];
    },
    // --- Studio ---
    studio_create: () => {
      const o = {};
      const r = FMOD.Studio_System_Create(o);
      if (r === FMOD.OK) state.studio = o.val;
      return handed(r, o, 'studio');
    },
    studio_get_low_level_system: (a) => { const o = {}; return handed(obj(a[0]).getCoreSystem(o), o, 'core'); },
    studio_set_advanced_settings: (a) => {
      // 1.08's FMOD_STUDIO_ADVANCEDSETTINGS: cbsize, commandQueueSize, handleInitialSize,
      // studioUpdatePeriod, idleSampleDataPoolSize (0: FMOD's default)
      const s = FMOD.STUDIO_ADVANCEDSETTINGS();
      obj(a[0]).getAdvancedSettings(s);
      if (a[2]) s.commandqueuesize = a[2];
      if (a[3]) s.handleinitialsize = a[3];
      if (a[4]) s.studioupdateperiod = a[4];
      if (a[5]) s.idlesampledatapoolsize = a[5];
      return [obj(a[0]).setAdvancedSettings(s)];
    },
    studio_initialize: (a) => [obj(a[0]).initialize(a[1], a[2] | FMOD.STUDIO_INIT_ALLOW_MISSING_PLUGINS, a[3], null)],
    // shortcut: the game's two plug-ins (gain, distance filter) are left out: FMOD's HTML5 build
    // creates a JavaScript plug-in only through createDSP, never from a bank (Studio's
    // registerPlugin loses the parameters, the core's registerDSP the callbacks), so the banks
    // load with them missing (ALLOW_MISSING_PLUGINS) and those effects are not heard. Upgrade
    // when FMOD's HTML5 build runs JavaScript plug-ins from banks.
    studio_register_plugin: () => [FMOD.OK],
    studio_update: (a) => [obj(a[0]).update()],
    studio_flush_commands: (a) => [obj(a[0]).flushCommands()],
    studio_release: (a) => { const r = obj(a[0]).release(); forget(a[0]); state.studio = null; return [r]; },
    studio_get_cpu_usage: () => [FMOD.OK, 0],
    studio_get_buffer_usage: () => [FMOD.OK, 0],
    studio_get_listener_attributes: (a) => {
      // a listener never set: 1.08 answers its default as stored, 2.03 turns it right-handed
      if (!state.listeners?.has(a[1])) return [FMOD.OK, 0, 0, 0, 0, 0, 0, 0, 0, W(1), 0, W(1), 0];
      const o = {}, at = {};
      const r = obj(a[0]).getListenerAttributes(a[1], o, at);
      return r === FMOD.OK ? [r, ...aw(o)] : [r];
    },
    studio_set_listener_attributes: (a) => {
      (state.listeners ||= new Set()).add(a[1]);
      return [obj(a[0]).setListenerAttributes(a[1], A(a, 2), null)];
    },
    studio_get_event_by_id: (a) => {
      const g = FMOD.GUID();
      g.Data1 = a[1];
      g.Data2 = a[2] & 0xffff;
      g.Data3 = a[2] >>> 16;
      g.Data4 = [0, 1, 2, 3, 4, 5, 6, 7].map((i) => (a[3 + (i >> 2)] >>> ((i & 3) * 8)) & 0xff);
      const o = {};
      return handed(obj(a[0]).getEventByID(g, o), o, `event:${a[1]},${a[2]},${a[3]},${a[4]}`);
    },
    studio_load_bank_file: (a, text) => {
      // the bank's bytes are the page's file: they go into FMOD's own file system
      const bytes = rac.sound_read_file(text);
      if (!bytes) return [FMOD.ERR_FILE_NOTFOUND];
      const name = `bank${state.banks++}.bank`;
      FMOD.FS_createDataFile('/', name, bytes, true, false);
      const o = {};
      return handed(obj(a[0]).loadBankFile('/' + name, a[1], o), o);
    },
    bank_unload: (a) => { const r = obj(a[0]).unload(); forget(a[0]); return [r]; },
    // --- EventDescription ---
    description_load_sample_data: (a) => [obj(a[0]).loadSampleData()],
    description_unload_sample_data: (a) => [obj(a[0]).unloadSampleData()],
    description_create_instance: (a) => { const o = {}; return handed(obj(a[0]).createInstance(o), o); },
    description_get_maximum_distance: (a) => { const min = {}, max = {}; const r = obj(a[0]).getMinMaxDistance(min, max); return [r, W(max.val)]; },
    // --- EventInstance ---
    event_set_parameter_value: (a, text) => [obj(a[0]).setParameterByName(text, F(a[1]), false)],
    event_release: (a) => { const r = obj(a[0]).release(); forget(a[0]); return [r]; },
    event_get_playback_state: (a) => { const o = {}; return one(obj(a[0]).getPlaybackState(o), o); },
    event_set_timeline_position: (a) => [obj(a[0]).setTimelinePosition(a[1] | 0)],
    event_stop: (a) => [obj(a[0]).stop(a[1])],
    event_start: (a) => [obj(a[0]).start()],
    event_set_paused: (a) => [obj(a[0]).setPaused(!!a[1])],
    event_get_paused: (a) => { const o = {}; const r = obj(a[0]).getPaused(o); return [r, o.val ? 1 : 0]; },
    event_set_3d_attributes: (a) => [obj(a[0]).set3DAttributes(A(a, 1))],
    event_get_3d_attributes: (a) => { const o = {}; const r = obj(a[0]).get3DAttributes(o); return r === FMOD.OK ? [r, ...aw(o)] : [r]; },
    event_set_pitch: (a) => [obj(a[0]).setPitch(F(a[1]))],
    event_set_volume: (a) => [obj(a[0]).setVolume(F(a[1]))],
    event_get_channel_group: (a) => { const o = {}; return handed(obj(a[0]).getChannelGroup(o), o); },
    // --- what the layer itself needs ---
    x_set_output: (a) => {
      // the HTML5 build has no WAV writer: a proof's mix is caught at the master group instead
      if (a[1] === FMOD.OUTPUTTYPE_WAVWRITER_NRT) {
        state.capture = [];
        return [core(a).setOutput(FMOD.OUTPUTTYPE_NOSOUND_NRT)];
      }
      return [core(a).setOutput(a[1])];
    },
    x_set_dsp_buffer_size: (a) => [core(a).setDSPBufferSize(a[1], a[2])],
    x_flush_sample_loading: (a) => [obj(a[0]).flushSampleLoading()],
    x_get_sample_loading_state: (a) => {
      // no loader thread here: what is asked for is loaded now
      state.studio?.flushSampleLoading();
      const o = {};
      return one(obj(a[0]).getSampleLoadingState(o), o);
    },
  };

  globalThis.rustyacSound = {
    // `args`: the call's words; the answer: the result, then the words FMOD gave back
    call(name, text, args) {
      const t = T[name];
      if (!t) return Uint32Array.of(FMOD.ERR_UNSUPPORTED);
      if (name !== 'studio_create' && !objects.has(args[0])) return Uint32Array.of(FMOD.ERR_INVALID_HANDLE);
      return Uint32Array.from(t(args, text), (w) => w >>> 0);
    },
    // the setters the wasm kept back (one `name\ttext` line each, their words each led by its
    // count), then one call that answers: one crossing per answer, not per call
    batch(queued, words, name, text, args) {
      let at = 0;
      for (const line of queued.split('\n')) {
        if (!line) continue;
        const tab = line.indexOf('\t');
        const n = words[at];
        const r = this.call(line.slice(0, tab), line.slice(tab + 1), words.subarray(at + 1, at + 1 + n))[0];
        at += 1 + n;
        if (r !== FMOD.OK) console.warn(`sound: ${line.slice(0, tab)} answered ${r} after the wasm logged 0`);
      }
      return this.call(name, text, args);
    },
  };

  // the browser starts sound only after the player's first click or key
  if (win) {
    const resume = () => {
      FMOD.OutputAudioWorklet_resumeAudio?.();
      FMOD.OutputWebAudio_resumeAudio?.();
      FMOD.mContext?.resume?.();
      FMOD.context?.resume?.();
    };
    for (const kind of ['click', 'keydown', 'touchend', 'pointerdown']) win.addEventListener(kind, resume, { capture: true });
    // a hidden tab is silent and its mixer stops
    win.document?.addEventListener('visibilitychange', () => {
      const c = keyed.has('core') ? obj(keyed.get('core')) : null;
      if (!c) return;
      if (win.document.hidden) c.mixerSuspend();
      else {
        c.mixerResume();
        resume();
      }
    });
  }

  return {
    FMOD,
    state,
    // a proof's mix as a 16-bit WAV file (as FMOD's WAV writer writes it), or null
    wav() {
      if (!state.capture) return null;
      const n = state.capture.reduce((sum, b) => sum + b.length, 0);
      const out = new DataView(new ArrayBuffer(44 + n * 2));
      const text = (at, t) => [...t].forEach((c, i) => out.setUint8(at + i, c.charCodeAt(0)));
      const ch = state.channels || 2;
      text(0, 'RIFF'); out.setUint32(4, 36 + n * 2, true); text(8, 'WAVEfmt ');
      out.setUint32(16, 16, true); out.setUint16(20, 1, true); out.setUint16(22, ch, true);
      out.setUint32(24, 48000, true); out.setUint32(28, 48000 * ch * 2, true); out.setUint16(32, ch * 2, true); out.setUint16(34, 16, true);
      text(36, 'data'); out.setUint32(40, n * 2, true);
      let at = 44;
      for (const b of state.capture) for (const x of b) { out.setInt16(at, Math.round(Math.max(-1, Math.min(1, x)) * 32767), true); at += 2; }
      return new Uint8Array(out.buffer);
    },
    // the page's volume (0..1) and mute, on top of the game's master level
    setVolume(volume, muted) {
      state.volume = Math.min(1, Math.max(0, Number(volume) || 0));
      state.muted = !!muted;
      applyVolume();
    },
  };
}
