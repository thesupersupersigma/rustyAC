# Every option, in one place

What you can type after `rustyac.exe` and the other programs, what you can put in the browser
page's address, and the settings and variables rustyAC reads. Tests keep this page and the
programs in step: an option that is added, renamed or removed without this page fails the build's
tests.

Two tags:

- **not AC**: an extra that Assetto Corsa does not have. Everything else behaves as the game does.
- **checks**: made for the project's own tests and for developers. Harmless, but of little use
  while driving.

Options are typed in a terminal (PowerShell or Command Prompt) in the folder of the program.
An option that takes a value is followed by it: `--car bmw_z4_gt3`. Assetto Corsa's own files
are only ever read, never changed.

## Common recipes

Drive a car on a track, in a window:

    rustyac.exe --car bmw_z4_gt3 --track spa --windowed

Join a server (the server names the track; you ask for a car):

    rustyac.exe --join 192.168.1.20:9600 --car bmw_z4_gt3 --name "Your Name"

Record a lap and watch it again (the inputs of every physics step, played back exactly):

    rustyac.exe --car bmw_z4_gt3 --track spa --windowed --record lap.ryin
    rustyac.exe --replay lap.ryin --windowed

Take a screenshot five seconds into a drive, with nobody at the wheel:

    rustyac.exe --car bmw_z4_gt3 --track spa --autodrive --screenshot shot.png --width 1920 --height 1080 --at 5

Run without a window for a minute (a line follower drives):

    rustyac.exe --car bmw_z4_gt3 --track spa --autodrive --headless --duration 60

## rustyac.exe

The game. Without any option it starts as Assetto Corsa's last session says (the `race.ini` that
the launcher or Content Manager wrote), or with the Ferrari F2004 on an endless flat road when
there is no such file. `rustyac.exe --help` prints the same list in short.

### Car and track

| Option | Default | What it does |
|---|---|---|
| `--car <name\|folder>` | `ks_ferrari_f2004` | The car, by its folder name in Assetto Corsa's `content\cars`, or a path to a car folder. Example: `--car bmw_z4_gt3`. |
| `--skin <folder>` | race.ini's, else the first | The car's skin (its paint). |
| `--track <name>` | the endless flat road | The track: its folder name (`ks_laguna_seca`), the name the game's menu shows, or a part of either that fits one track (`laguna`). |
| `--layout <name>` | race.ini's layout of this track, else the first | The track's layout. Needs `--track`. Example: `--track ks_nurburgring --layout layout_gp_a`. |
| `--list-tracks` | | Prints every installed track and layout (and why some are refused), then stops. |
| `--flat` | on | The endless flat road. |
| `--spawn <hotlap\|pit\|start>` | where race.ini's session starts, else `hotlap` | Where the car starts on a track: just before the line, in the pits, or on the grid. |

### Session and conditions

| Option | Default | What it does |
|---|---|---|
| `--race-ini [file]` | on when the file exists | Takes the session from Assetto Corsa's last session file (`Documents\Assetto Corsa\cfg\race.ini`, with `assists.ini` beside it): temperatures, grip, wind, ballast, aids. The file name is optional. |
| `--no-race-ini` | | The built-in conditions instead: 26 C air, 30 C road, grip 100 %, no wind. |
| `--air <C>` | race.ini's, else 26 | Air temperature. |
| `--road <C>` | race.ini's, else 30 | Road temperature. |
| `--grip <percent>` | race.ini's, else 100 | The track's grip, fixed. Example: `--grip 97`. |
| `--wind <km/h>` | race.ini's, else none | The wind, exactly this strong. 0: none. |
| `--wind-dir <deg>` | race.ini's | The direction the wind comes from. |
| `--wind-from-log` | | The wind Assetto Corsa drew in its last session (read from its `log.txt`), for a lap in the same wind as the lap just driven in the game. |
| `--sun <angle>` | race.ini's | The sun's angle: -80 sunrise, 0 one o'clock, 80 sunset, beyond that night. |
| `--weather <name>` | race.ini's | A folder of Assetto Corsa's `content\weather`. Example: `--weather 7_heavy_clouds`. |
| `--time-mult <n>` | race.ini's | How fast the sun and the clouds move. |
| `--sessions <list>` | race.ini's sessions | The sessions without a race.ini. `p10,q5,r3` is a practice of 10 minutes, a qualifying of 5 and a race of 3 laps; `rt20` is a race of 20 minutes, `rt20+` the same and the lap after. |
| `--no-referee` | | No race rules. |
| `--penalties <0\|1>` | race.ini's | Penalties off or on. |
| `--jump-start <0\|1\|2>` | race.ini's | What a jump start costs (race.ini's `JUMP_START_PENALTY`). |
| `--pit-window <open>,<close>` | none | A mandatory pit stop window, in the leader's laps (minutes in a timed race). |
| `--no-pit-stop` | | No pit stop for your car. |
| `--race-start-in <s>` | | The green light of a race comes this many seconds after the session's start. |
| `--start-lights <s>` | | A race start for the picture: the start lights count down, the start being this many seconds away. The car is not held on the grid. |
| `--esc-start`, `--no-esc-start` | on for a drive at the wheel in a window | The session begins in the ESC menu with the car held until you click Drive, as in the game, or it does not. Off by default for replays, unattended drives, pictures and online. |
| `--out-dir <folder>` | `out` beside rustyac.exe | Where the results (`race_out.json`, `laps.ini`) go. |
| `--air-density <kg/m3>` | the game's formula | **not AC.** A fixed air density, to try what Custom Shaders Patch's thinner air at altitude does (about 1.165 at Spa at 14 C). |
| `--pressure-law <a>,<b>` | the game's formula | **not AC.** Tyre pressure = a + b x (core temperature - 26 C), to try what Custom Shaders Patch's pressures do. Example: `--pressure-law 10.97,0.084`. |

### Setup, aids and damage

| Option | Default | What it does |
|---|---|---|
| `--setup <name\|file>` | the car's default setup | A saved setup: a file, or a name in `Documents\Assetto Corsa\setups\<car>\<track>` (then `generic`). |
| `--setups-dir <folder>` | `setups` beside rustyac.exe | Where the setup screen saves. |
| `--no-ac-setups` | | The setups of `Documents\Assetto Corsa\setups` are not listed. |
| `--damage <percent>` | assists.ini's | Mechanical damage: 0 off, 100 full. At 0 a crash bends no suspension and no tyre can burst. |
| `--auto-shifter`, `--no-auto-shifter` | assists.ini's, else off | The automatic gearbox on or off. Alt+G or Ctrl+G switches it while driving. |
| `--auto-clutch`, `--no-auto-clutch` | on with a pad or the keyboard, else assists.ini's | The automatic clutch on or off. With a pad or the keyboard the game forces it on. |
| `--auto-blip`, `--no-auto-blip` | assists.ini's | The automatic throttle blip on down-shifts, on or off. |
| `--ideal-line`, `--no-ideal-line` | assists.ini's | The ideal line (the coloured arrows on the road) on or off from the start. Ctrl+I switches it while driving. |

### Sound

The sound is Assetto Corsa's own: its FMOD DLLs and sound banks are loaded from its folder.
Without them rustyAC is silent.

| Option | Default | What it does |
|---|---|---|
| `--no-audio` | | No sound. |
| `--volume <0..1>` | audio.ini's master level | Master volume. Example: `--volume 0.5`. |
| `--audio-ini <file>` | `Documents\Assetto Corsa\cfg\audio.ini` | The sound settings from this file instead. |
| `--audio-wav <file>` | | Writes the sound to a WAV file instead of playing it. Works with `--headless` too. |
| `--sound-tape <file>` | | With `--audio-wav`: the sound of a browser page's sound tape, every frame as the page heard it. **checks** |
| `--audio-log <file>` | | Writes every call to the sound engine to a text file. **checks** |
| `--audio-null` | | The sound engine runs but nothing is heard (for timing runs). **checks** |

### Graphics, window and screenshots

The video settings come from Assetto Corsa's `Documents\Assetto Corsa\cfg\video.ini`; the options
here win over it. There is no option for the post-processing filter, triple screens or VR: those
are read from Assetto Corsa's own settings (see "VR, screens and input").

| Option | Default | What it does |
|---|---|---|
| `--windowed` | borderless full screen | A window instead of full screen. |
| `--width <px>`, `--height <px>` | 1280 x 720 | The size of the window with `--windowed`, or of the picture of `--screenshot`. |
| `--vsync <0\|1>` | 1 | Wait for the display, or not. |
| `--no-focus` | | Shows the window without activating it and keeps running in the background. |
| `--camera <name>` | `chase` | The view to start in: `chase`, `chase2`, `bonnet`, `bumper`, `dash`, `cockpit`, `track`, or `car0`, `car1` ... (the car's own cameras). F1 goes through the first six while driving. |
| `--virtual-mirror` | | The virtual mirror on from the start (F11 switches it). Needs mirrors in video.ini. |
| `--show-fps` | | The frame-rate counter on from the start (Ctrl+F or F12 switches it). |
| `--graphics <max\|low>` | your video.ini | Built-in video settings instead of your video.ini: everything at its highest, or everything off or at its lowest. The options below still win. |
| `--video-ini <file>` | `Documents\Assetto Corsa\cfg\video.ini` | The video settings from this file instead. |
| `--video-ini-exact` | | Follows video.ini also where plain Assetto Corsa cannot make a proper picture with it (the values Content Manager writes for Custom Shaders Patch: everything in shadow, no reflections). |
| `--aa <n>` | video.ini's | Anti-aliasing samples per pixel: 1, 2, 4 or 8. |
| `--post <0\|1>` | video.ini's | Post-processing off or on (filter, tone mapping, glare, light shafts, depth of field, heat shimmer). |
| `--fxaa <0\|1>` | video.ini's | FXAA. 1 switches post-processing on too. |
| `--motion-blur <n>` | video.ini's | Motion blur, 0 to 12. Above 0 it switches post-processing on too. |
| `--hdr-levels` | | With post-processing: the engine's HDR light levels as the game sets them. |
| `--cube-size <n>` | video.ini's | The size of the reflections: 512, 1024 or 2048. 2048 is costly, in the game too. |
| `--cube-faces <n>` | video.ini's | How many of the six reflection faces are drawn again every frame, 0 to 6. |
| `--mirror-size <n>` | video.ini's | The mirror picture's width. 0: no mirrors. |
| `--mirror-hq <0\|1>` | video.ini's | The high-quality mirror. |
| `--world-detail <0..5>` | video.ini's | World detail: clouds and skid marks need 1, crowds 3 to 5. |
| `--texture-size <px>` | 1024 | The longest side of a texture on the graphics card. 0: as stored. |
| `--no-textures` | | Flat colours instead of textures. |
| `--boxes` | | Draws the car as boxes, not with its 3D model. |
| `--language <code>` | the game's, else `en` | The language of the menus and apps: `en`, `it`, `de`, `fr`, `es`, `chs`, `cht`. |
| `--dev-apps` | | Assetto Corsa's developer apps get their icons in the app bar. |
| `--debug-hud` | | rustyAC's own panels and numbers too (F10 switches them in the window). **not AC** |
| `--no-gui` | | Without Assetto Corsa's apps, messages, flags and name tags. |
| `--screenshot <png>` | | Draws one frame off screen into a PNG and stops. |
| `--at <s>` | the end of the replay, or 3 | With `--screenshot`: how many seconds into the drive. |
| `--no-hud` | | With `--screenshot`: the picture alone, without apps and panels. |
| `--supersample <n>` | 1 | With `--screenshot`: draws the picture 2, 3 or 4 times wider and higher and averages it down, for smoother edges. The drawn picture may be 7680 x 4320 at most. |
| `--free-camera <r,u,a>[/<r,u,a>[/<fov>]]` | | With `--screenshot`: a camera beside the car. Its place in metres to the right of, above and ahead of the car, then the point it looks at (default `0,0.4,0`) and its field of view in degrees (default 60). Example: `--free-camera -4,0.6,-5/0,0.4,0/45` is low, behind and to the left. |
| `--lead-in <s>` | | With `--screenshot`: draws the last seconds frame by frame first, so that smoke, skid marks and flames are in the picture. |
| `--flat-out` | | With `--screenshot`: nobody steers, the throttle is held down from the start (a launch with wheelspin). |
| `--flat-out-brake <0..1>` | 0 | With `--flat-out`: the brake held too. Around 0.6 the F2004 does a burnout on the spot. |
| `--show-lights` | | With `--screenshot`: the headlights on. |
| `--show-damage <f,r,l,r,c>` | | With `--screenshot`: the car drawn with these damage levels (km/h of impact: front, rear, left, right, centre). |

### Multiplayer

| Option | Default | What it does |
|---|---|---|
| `--join <host:port>` | | Joins a Kunos acServer (its TCP port), as Assetto Corsa does. The server names the track and gives you a car. `--join ws://...` or `--join wss://...` goes through a WebSocket gateway next to the server (see "rustyac-gateway"). Without it, a start without `--car` and `--track` joins the server Content Manager wrote into race.ini. |
| `--name <name>` | your Steam name, else your Windows user name | Your driver name. |
| `--team <name>` | none | Your team. |
| `--password <text>` | none | The server's password. |
| `--guid <id>` | Steam's, else rustyAC's own | The player id to send. |
| `--offline` | | Never joins a server, whatever race.ini says. |
| `--no-steam` | | Does not ask Steam for your player id: rustyAC's own id is used (a random number made once, kept in `rustyac_net.ini`). |
| `--net-identity <file>` | `rustyac_net.ini` | Takes rustyAC's own player id from this file (a second player on one PC). |
| `--steam-achievements <on\|off>` | off | The game's Steam achievements, stats and Friends Leaderboard scores. They need Steam running. |
| `--chat <text>@<s>` | | Sends a chat line after this many seconds. Repeat it for more. |
| `--join-header <Name: value>` | | One more header of the WebSocket request. Repeat it for more. |
| `--tls-root <file.pem>` | | Trusts this certificate too for `wss://` (a gateway of your own). |
| `--net-tape <file>` | | Writes every message of the online session to this file at its end. Your player id and the password are blanked in it; names and chat lines are in it. **checks** |
| `--net-arrivals <csv>` | | Writes when every state of another car arrived, to measure delay. **checks** |

### AI and opponents

| Option | Default | What it does |
|---|---|---|
| `--ai-cars <n>` | | n cars in all: the others are copies of your car, each with Assetto Corsa's AI driver. Without a session in race.ini this is a race from the grid. |
| `--opponent <car>[:<skin>]:<ai\|parked\|auto\|file.ryin>` | | Another car: driven by the AI, parked, driven by the line follower, or by a recorded drive. Repeat it for more. Example: `--opponent bmw_z4_gt3:ai`. |
| `--ai-level <0..100>` | race.ini's, else 100 | How good the AI drivers are. |
| `--ai-aggression <0..100>` | race.ini's, else 0 | How aggressive they are. |
| `--ai-laps <n>` | race.ini's, else 5 | The race's laps as the AI drivers are told them (they fuel for them). |
| `--ai-drive` | | An AI driver drives your car from the start. While driving, Ctrl+C hands your car to one. |
| `--ai-at <s>` | | Hands your car to an AI driver after this many seconds. |
| `--no-race-cars` | | race.ini's other cars are left out. |
| `--stand-in-drivers` | | race.ini's other cars are driven by rustyAC's line follower, not the AI. **not AC** |
| `--focus <n>` | 0 | The car the camera is on at the start. 0: yours. |
| `--autodrive` | | On a track: a simple driver follows the track's AI line, with the automatic gearbox. For watching and for checks. Needs `--track`. **not AC** |
| `--cars <n>` | | n cars in all: the others are copies of your car driven by the line follower. **not AC** |
| `--drift-drive` | | On the drift track: a scripted driver that holds slides one way and the other. **checks** |
| `--drag-drive` | | A drag session as the game's test mode drives it: the AI drag driver drives your car too. **checks** |
| `--ai-seed <n>` | the clock | What the AI drivers' random numbers start from. **checks** |
| `--race-seed <n>` | the clock | What race.ini's draws start from (grip, wind, grid skins). **checks** |
| `--physics-threads <n>` | as the game: half the logical processors | Worker threads that step the cars. The result never depends on it. |

### Recording, replays and telemetry

| Option | Default | What it does |
|---|---|---|
| `--record <file>` | | Logs the inputs of every physics step into a file (`.ryin`). **not AC** |
| `--replay <file>` | | Drives from a recorded input file, or plays one of Assetto Corsa's own replays (`.acreplay`; the file names its track and cars). In the window the game's replay keys work: Ctrl+Space or P play and pause, Ctrl+S slow motion, Ctrl+F faster, Home back to the start. |
| `--replay-dir <dir>` | `replay` beside rustyac.exe | Where replays are saved (F10 in a replay). |
| `--save-replay <file>` | | Writes the session's replay to this file when the run ends. |
| `--no-ac-replay` | | Records no replay. |
| `--ghost <record\|play\|both\|off>` | race.ini's | The ghost car (the see-through car that drives your best lap again): keep the best lap, show the last one, both, or neither. Never on a server. |
| `--ghost-file <file>` | | Loads this `.ghost` file (one of the game's too). |
| `--ghost-advantage <s>` | 0 | The ghost runs this many seconds ahead. |
| `--ghost-dir <dir>` | `GhostCar` beside rustyac.exe | Where ghost cars are kept. |
| `--telemetry-dir <dir>` | `ctelemetry` beside rustyac.exe | Where your best lap's gear, speed and pedals are kept (a `.tc` file, as the game's comparative telemetry). Never on a server. |
| `--no-telemetry` | | Records no comparative telemetry. |
| `--no-shm` | | Does not publish the shared memory that telemetry apps read (`acpmf_physics`, `acpmf_graphics`, `acpmf_static`). |
| `--ai-record <dir>` | | Records your laps into an AI line, as the game's own tool does; the last lap is saved as `fast_lane.ai.candidate` in that folder. |

### VR, screens and input

VR headsets (OpenVR, Oculus), triple screens and TrackIR have no options of their own: rustyAC
follows Assetto Corsa's settings for them (`video.ini`, `oculus.ini`, `triple_screen.ini`), as the
game does. Set them in the launcher or in Content Manager.

| Option | Default | What it does |
|---|---|---|
| `--controls <ini>` | Assetto Corsa's `controls.ini` | The bindings file to use. |
| `--default-controls` | | Ignores controls.ini and uses the built-in Xbox pad and keyboard layout. |
| `--list-devices` | | Prints the detected devices and the active bindings, then stops. |
| `--ffb` | off | Force feedback to a DirectInput wheel, if one is found. |
| `--no-rumble` | | No rumble on the Xbox pad. |

### Where rustyAC keeps its files

| Option | Default | What it does |
|---|---|---|
| `--cfg-dir <folder>` | `cfg` beside rustyac.exe | Where rustyAC keeps its own settings. The folder above it holds the rest of rustyAC's own files (setups, replays, ghosts, telemetry, results, screenshots) unless their own options name other folders. |
| `--documents <folder>` | `Documents\Assetto Corsa` | That folder instead of Assetto Corsa's Documents folder (its cfg, setups, replays). The launcher names its own. |

### Checks and developer tools

All **checks**: for the project's tests and for developers.

| Option | Default | What it does |
|---|---|---|
| `--help`, `-h`, `/?` | | Prints the list of options. |
| `--headless` | | No window. With `--replay` it runs the file as fast as possible, otherwise in real time with nobody driving. |
| `--duration <s>` | | Stops by itself after this many seconds. |
| `--realtime` | | With `--replay --headless`: keeps to the clock and publishes shared memory. |
| `--dump-states <file>` | | With `--replay --headless`: writes the car's state after every step. |
| `--state-hash` | | With `--replay --headless`: prints a hash over every car's state of every step, and how long a step took. |
| `--drawn-frames <seed>` | | With `--headless`: Time Attack, Drift and Drag on drawn frames of uneven length. |
| `--bench-render` | | With `--headless`: also draws frames off screen and reports their rate. |
| `--bench-shot <png>` | | With `--headless --bench-render`: writes the last frame drawn to a PNG. |
| `--debug-view` | | A fast debug view that shows the physics, instead of Assetto Corsa's renderer. **not AC** |
| `--warp` | | Draws with Windows' software rasteriser: slow, but no graphics card is needed. |
| `--gpu-log <file>` | | Writes the Direct3D command log of the second frame. |
| `--render-seed <n>` | the clock while driving | What the picture's random numbers start from. |
| `--no-a2c` | | **not AC**, a diagnosis only: no alpha to coverage. Trees and fences turn into solid cards. |
| `--pose-out <file>` | | With `--screenshot`: also writes the car's state the picture is made from. |
| `--gui-do <commands>` | | Things done to the apps and menus before the first frame (clicks, keys, messages), `;` between them. |
| `--replay-script <list>` | | With `--replay` in a window: presses the replay's keys by the clock. Example: `--replay-script slow@2,pause@4,ffwd@5,stop@8`. |
| `--focus-script <list>` | | The camera's car keys by the clock. Example: `--focus-script next@2,previous@4,player@6`. |
| `--new-session-at <s>` | | In a window: a new session after this many seconds. |
| `--next-session-at <s>`, `--restart-session-at <s>` | | The referee's two buttons, pressed after this many seconds. |
| `--ai-pit-at <s>` | | Calls the AI driver of your car (`--ai-drive`) in for a pit stop after this many seconds. |
| `--pit-key <key>@<s>` | | A key of the pit menu pressed after this many seconds: `left`, `up`, `right`, `down` or `drive`. |
| `--pit-click <control>@<s>` | | A click in the old mouse pit menu after this many seconds: `tyre:2`, `fuel_up`, `confirm` ... |
| `--audio-oracle-camera <cockpit\|chase\|trackside>` | | The place the sound is heard from, as the sound test tool sets it. |
| `--audio-script <log>` | | Another run's answers to the sound engine's state questions. |

### Options that only work together

- `--supersample`, `--free-camera`, `--no-hud`, `--at`, `--lead-in`, `--flat-out`, `--show-lights`,
  `--show-damage` and `--pose-out` need `--screenshot`. `--flat-out-brake` needs `--flat-out`.
- `--layout` and `--autodrive` need `--track <name>`.
- `--state-hash`, `--dump-states` and `--realtime` need `--replay` and `--headless`.
- `--bench-render` and `--drawn-frames` need `--headless`; `--bench-shot` needs `--bench-render`.
- `--sound-tape` needs `--audio-wav`. `--replay-script` needs `--replay` in a window.
- `--record` with `--screenshot` needs `--autodrive` or `--flat-out` (somebody has to drive).
- `--hdr-levels` needs post-processing (`--post 1`). `--fxaa 1` and `--motion-blur` above 0 switch
  post-processing on by themselves.

## rustyac-launcher.exe

Assetto Corsa's launcher (its own pages, in a window), with rustyAC behind the Drive button.

| Option | Default | What it does |
|---|---|---|
| `--docs <folder>` | the launcher's own folder | The player's folder: where the launcher keeps its settings, setups and personal bests. |
| `--ac-root <folder>` | `AC_ROOT`, else Steam's install | Assetto Corsa's folder. |
| `--game <exe>` | `rustyac.exe` beside the launcher | The program Drive starts. |
| `--game-arg <arg>` | | One more option handed to the game. Repeat it for more. |
| `--player-cfg <folder\|none>` | `Documents\Assetto Corsa\cfg` | Where missing settings files are first copied from. Only read. |
| `--player-name <name>` | your profile's name, else race.ini's driver name | Your driver name. |
| `--import-pbs [<folder>]` | | Copies the personal bests of an Assetto Corsa Documents folder into the launcher's own where they beat yours, then stops. |
| `--log <file>` | | Writes the launcher's log to this file. |
| `--script <file>` | | Runs a list of clicks and checks instead of waiting for you. **checks** |
| `--offscreen <w>x<h>` | | With `--script`: no window on screen. **checks** |
| `--lobby <host:port>` | Kunos' lobby | Another server list for the online page. **checks** |
| `--transcript <requests> <out>` | | Answers a recorded list of the pages' requests into a file, without a window. **checks** |
| `--pure-ini` | | Reads and writes ini files with rustyAC's own code only. **checks** |
| `--fixed-host` | | With `--transcript`: fixed answers for what depends on the PC. **checks** |
| `--help`, `-h` | | Prints the list of options. |

## rustyac-gateway

A small server program that stands next to a Kunos acServer and carries the game's messages over
a WebSocket, for the browser page and for `rustyac.exe --join ws://...`. Every option with a value
can also be given as an environment variable: `GATEWAY_` and the option's name in capitals, with
underscores for the dashes (`GATEWAY_MAX_CLIENTS`). The command line wins. The log never holds a player id,
password, name or chat line.

| Option | Default | What it does |
|---|---|---|
| `--name <text>` | `gateway` | The name in the log lines. |
| `--listen <addr:port>` | `127.0.0.1:9601` | Where the gateway listens. |
| `--server <addr:port>` | `127.0.0.1:9600` | The acServer's TCP port. |
| `--udp-port <port>` | the TCP port | The acServer's UDP port. |
| `--origins <list>` | `http://127.0.0.1:*,http://localhost:*` | The sites whose pages may connect, with commas between them: `https://site.example`; `http://127.0.0.1:*` is any port; `*` is every site. A program that is not a browser is let in. |
| `--max-clients <n>` | 24 | The most players at once. |
| `--max-message <bytes>` | 4096 | The longest message taken (at least 64). |
| `--idle-timeout <s>` | 300 | A silent player is dropped after this many seconds. |
| `--ping-interval <s>` | 20 | How often the connection is pinged. |
| `--quiet` | | Fewer log lines. |
| `--check [addr:port]` | | Asks a running gateway whether it is well and stops (exit code 0 when it answers). |
| `--help`, `-h` | | Prints the list of options. |

## web_pack

Makes the pack of cars and tracks the browser page downloads, from your Assetto Corsa install
(`cargo run --release --manifest-path tools/web_pack/Cargo.toml -- <options>`). See
[web.md](web.md).

| Option | Default | What it does |
|---|---|---|
| `--config <file>` | `web/my_pack.toml` when it exists, else `web/preview.toml` | The list of cars and tracks to pack. |
| `--out <folder>` | `dist-web/preview` | Where the pack goes. |
| `--ac <folder>` | the install that is found | Assetto Corsa's folder. |
| `--dry-run` | | Checks the list and says what would be packed; writes nothing. |
| `--delta <manifest.json>` | | Writes only the files that are new or changed against that manifest of a live pack, plus the new manifest, into an empty `--out`. |
| `--help`, `-h` | | Prints the options. |

## The server kit

The folder `server/`: two Kunos acServer instances with a gateway each, and the server panel, as
Docker containers. See [server/README.md](../server/README.md).

`server/make_server_pack.py` makes the folder of cars and tracks the servers need:

| Option | Default | What it does |
|---|---|---|
| `--ac <folder>` | `AC_ROOT`, else the usual Steam path | Assetto Corsa's folder. |
| `--out <folder>` | `dist-server` | Where the pack goes. |
| `--server <name>` | every server of the kit | Only this server's cars and track. Repeat it for more. |
| `--windows` | | Also copies `acServer.exe`. |
| `--all` | | Every car and track layout that acServer and rustyAC can run. |
| `--car <name>` | | Adds this car. Repeat it for more. |
| `--track <name>` | | Adds this track, or `track/layout`. Repeat it for more. |
| `--no-check` | | Skips rustyAC's check of the pack. |

`server/docker-compose.yml` reads these variables (from the shell, or from a file `.env` beside it):

| Variable | Default | What it is |
|---|---|---|
| `AC_PACK_DIR` | `/srv/rustyac/dist-server` | The folder `make_server_pack.py` made. |
| `AC_ADMIN_PASSWORD` | none: set it | The servers' admin password. Never kept in git. |
| `AC_PASSWORD` | empty | The password players need. |
| `GATEWAY_SRC` | `../crates/rustyac-gateway` | Where the gateway's source is. |
| `GATEWAY_BIND` | `127.0.0.1` | Where the gateways listen. `0.0.0.0` to reach them from the network. |
| `GATEWAY_ORIGINS` | `https://rustyac.example.com` | The sites whose pages may connect. |
| `GATEWAY_MAX_CLIENTS` | 12 | The most players of a gateway. |
| `SPA_GAME_PORT`, `SPA_HTTP_PORT`, `SPA_GATEWAY_PORT` | 9600, 8081, 9601 | The Spa server's ports. |
| `LAGUNA_GAME_PORT`, `LAGUNA_HTTP_PORT`, `LAGUNA_GATEWAY_PORT` | 9610, 8082, 9611 | The Laguna Seca server's ports. |
| `PANEL_PORT`, `PANEL_ROUTER_PORT` | 9680, 9681 | The server panel's ports. |
| `PANEL_DATA_DIR` | `/srv/rustyac/panel` | Where the panel keeps its data. |
| `PANEL_SITE_URL`, `PANEL_GAME_HOST` | empty | The site's address and the name players reach the servers by. |
| `PANEL_MAX_SERVERS` | 3 | The most servers the panel starts at once. |
| `CF_ACCESS_TEAM_DOMAIN`, `CF_ACCESS_AUD` | empty | The Cloudflare Access team domain and application tag the panel checks sign-ins against (see the kit's README). |
| `PANEL_GAME_PORTS`, `PANEL_HTTP_PORTS`, `PANEL_GATEWAY_PORTS` | 9620-9629, 9630-9639, 9640-9649 | The port ranges of the panel's servers. |

A server's container also reads `AC_SERVER` (the kit folder, default `spa`), `AC_PACK` (default
`/pack`) and `AC_CFG` (its settings folder); the compose file sets them.

## The browser page

### In the address

Put them after the page's address: `?car=bmw_z4_gt3&track=ks_laguna_seca&go=1`.

| Parameter | Default | What it does |
|---|---|---|
| `?car=<folder>` | the page's choice | The car picked when the page opens. |
| `?track=<folder>` | the page's choice | The track. |
| `?layout=<name>` | the track's first | The track's layout. |
| `?go=1` | | Starts the drive without a click. |
| `?spawn=<hotlap\|pit\|start>` | the page's choice (`pit` online) | Where the car starts. |
| `?autodrive=1` | | A line follower drives. |
| `?backend=<full\|webgl>` | the page's setting | `full`: Assetto Corsa's own renderer (needs WebGPU). `webgl`: the debug picture on WebGL2. |
| `?graphics=<preset>` | the page's setting | `low`, `medium` or `high`, then single settings if you like: `?graphics=high,VIDEO/SHADOW_MAP_SIZE=1024`. It is kept. |
| `?tex=<px>` | 0: as stored | Smaller textures, for example `512`. |
| `?msaa=<n>` | 4 | Anti-aliasing samples of the debug picture. 1: none. |
| `?sound=0` | on | No sound. |
| `?launcher=0` | on | The plain page instead of Assetto Corsa's launcher. |
| `?pack=<folder>` | `preview/` | Another pack of cars and tracks on the site. |
| `?join=<server>` | | Joins this server of the site's list. |
| `?name=<text>` | the name you typed last, else `rustyAC driver` | Your driver name online. |
| `?password=<text>` | | The server's password. |
| `?mpcar=<folder>` | any free car | The car to ask the server for. |
| `?cars=<m>` | 0 | Online: up to this many metres away, other cars are drawn as models. |
| `?sessions=<list>` | | The sessions, as `--sessions` of rustyac.exe (`p10,q5,r3`). |
| `?set=<list>` | | Controls values for this visit only: `?set=X360.SPEED_SENSITIVITY=0,KEYBOARD.STEERING_SPEED=3`. |
| `?nocontrols=1` | | Does not use the kept Controls settings. |
| `?nocache=1` | | Keeps nothing of the pack in the browser's storage. |

For the project's tests (**checks**):

| Parameter | What it does |
|---|---|
| `?seed=<n>` | What the random draws start from (default: the clock). |
| `?selftest=<steps>`, `?keys=<list>`, `?hold=<list>` | Drives a number of steps by itself, with these keys pressed or held, and reports. |
| `?guido=<commands>` | Things done to the apps and menus first, as `--gui-do` of rustyac.exe. |
| `?fullproof=<n>`, `?fullgui=<commands>` | Draws fixed frames with Assetto Corsa's renderer for a comparison with the desktop program. |
| `?dragdrive=1` | The AI drag driver drives your car too. |
| `?soundtape=<n>` | Keeps a tape of the sound's inputs. |
| `?arrivals=1` | Records when every state of another car arrived. |
| `?server=<address>`, `?servercars=<list>`, `?servertrack=<name>`, `?serverpass=1` | A server named in the address instead of the site's list. Only on this computer. |
| `?ondemand=<address>` | The address of a server panel's router. Only on this computer. |
| `?testfolder=<path>` | The Assetto Corsa folder without the folder picker. |
| `?font=selawik` | The launcher's text in the Selawik font. |

### On the page

- **Where the cars and tracks come from**: the site's pack, or your own Assetto Corsa folder
  (Open folder; the browser asks for permission, nothing is uploaded).
- **Car, track, start place, automatic gearbox**, then **Drive**.
- **Settings** (the gear tile under Drive): Assetto Corsa's own Video, Audio, Profile and General
  pages. What a browser decides for itself (resolution, full screen, vertical sync) is greyed out.
- The **rustyAC** tab of Settings (during a drive: the gear at the bottom right, or O):
  - full screen;
  - sound volume and mute;
  - the picture: Full (Assetto Corsa's own renderer) or Debug;
  - the graphics preset (Low, Medium, High, Custom) and its single settings;
  - controls: load your own `controls.ini`, or reset; keyboard and pad sensitivity; the key list;
  - the downloaded data: what is kept in the browser, and buttons to clear it;
  - Copy diagnostics.
- **Multiplayer**: the server, the car, your name, the password, then Join.

## cfg\rustyac.ini

rustyAC's own settings file, in the folder of `--cfg-dir` (default: `cfg` beside rustyac.exe).
You make it yourself; one key is read.

| Key | Default | What it does |
|---|---|---|
| `[STEAM] ACHIEVEMENTS` | 0 | 1: the game's Steam achievements, stats and Friends Leaderboard scores are on. `--steam-achievements on` or `off` wins. |

Two more files of rustyAC's own sit beside the program and are not for editing by hand:
`rustyac_net.ini` (your own player id, made once) and `rustyac_controls.ini` (bindings saved by
rustyAC; `--controls` wins over it, and it wins over Assetto Corsa's controls.ini).

## Environment variables

| Variable | What it does |
|---|---|
| `AC_ROOT` | Assetto Corsa's folder, when it is not where Steam says. Read by rustyac.exe, the launcher and the tools. |
| `LIBOVR_DLL_DIR` | The folder of the Oculus runtime's `LibOVRRT64_1.dll`, when it is not found by itself. |
| `CF_ACCESS_HOST`, `CF_ACCESS_CLIENT_ID`, `CF_ACCESS_CLIENT_SECRET` | A Cloudflare Access service token for `--join wss://...`: sent only over `wss://` and only to that host. |
| `GATEWAY_NAME`, `GATEWAY_LISTEN`, `GATEWAY_SERVER`, `GATEWAY_UDP_PORT`, `GATEWAY_ORIGINS`, `GATEWAY_MAX_CLIENTS`, `GATEWAY_MAX_MESSAGE`, `GATEWAY_IDLE_TIMEOUT`, `GATEWAY_PING_INTERVAL` | The gateway's options (see "rustyac-gateway"). |
| `RUSTYAC_WGPU_ADAPTER` | In the builds that draw with wgpu: `gpu` or `swiftshader`. **checks** |
| `RUSTYAC_MATH` | Which maths functions are used. Default: the game's own (its `MSVCR120.dll`), or rustyAC's exact copy of them when the DLL is missing. `pure`: the copy. `no-dll`: as if the DLL were missing. `std`: the standard library's; results then differ from the game's in the last digits. **checks** |
| `RUSTYAC_PROOF_RUN` | Set by the proof scripts: rustyac.exe then refuses to start without `--cfg-dir` and `--documents`, and never asks Steam. **checks** |
| `RUSTYAC_AUDIO_VERBOSE` | More lines from the sound engine. **checks** |
| `RUSTYAC_DISPLAY_MODES` | The launcher's list of display sizes, fixed (`1920x1080,1280x720`). **checks** |
| `RUSTYAC_ROW_DUMP`, `RUSTYAC_ALSO` | What a replay that differs from its recording prints. **checks** |
| `RUSTYAC_FW1_GLYPHS` | A file to dump every drawn letter into. **checks** |
| `RENDER_ORACLE_TEXTURE_DATA` | 1: the graphics log hashes a texture again after each upload. **checks** |
| `RUSTYAC_DIAG_DRAW_ONE_CAR`, `RUSTYAC_DIAG_NO_CUBE_MIPS`, `RUSTYAC_DIAG_FINISH_SPIN` | 1: diagnosis switches of the renderer (only your car is drawn; no smaller copies of the reflections; wait for the frame without yielding). **not AC**, **checks** |

`USERPROFILE` and `USERNAME` are Windows' own: rustyAC reads them to find your Documents folder
and your default driver name.
