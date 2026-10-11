# Media for the website: high-resolution screenshots and engine clips

## Resume here

State: **finished** (v0.23.1). Everything asked for was made except `tyre_pop.wav`,
`pit_jack_up.wav` and `pit_jack_down.wav`, which Assetto Corsa has no sound for (see "Not done").

- The media are in `rustyACSite\assets-src\media\` (16 pictures, 18 WAVs, `audio/manifest.json`,
  `README.md`). Nothing of it is in this repo.
- To make everything again, from the rustyAC folder, with Assetto Corsa installed:
  1. `cargo build --release -p rustyac-game --example site_clips` (and `cargo build --release`)
  2. `sh tools/site_media/shots.sh` (about 3 minutes; pictures into `re/scratch/site_media/final/`)
  3. `sh tools/site_media/audio.sh` (about 2 minutes; raw clips into `re/scratch/site_media/raw/`)
  4. `python tools/site_media/post.py re/scratch/site_media/raw <media>/audio <version>`, then copy
     the pictures to `<media>/screenshots/`
- The site itself was not touched: its pages still use the old files in
  `src/assets/screenshots/`. Putting the new ones in is the site's job.

## Plain-English summary

The website's pictures were small (1280x720) and soft. rustyAC can now draw a picture four
times larger than asked for and shrink it, which gives clean edges and fine detail, so the new
pictures are 2560x1440 at the best quality the renderer has. It can also leave its own debug
panels off the picture and put a camera anywhere beside the car, which the old pictures could
not do. With that, 16 pictures were made: the three before/after pairs of the site's slider
(the same moments as before, both halves from exactly the same car position), a Ferrari F2004
burnout in tyre smoke for the homepage and for the share image, and eight gallery pictures
(the F2004 at speed, its cockpit, dusk with headlights, heavy clouds, crash damage, an F40 and
a 488 GT3).

For the homepage's hold-to-rev, 18 sound clips were made. They are Assetto Corsa's own F2004
sounds, played by rustyAC's sound code and written straight to files without a sound card, so
they come out the same on any PC. There are twelve engine loops (six engine speeds, each with
the throttle open and closed), the engine on its rev limiter, the tyre squeal of spinning rear
wheels, an exhaust backfire, the pit-stop wheel gun (on and off), and one 32-second recording
of a real burnout driven by rustyAC's physics until a rear tyre bursts. Every loop repeats
without a click, and all clips share one volume setting, so their loudness relative to each
other is the game's.

Three things the task asked for do not exist in Assetto Corsa and so were not made: the game
plays no sound when a tyre bursts, and it has no jack sounds. The F2004 also has no separate
rev-limiter sound: the limiter clip is its engine sound with the revs bouncing on the limit.

## What was made

Every picture command starts with `target\release\rustyac.exe`; every sound command is one run
of `target\release\examples\site_clips.exe`, after which `tools/site_media/post.py` cuts,
loops and scales the clip. Paths are relative to `rustyACSite\assets-src\media\`.

| File | Size / length | What | Command |
|---|---|---|---|
| `screenshots/renderer_core_spa.png` | 2560x1440, 5.8 MB | F2004 at Spa, 12 s into the automatic drive, AC's renderer | `rustyac.exe --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 12 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --motion-blur 0 --supersample 2 --no-hud --screenshot renderer_core_spa.png` |
| `screenshots/renderer_core_spa_debug.png` | 2560x1440, 4.7 MB | the same frame, debug view | `rustyac.exe --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 12 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --motion-blur 0 --supersample 2 --no-hud --debug-view --screenshot renderer_core_spa_debug.png` |
| `screenshots/renderer_core_monza.png` | 2560x1440, 6.2 MB | BMW 1M at Monza, 12 s, AC's renderer | `rustyac.exe --car bmw_1m --track monza --no-race-ini --autodrive --at 12 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --motion-blur 0 --supersample 2 --no-hud --screenshot renderer_core_monza.png` |
| `screenshots/renderer_core_monza_debug.png` | 2560x1440, 5.4 MB | the same frame, debug view | `rustyac.exe --car bmw_1m --track monza --no-race-ini --autodrive --at 12 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --motion-blur 0 --supersample 2 --no-hud --debug-view --screenshot renderer_core_monza_debug.png` |
| `screenshots/renderer_core_laguna_seca.png` | 2560x1440, 5.0 MB | BMW M3 E30 at Laguna Seca, 12 s, AC's renderer | `rustyac.exe --car bmw_m3_e30 --track ks_laguna_seca --no-race-ini --autodrive --at 12 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --motion-blur 0 --supersample 2 --no-hud --screenshot renderer_core_laguna_seca.png` |
| `screenshots/renderer_core_laguna_seca_debug.png` | 2560x1440, 4.2 MB | the same frame, debug view | `rustyac.exe --car bmw_m3_e30 --track ks_laguna_seca --no-race-ini --autodrive --at 12 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --motion-blur 0 --supersample 2 --no-hud --debug-view --screenshot renderer_core_laguna_seca_debug.png` |
| `screenshots/hero_burnout.png` | 2560x1440, 7.2 MB | F2004 burnout at Spa under the start gantry, 5 s in, from low at the front right | `rustyac.exe --car ks_ferrari_f2004 --track spa --no-race-ini --flat-out --flat-out-brake 0.6 --no-auto-clutch --at 5 --lead-in 5 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --motion-blur 0 --supersample 2 --no-hud --free-camera 5,0.5,3/0,0.5,-1/50 --screenshot hero_burnout.png` |
| `screenshots/og_burnout.png` | 2400x1260, 6.3 MB | the same burnout for the share image: car and smoke on the left, the right third calm (track, wall, hillside) | `rustyac.exe --car ks_ferrari_f2004 --track spa --no-race-ini --flat-out --flat-out-brake 0.6 --no-auto-clutch --at 5 --lead-in 5 --width 2400 --height 1260 --graphics max --cube-size 2048 --aa 8 --motion-blur 0 --supersample 2 --no-hud --free-camera 5,0.5,3/-0.9,0.7,0.2/50 --screenshot og_burnout.png` |
| `screenshots/gallery_f2004_spa_chase.png` | 2560x1440, 5.7 MB | F2004 at 175 km/h at Spa, chase camera, motion blur 4 | `rustyac.exe --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 21.6 --lead-in 1 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --supersample 2 --no-hud --motion-blur 4 --screenshot gallery_f2004_spa_chase.png` |
| `screenshots/gallery_f2004_spa_trackside.png` | 2560x1440, 6.6 MB | F2004 at 259 km/h past the Spa grandstand, from trackside | `rustyac.exe --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 19 --lead-in 1 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --supersample 2 --no-hud --motion-blur 0 --free-camera 4,0.7,11/0,0.5,0/22 --screenshot gallery_f2004_spa_trackside.png` |
| `screenshots/gallery_f2004_cockpit.png` | 2560x1440, 4.8 MB | F2004 cockpit at Spa | `rustyac.exe --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 21.6 --lead-in 1 --camera cockpit --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --supersample 2 --no-hud --motion-blur 0 --screenshot gallery_f2004_cockpit.png` |
| `screenshots/gallery_night_lights.png` | 2560x1440, 7.2 MB | BMW 1M at Spa after sunset (sun angle 86, as dark as plain AC gets), headlights on | `rustyac.exe --car bmw_1m --track spa --no-race-ini --sun 86 --show-lights --autodrive --at 12 --lead-in 1 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --supersample 2 --no-hud --motion-blur 0 --free-camera -3.5,0.9,7/0,0.6,0/40 --screenshot gallery_night_lights.png` |
| `screenshots/gallery_heavy_clouds.png` | 2560x1440, 5.1 MB | F2004 at Spa under the 7_heavy_clouds weather | `rustyac.exe --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 30 --lead-in 1 --camera chase2 --weather 7_heavy_clouds --sun 30 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --supersample 2 --no-hud --motion-blur 0 --screenshot gallery_heavy_clouds.png` |
| `screenshots/gallery_crash_damage.png` | 2560x1440, 5.9 MB | BMW 1M with crash damage drawn (scratches, bent panels) | `rustyac.exe --car bmw_1m --track spa --no-race-ini --show-damage 70,65,80,75,80 --at 2 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --supersample 2 --no-hud --motion-blur 0 --free-camera -3,1.0,5/0,0.6,0.5/40 --screenshot gallery_crash_damage.png` |
| `screenshots/gallery_f40_monza.png` | 2560x1440, 5.2 MB | Ferrari F40 at Monza | `rustyac.exe --car ferrari_f40 --track monza --no-race-ini --autodrive --at 12 --lead-in 1 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --supersample 2 --no-hud --motion-blur 0 --free-camera -4,0.7,7/0,0.6,0/38 --screenshot gallery_f40_monza.png` |
| `screenshots/gallery_488_gt3_laguna_seca.png` | 2560x1440, 4.5 MB | Ferrari 488 GT3 at Laguna Seca | `rustyac.exe --car ks_ferrari_488_gt3 --track ks_laguna_seca --no-race-ini --autodrive --at 14 --lead-in 1 --width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --supersample 2 --no-hud --motion-blur 0 --free-camera -4.5,0.9,8/0,0.6,0/35 --screenshot gallery_488_gt3_laguna_seca.png` |
| `audio/engine_on_idle.wav` | 7.75 s, peak -11.99 dBFS | loop, 4000 rpm, throttle 1 | `site_clips steady --rpm 4000 --gas 1 --seconds 14 --volume 0.12 --out engine_on_idle.wav` |
| `audio/engine_off_idle.wav` | 7.75 s, peak -13.94 dBFS | loop, 4000 rpm, throttle 0 | `site_clips steady --rpm 4000 --gas 0 --seconds 14 --volume 0.12 --out engine_off_idle.wav` |
| `audio/engine_on_4000.wav` | 7.75 s, peak -12.07 dBFS | loop, 4000 rpm, throttle 1 | `site_clips steady --rpm 4000 --gas 1 --seconds 14 --volume 0.12 --out engine_on_4000.wav` |
| `audio/engine_off_4000.wav` | 7.75 s, peak -13.94 dBFS | loop, 4000 rpm, throttle 0 | `site_clips steady --rpm 4000 --gas 0 --seconds 14 --volume 0.12 --out engine_off_4000.wav` |
| `audio/engine_on_7000.wav` | 7.80 s, peak -5.8 dBFS | loop, 7000 rpm, throttle 1 | `site_clips steady --rpm 7000 --gas 1 --seconds 14 --volume 0.12 --out engine_on_7000.wav` |
| `audio/engine_off_7000.wav` | 7.76 s, peak -8.42 dBFS | loop, 7000 rpm, throttle 0 | `site_clips steady --rpm 7000 --gas 0 --seconds 14 --volume 0.12 --out engine_off_7000.wav` |
| `audio/engine_on_10000.wav` | 8.03 s, peak -3.01 dBFS | loop, 10000 rpm, throttle 1 | `site_clips steady --rpm 10000 --gas 1 --seconds 14 --volume 0.12 --out engine_on_10000.wav` |
| `audio/engine_off_10000.wav` | 7.99 s, peak -4.43 dBFS | loop, 10000 rpm, throttle 0 | `site_clips steady --rpm 10000 --gas 0 --seconds 14 --volume 0.12 --out engine_off_10000.wav` |
| `audio/engine_on_13000.wav` | 8.22 s, peak -3.0 dBFS | loop, 13000 rpm, throttle 1 | `site_clips steady --rpm 13000 --gas 1 --seconds 14 --volume 0.12 --out engine_on_13000.wav` |
| `audio/engine_off_13000.wav` | 7.78 s, peak -7.62 dBFS | loop, 13000 rpm, throttle 0 | `site_clips steady --rpm 13000 --gas 0 --seconds 14 --volume 0.12 --out engine_off_13000.wav` |
| `audio/engine_on_16000.wav` | 8.25 s, peak -3.38 dBFS | loop, 16000 rpm, throttle 1 | `site_clips steady --rpm 16000 --gas 1 --seconds 14 --volume 0.12 --out engine_on_16000.wav` |
| `audio/engine_off_16000.wav` | 7.76 s, peak -3.89 dBFS | loop, 16000 rpm, throttle 0 | `site_clips steady --rpm 16000 --gas 0 --seconds 14 --volume 0.12 --out engine_off_16000.wav` |
| `audio/limiter.wav` | 7.75 s, peak -3.54 dBFS | loop, 18800 rpm, throttle 1 | `site_clips drive --neutral --seconds 16 --volume 0.12 --out limiter.wav` |
| `audio/wheelspin.wav` | 7.83 s, peak -15.85 dBFS | loop | `site_clips steady --rpm 18500 --gas 1 --spin 110 --slip 1 --speed 5.56 --engine-level 0 --seconds 14 --volume 0.12 --out wheelspin.wav` |
| `audio/pit_wheelgun.wav` | 1.02 s, peak -18.83 dBFS | oneshot | `site_clips event --path event:/common/screw --seconds 6 --volume 0.12 --out pit_wheelgun.wav` |
| `audio/pit_wheelgun_off.wav` | 1.23 s, peak -20.83 dBFS | oneshot | `site_clips event --path event:/common/unscrew --seconds 6 --volume 0.12 --out pit_wheelgun_off.wav` |
| `audio/backfire_1.wav` | 0.48 s, peak -12.15 dBFS | oneshot | `site_clips event --path event:/cars/ks_ferrari_f2004/backfire_ext --param throttle=0 --at exhaust --base-volume 2.5 --seconds 4 --volume 0.12 --out backfire_1.wav` |
| `audio/burnout_reference.wav` | 32.10 s, peak -1.0 dBFS | oneshot | `site_clips drive --track spa --brake 0.6 --after-pop 2 --seconds 60 --volume 0.12 --out burnout_reference.wav` |
| `audio/manifest.json` | | every clip: kind, rpm, throttle, loop points, length, peak, the common gain, FMOD events | `python tools/site_media/post.py re/scratch/site_media/raw <media>/audio 0.23.1` |
| `README.md` | | what each file is and its command | |

### How the pictures were made

- Quality: `--graphics max` (shadows 4096, world detail 5, anisotropic 16, mirrors 1024 in high
  quality, smoke 5, the post-processing frame with FXAA), then on top of it `--cube-size 2048`
  (the launcher's largest reflection map), `--aa 8` (the most samples AC has) and
  `--supersample 2` (drawn at 5120x2880 and averaged down). `--world-detail 5` is part of the
  preset.
- `--motion-blur 0` on all but one: the preset's blur 6 smears a still picture. The chase
  picture has blur 4 with a one-second lead-in, where it shows speed.
- `--hdr-levels` was not used: without YEBIS' tone mapper it washes the picture out.
- The pairs use the original scenes and `--at 12`. Both halves of a pair come from the same
  car state: `--pose-out` of the two commands is byte-identical for all three scenes (the
  physics does not depend on the renderer).
- The burnout: `--flat-out --flat-out-brake 0.6 --at 5 --lead-in 5`. With the throttle down and
  the brake at 0.6 the F2004's front wheels hold it back while the rear ones spin, so it creeps
  round under the start gantry in its smoke. Five seconds in is the most smoke on the tarmac:
  later the car wanders onto the grass (dust instead of smoke), and a rear tyre bursts after
  29.5 s.
- `og_burnout.png` is the same moment with the camera turned so that car and smoke sit in the
  left 60 %; the right is track, the green wall and the hillside.

### How the clips were made

- **Steady rpm: the engine event's own parameters, without the physics** (the task's "best"
  way). `site_clips steady` gives `CarAudioFMOD` the same hand-made car state every frame:
  `rpms` and `throttle` of `engine_ext` are exactly the numbers on the command line.
- **Listener**: the free camera's sound mode (the game makes the engine 2.5 times louder for
  the track and free cameras), 6 m from the car: 5.5 m to its right, 2.5 m behind its centre,
  1.2 m up, looking at the car. No track is loaded for the sound: no ambience, crowd or reverb.
  Wind level 0. No music exists in this path.
- **Gain**: FMOD ran at master level 0.12 so that its 16-bit writer never clips; afterwards
  every clip was multiplied by the same 8.38 dB. The loudest clip is `burnout_reference.wav`
  (peak -1.0 dBFS); `limiter.wav` peaks at -3.5 dBFS. (The task named the limiter as the
  loudest; the real burnout turned out louder, so it sets the gain.)
- **Loops**: each raw clip is 14 s. The loop is cut from second 3 on (the master level ramps
  up over the first 0.9 s), about 8 s long; the end is placed where the sound is most like
  the sound before the cut, and the last 0.5 s is crossfaded into it. The whole file is the
  loop (`loop_start` 0, `loop_end` = sample count in the manifest).
- **Measured** (all in `manifest.json`):
  - seams: the jump from the last sample to the first is smaller than 99.9 % of the jumps
    between neighbouring samples inside the same file, for all 14 loops;
  - pitch rises with rpm: the engine's strongest line is 81.8 Hz at 4,000 rpm (its octave,
    163.6 Hz, is the matching harmonic), then 288.5, 415.5, 540.0, 669.2 Hz at 7,000 to 16,000
    and 780.8 Hz on the limiter: 0.0412 to 0.0418 Hz per rpm throughout;
  - nothing clips: no sample reaches full scale in any file.
- `engine_*_idle` and `engine_*_4000` are the same state: the F2004's idle (`engine.ini
  MINIMUM`) is 4,000 rpm. They are separate renders (FMOD's mix is never the same twice).
- `limiter.wav`: rustyAC's physics in neutral with the throttle held down (`site_clips drive
  --neutral`); the engine sits between 18,700 and 18,800 rpm with the limiter cutting in and out.
- `wheelspin.wav`: AC makes the skid sound from the car's state, not from a parameter: a wheel
  that spins (slip ratio above 0.8) sounds at pitch 1.2 with a volume of `speed in km/h x 0.05`.
  A car that does not move at all is therefore silent in AC. The clip uses the rear wheels at
  110 rad/s (what the real burnout shows) and tells the sound the car does 20 km/h, which is
  the full volume; the engine level is 0 so only the tyres are heard (skid plus the rolling
  `wheel` event). It is 20 dB quieter than the engine at full throttle: that is the game's mix.
- `backfire_1.wav`: the event `backfire_ext` started once behind the rear axle with
  `throttle` 0, at the volume `CarAudioFMOD` gives it for this camera. Three renders came out
  the same, so there is one file.
- `burnout_reference.wav`: `site_clips drive --track spa --brake 0.6 --after-pop 2`. The same
  drive as the pictures: spawn, 1.2 s of rest, first gear, a second of revs, clutch dropped,
  wheelspin with the car creeping about; the right rear tyre bursts after 29.45 s and the clip
  ends 2 s later. The listener moves with the car.

## Code added, and why

Nothing existing changes its output: every new option does nothing unless given, and the
proofs do not use them.

| Where | What | Why |
|---|---|---|
| `rustyac.exe --supersample <1..4>` | draws the screenshot n times wider and higher and averages it down (`render::box_downscale`) | the cheap supersampling switch the task asked for; the drawn picture may be 7680x4320 at most |
| `rustyac.exe --no-hud` | a screenshot without rustyAC's panels and without the virtual mirror | the old site pictures had the debug panels on top |
| `rustyac.exe --free-camera r,u,a[/r,u,a[/fov]]` | a camera standing still beside the car of the picture (`scene::free_camera`) | only AC's driving cameras existed; the task wants low side and trackside views |
| `rustyac.exe --flat-out-brake <0..1>` | the brake held during `--flat-out` (`FlatOutSource::with_brake`) | without it the F2004 simply drives off; with 0.6 it burns out nearly on the spot |
| `crates/rustyac-game/examples/site_clips.rs` | renders one clip per run through FMOD's clock-less WAV writer: `steady`, `event`, `drive` | the game could only record a whole drive from a driving camera |
| `tools/site_media/shots.sh`, `audio.sh`, `post.py` | the batches and the cutting / looping / gain / manifest step | to make the media again |

Tests: `cli::tests::the_screenshot_options_of_the_site_pictures`,
`render::tests::box_downscale_averages_each_block`,
`render::scene::tests::free_camera_stands_level_beside_the_car`. Re-run at the end:

- `cargo test --release --workspace --locked`: 43 test binaries, 217 tests passed, 0 failed
  (the golden render, golden audio and golden chassis tests among them);
- clippy `-D warnings`: native (with the examples), `wasm32-unknown-unknown` (`rustyac-web`) and
  `wasm32-wasip1` (the five crates CI checks), all clean; `--locked` builds of the workspace and
  of every tool pass after the version bump;
- the v0.23.0 `rustyac.exe` (built from the tag in a scratch worktree) against the new one,
  with none of the new options: four screenshots (AC's renderer at Spa with its command log,
  the debug view at Monza, the F40 burnout with `--graphics max` and a lead-in, the cockpit
  with FXAA and motion blur) are byte-identical PNGs with identical `--pose-out`, and four
  replays (`audio_e2e_spa_kerb_strike`, `car_wc_abs1_wc_stops`,
  `t17_ks_ferrari_250_gto_pt_autoshift`, `t18_trento_bondone___ks_ferrari_f2004_trk_kerb_strike`)
  give byte-identical `--dump-states`;
- the long oracle batches (render proofs, audio drives, the car oracle) were **not** run again:
  the change touches `crates/rustyac-game` only (`cli.rs`, `desktop.rs`, `sim.rs`,
  `render/hud.rs`, `render/mod.rs`, `render/scene.rs`, one example). `rustyac-render`,
  `rustyac-audio`, `rustyac-physics` and every tool's sources are untouched.

## Sound events in AC's banks

Found in `content/sfx/GUIDs.txt` and the code (`CarAudioFMOD`, `PitCrew`):

| Kind | FMOD path | Rendered |
|---|---|---|
| engine | `event:/cars/ks_ferrari_f2004/engine_ext` (parameters `rpms`, `throttle`), `engine_int` | `engine_on_*`, `engine_off_*`, `limiter` (exterior) |
| limiter | `event:/cars/<car>/limiter` (parameter `decay`): 112 cars have it, **the F2004 does not** | no: `limiter.wav` is the engine on the limiter |
| tyres | `event:/cars/ks_ferrari_f2004/skid_ext`, `skid_int` (no parameter; volume and pitch set by the code), `wheel` (`speed`, `brake`, `inflation`, `suspension_damage`) | `wheelspin.wav` |
| surfaces | `event:/surfaces/tyre_rolling`, `grass`, `gravel`, `kerb`, `old`, `sand`, `extraturf`, `event:/common/dirt` | no (tarmac plays none) |
| backfire | `event:/cars/ks_ferrari_f2004/backfire_ext`, `backfire_int` (parameter `throttle`) | `backfire_1.wav` (exterior) |
| tyre burst | **none**: no event for a burst or a flat tyre in any bank; the only hook is `wheel`'s `inflation`, and a render with it at 0 measured the same as at 1 (-40.1 dB both) | no |
| pit stop | `event:/common/screw`, `event:/common/unscrew` (the wheel gun; `PitCrew` plays them) | `pit_wheelgun.wav`, `pit_wheelgun_off.wav` |
| jack | **none** | no |
| collisions | `event:/collisions/car/hit`, `car/scrape`, `track/hit`, `track/scrape`, `object/hit` | no |
| other F2004 events | `gear_ext`, `gear_int`, `gear_grind`, `tractioncontrol_ext`, `tractioncontrol_int`, `transmission`, `bodywork`, `wind` | `gear_ext` is heard in the reference recording |
| common | `event:/common/ds_protection`, `event:/common/ambience` | no |

## Not done

- `tyre_pop.wav`: AC has no such sound (see the table). In `burnout_reference.wav` nothing is
  heard at the burst but the wheel speeding up.
- `pit_jack_up.wav`, `pit_jack_down.wav`: AC has no jack sounds.
- The site's pages were not changed to use the new files (the task was media only).
- Nobody listened to the clips: they were checked by measurement only.

## Open questions

1. **The HUD.** The old pair pictures showed rustyAC's debug panels; the new ones do not
   (`--no-hud`). Drop `--no-hud` from the commands if the panels are wanted back.
2. **Skins in two pairs.** In the Monza and Laguna Seca pairs the debug view draws a white car
   and AC's renderer an orange / a black one: the two renderers pick a different default skin.
   The old pictures were made the same way. `--skin <folder>` would make them match.
3. **The burnout is not on the spot.** With nobody steering, the F2004 creeps around at 5 to
   20 km/h while its rear wheels spin. A car held perfectly still would be silent in AC (skid
   volume follows speed) and stalls with more brake.
4. **`wheelspin.wav` is quiet** next to the engine (rms -32 dBFS against -12): the game's own
   mix at 6 m. The site may want to raise it; the manifest has the levels.
5. **Idle = 4,000 rpm** for this car, so the idle and 4,000 clips are the same sound.
6. **The gain** is set by the reference recording, not by the limiter clip, because it is the
   loudest. If the reference is not used on the site, 2.5 dB more would fit.
7. **Pit clips are quiet** (-19 dBFS peak): the wheel gun was rendered 6 m away like the rest.
8. **Night**: plain AC has no real night; sun angle 86 is dusk.
9. **Rain**: AC has no rain weather; `7_heavy_clouds` is the darkest.
10. **Content**: the pictures and clips are made of Assetto Corsa's cars, tracks and sounds.
    Whether the site may publish them is for you to decide.
