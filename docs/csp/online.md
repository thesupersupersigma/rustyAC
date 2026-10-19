# Online

> **Task 38c:** the `re/scratch/task…` paths below are history: those folders were scratch and
> most were deleted in the disk cleanups of 2026-10-15 and 2026-10-18. Every proof of this report
> reruns from `tools/proofs` (`sh tools/proofs/run_all.sh`); `docs/port/proofs.md` lists each path
> and what replaces it.

CSP does not open a second game connection and does not change AC's protocol ids. Everything it adds online rides on AC's own
TCP stream (`u16` length frame) and UDP datagrams, and on three side channels: the server's welcome text, chat messages, and the
server's HTTP port. A CSP client needs nothing from a vanilla `acServer`; a CSP-aware server (an `acServer` fronted by a
launcher / plugin that writes an "extended config", or a custom server) turns on extras one by one through a **feature list** and an
**extended INI config**. The client side is 34 hooks and 3 lookups on 9 acs.exe classes plus about 240 CSP functions (95 KB) in
the online module `dwrite.dll RVA 0x565360 .. 0x57d000`. Hook mechanism, gate records and the module-qualified names: `attach.md`
1.7 and 1.8 (not repeated). AC's own protocol: `docs/atlas/multiplayer.md`, `docs/port/multiplayer.md` (protocol version 202).

Corrections to earlier briefs, found while reading: (a) `attach.md` 2.2 lists `NetCarStateProvider 6/3`; `csp_hook_table.tsv` has **9
hooks and 0 lookups** for that class (the three lookups of the online group are `ACClient::getNetCarFromSessionID`,
`QOSOverlay::QOSOverlay`, `RaceControlVoting::voteSpamCheck`). (b) `ui_apps.md` 1.x says `ALLOW_APPS` / `ALLOW_TOOLS` are keys of the
server's extended config; they are keys of a **track's `surfaces.ini [_SCRIPTING_PHYSICS]`** (SDK text of every module's
`lib.lua`, `ac_physics_unrestricted.lua`; same block as `ALLOW_TRACK_SCRIPTS`, `ALLOW_DISPLAY_SCRIPTS`, `ALLOW_NEW_MODE_SCRIPTS`), not online.
(c) Evidence markers: `[dll]` seen in dwrite.dll (RVA / string), `[sdk]` Lua SDK text, `[lua]` shipped Lua, `[ini]` shipped config,
`[log-csp]` the owner's log, `[interp]` inference. **The owner's log has no online session** (only "Extension `Mumble Plugin`
disabled: available in online race only" and "App is not available: ...\Mumble"), so nothing here was seen on a live wire.

## 1. What it does

### 1.1 The channels CSP uses (summary)

| # | Channel | Direction | What rides on it | Evidence |
|---|---|---|---|---|
| 1 | **Welcome text** (TCP `0x51`, type 0) carrying a `$CSP0:` payload | server to client | the server's **extended config** (INI sections `[EXTRA_RULES]`, `[EXTRA_TWEAKS]`, `[CHAT]`, `[PHYSICS_EXPERIMENTS]`, `[CUSTOM_PHYSICS]`, `[MUMBLE_INTEGRATION]` ...), server Lua script entries, required CSP build | `$CSP0:` UTF-16 literal at RVA 0x2214ae8; base64 alphabet in the parser 0x566030 (RVA 0x22ad940); hook 0x56ace0 `[dll]` |
| 2 | **Feature list** `[REMOTE] __FEATURES` of `race.ini` (written by the launcher), or HTTP `GET /api/details` on the server's HTTP port when missing | server/launcher to client | which optional wire features the server speaks: `STEAM_TICKET`, `CUSTOM_UPDATE`, `WEATHERFX_V1/V2/V3`, `CLIENT_MESSAGES`, `CLIENT_UDP_MESSAGES`, `BATCH_FIX_V2`, `SPECTATING_AWARE`, `EXPLICIT_ADMIN_STATE`, `EMOJI`, `FREQUENT_TRACK_CHANGES` | strings 0x2215510, 0x2225640..0x2225830, 0x22a9f80; reader inside 0x1387350 (decomp_req1); fetcher 0x6d36d0 `[dll]` |
| 3 | **CSP envelope**: first byte `0xAB`, second byte = CSP message type (0..255), rest = payload; same envelope on TCP and UDP | both | online events of scripts, extra car data, car swap (type 7), anything a server plugin defines | `ACClient::onMessage` hook 0x571db0 and `onMessageTCP` hook 0x572e80: two 256-entry dispatch tables of 24-byte entries at `0x18268cf30` (UDP) and `0x18268e728` (TCP) `[dll]` |
| 4 | **Chat messages** with a `$CSP0:` prefix | both, through a vanilla server | the same script events, packed as text, for servers that do not speak channel 3 (SDK: under 175 bytes, at least 200 ms apart) | hooks `ACChat::onChatMessage` 0x564030, `FormChat::onChatMessage` 0x5dbc80 (consume), decoder 0x574460 (96 B) + size table 0x5744c0 `[dll]`; limits `[sdk]` |
| 5 | **Unused bits of `statusBytes`** in AC's own `0x46` car update (UDP, 62 bytes, `u32` at offset 0x33) | both | headlights / brake lights / horn / ERS / DRS / slipping wheels / turn lights / wipers / "extra CSP switches" | `UDPPacket::send` hook 0x112e7e0 (acts only when the packet size is 0x3e and writes at +0x33) `[dll]`; list of flags `[sdk]` `altonline.getCarStatusBytes` |
| 6 | **Handshake request `0x3d`**, protocol 0xCA | client to server | a Steam auth ticket when the server lists `STEAM_TICKET` | `UDPPacket::send` hook 0x112f030 (first byte `=` and `u16` 0xCA) calls 0x112e830; strings "Ticket is attached, size: " 0x228c188, "Failed to get ticket" 0x22b9e90, "Steam is not available" `[dll]` |
| 7 | **UDP mega packet `'H'`** (0x48) | server to client | with `BATCH_FIX_V2` CSP parses it itself (0x5709d0) instead of AC's code | hook 0x571db0, flag `0x1826950d8` `[dll]` |
| 8 | **Custom car update** (`CUSTOM_UPDATE`, server-defined layout) | both | server describes the packet in a small text DSL (`CUSTOM_UPDATE_FORMAT`); CSP sends/parses it | parser 0xd47d40 (5,436 B), strings "packet:", "single", "group", "padding", "skip", "half3", "short3", "byte3", ... RVA 0x226b188..0x226b670; sender 0x570440 runs from `onPhysicsStep` when flag `0x1826950d2` is set `[dll][interp]` |
| 9 | **HTTP(S)** to the server's HTTP port, and to third parties | client to server/web | `/api/details` (features, `loadingImageUrl`), `/INFO`; setups upload to `https://ss.acstuff.club/`; replay-clip upload endpoint from the server config; `remote_assets` cache for downloaded skins / models / animations | strings 0x2225780, 0x2225924, 0x2248cf0, 0x2215a00, 0x2282568 `[dll]` |
| 10 | **Steam GameNetworkingSockets** (`libgns.dll`) with CSP's own signaling service and a LAN beacon | peer to peer | only used by Lua (`shared/utils/luasteam`), e.g. the `p2p-1v1` new mode; **not** part of AC's server protocol | strings 0x21f6410..0x21f7000; `lua/new-modes/p2p-1v1/gns.lua` `[dll][lua]` |

### 1.2 How a CSP client learns what a server can do

1. **`race.ini [REMOTE]`.** Keys CSP reads: `ACTIVE`, `SERVER_IP`, `SERVER_PORT`, `SERVER_HTTP_PORT`, `SERVER_NAME`, `PASSWORD`, `REQUESTED_CAR`,
   `SLOT_INDEX`, `EXT_SLOT_INDEX`, `EXT_SKIN_ID`, **`__FEATURES`** (a list of strings) `[dll]` (strings 0x2215400..0x22155ac, 0x22257c8, 0x2225958,
   0x2248e38, 0x2249380, 0x226b8b0). `__FEATURES` is read once at start-up by function 0x1387350; it sets 12 flag bytes `0x1826950d0 .. 0x1826950dc`
   and logs "Server features (race.ini): ..." and "Server client messages: TCP=%d, UDP=%d".
2. **HTTP.** If `general.ini [CONFIGURATIONS] SYNC_SERVER_FEATURES` (hidden, shipped `1`) and the launcher gave no list, CSP asks the server
   itself: `http://<ip>:<http port>/api/details` (JSON with a `features` entry and `loadingImageUrl`; a second "NoHesi style" shape is
   accepted; error "Server features are missing in /api/details"); `/INFO` is read for the track ID ("Failed to sync track ID: ").
   The worker thread is named "CSP: syncing custom server features" `[dll]` 0x6d36d0, 0xd3f850, 0xd3fb10, 0xd404b0.
3. **Welcome message.** The original `ACClient::onWelcomeMessageReceived` runs first (prints the text); the hook then parses the text's
   `$CSP0:` payload (0x566030) and applies the config in one 13 KB function (0x566ac0, the body of hook 0x56ace0). A second
   path sets the whole config from a script: `altonline.configure` ("Extended online config is already set" is the error for a second set). Lua gets
   it through `ac.onOnlineWelcome(cb(message, config))` and `ac.INIConfig.onlineExtras()` `[sdk]`.
4. **Declared minimum build.** Features that change what clients must agree on are enabled only when the server requires a high enough
   CSP build: the global dword `0x18268cd08` is compared with 3265 (0xcc1) for custom motion and 3302 (0xce6) for `EQUALIZE_PITS_CHECK` /
   `AWAKE_IN_PITS`; `general.ini` says of `HIGHBEAMS_FLASHING`: "Online, requires a server requiring a build 3336 or above" `[dll][ini]`.
   The writer of that dword is not in the decompiled set `[interp]: the server's required CSP build`.

The 12 feature flags (all bytes at `0x1826950d0 + n`; names matched to flags by code order, so slots marked ? are `[interp]`):

| Flag | Feature (string RVA) | Used by | Evidence |
|---|---|---|---|
| d0 | "a feature list exists" | gate | 0x1387350 |
| d1 | `STEAM_TICKET` (0x2225640) | handshake hook 0x112f030 / 0x112e830; clears the "steamless" override `0x182695220` | `[dll]` |
| d2 | `CUSTOM_UPDATE` (0x22256f0) ? | `onPhysicsStep` calls 0x570440 | `[interp]` |
| d3, d4, d5 | `WEATHERFX_V1`, `_V2`, `_V3` (0x2225758, 0x2225748, 0x2225738) | WeatherFX online start (function 0x1319690: "Weather FX module is required on this server.", "Rain FX module is required...", "WeatherFX is required on this server.") | `[dll]` |
| d6 | `CLIENT_MESSAGES` (0x2225728), getter 0x1392810 | `ac.getSim().directMessagingAvailable` ("server supports TCP exchange of extra data between clients") | `[sdk][interp]` |
| d7 | `CLIENT_UDP_MESSAGES` (0x2225710), getter 0x1392880 | `directUDPMessagingAvailable` | `[sdk][interp]` |
| d8 | `BATCH_FIX_V2` (0x2225700) | UDP `'H'` branch of `ACClient::onMessage` | `[dll]` |
| d9 | `SPECTATING_AWARE` (0x2225768) ? | not traced | `[interp]` |
| da | `FREQUENT_TRACK_CHANGES` (0x22a9f80), getter 0x1392940 | not traced | `[dll]` |
| db | `EMOJI` (0x2225830) ? | `UDPPacket::writeString` hook: when 0 every code point above U+FFFF is written as '?'; **set to 1 when the list is missing** | `[dll]` |
| dc | `EXPLICIT_ADMIN_STATE` (0x2225818) ? | admin probing in the chat app | `[interp]` |

### 1.3 The server's extended config: sections and keys

Read by the hook 0x56ace0 / function 0x566ac0 (all of it `[dll]`; destinations are `dwrite.dll` data addresses, VA = base + RVA, base
0x180000000). Value syntax is the usual CSP INI (`;` comments, lists separated by commas). The physics keys of `[EXTRA_RULES]` that
another function (0x945a70, 0x945720) reads are in `physics_car_hooks.md` 6 and `physics_documented.md`; they are only listed in 1.3.5.

#### 1.3.1 `[EXTRA_RULES]` (client-side rules enforced by CSP's UI/physics)

| Key | Stored at | Effect |
|---|---|---|
| `ENFORCE_BACK_TO_PITS_PENALTY` | 0x1826950fc | bool; back-to-pits (teleport) costs a penalty |
| `ENFORCE_BACK_TO_PITS_STOP` | 0x1826950fd | bool; also read by `ESCMenu::setRaceControlScreen` hook (race control not opened while the car moves faster than 1 km/h) |
| `NO_BACK_TO_PITS` | 0x1826950f8 | bool; "Teleporting to pits: not allowed" |
| `NO_BACK_TO_PITS_OUTSIDE_OF_PITS` | 0x1826950f9 | bool; only the "in pits" variant allowed |
| `NO_BACK_TO_PITS_SESSIONS` | 0x1826950fa | list of letters `p`, `q`, `r` (practice / qualifying / race) as a 3-bit mask 1/2/4 |
| `ENFORCE_CUSTOM_INPUT_SCHEME` | 0x182695100 | list of allowed input methods; function 0x10ab7e0 shows "Chosen input method is not available on this server, only %ls allowed." / "Server is configured incorrectly, none of input methods are allowed." |
| `INVALIDATE_LAP_TIME_IN_PITS` | 0x1826950fe | bool |
| `HIDE_MAP` | 0x1826950ff | bool; "Track map: not allowed" |
| `MAX_METERS_WRONG_WAY` | `ACClient + 0x218` (float) | wrong-way allowance; AC's `ACClient::update` counts wrong-way infractions |
| `FORCE_NEW_PARTICLES` | (calls 0x14f1a00, 0x12a44c0) | force CSP's particle system on all clients |
| (other `[EXTRA_RULES]` keys) | rules block `0x1826a9690` | `PITS_SPEED_LIMITER`, `CHANGE_TYRES_PER_AXIS`, `SPEEDING_PENALTY`, `SPEEDING_SUBSEQUENT_PENALTY`, `LIMIT_LOCK_CONTROLS_TIME`, `LIMIT_LOCK_CONTROLS_TOTAL_TIME`, `REQUIRE_NEW_LAP_FOR_DRIVETHROUGH_PENALTY`, `UNFIT_FOR_DRIVETHROUGH_PENALTY`, `BACK_TO_PITS_PRESERVES_DAMAGE`, `EMERGENCY_RESET`, `KEEP_COLLISIONS`, `RETAIN_BRAKES_TEMPERATURE_IN_PITS`, `PITS_ORDER`, `DISABLE_AUTOBRAKING`, `SLIPSTREAM_MULT`, `SLIPSTREAM_GAIN_MULT`, `SLIPSTREAM_SPEED_FACTOR`, `ALLOW_WRONG_WAY`, `ALLOW_PITS_JUMP`, `USE_THROTTLE_TO_START` (strings 0x2247cf8..0x2248158, 0x2249de8, 0x224afc8, 0x224a6f0) |

#### 1.3.2 `[CHAT]`

| Key | Effect |
|---|---|
| `SERVER_MESSAGES_ONLY` | bool (flag `0x18268cc80`); the chat is read-only: "Chat is only for server messages." (0x2214cb0). **Fallback for vanilla servers:** the same flag is set when the welcome text itself contains that sentence |
| `MESSAGES_FILTER` | regular expression (flag `0x18268cc81`, object `0x18268cc88`) on incoming driver messages |
| `SERVER_MESSAGES_FILTER` | regular expression (flag `0x18268cc82`, `0x18268ccb0`) on server messages |
| `COMMANDS_NONADMIN_FILTER` | regular expression (flag `0x18268cc83`, `0x18268ccd8`); the `ACClient::sendChat` hook (0x56f1b0) tests every outgoing message against it and sets flag `0x18268cc84` for the chat UI; also "You are not admin on this server" is handled by 0x574150 |

#### 1.3.3 `[EXTRA_TWEAKS]` and `[PHYSICS_EXPERIMENTS]` (remote-car handling and a few physics options)

| Section | Key | Stored at | Effect |
|---|---|---|---|
| `[EXTRA_TWEAKS]` | `CUSTOM_MOTION` | 0x1826950df (list: bool, then optional words `SMOOTH` -> 0x1826950e0, `RESET` -> 0x18268b330) | CSP's own remote-car motion (see 1.5). **Default on** when the server requires build > 3265 and the key is absent |
| | `SHOW_DISCONNECTED` | 0x1826950e2 (value `WITH_COLLISIONS` = 2, else bool) | disconnected cars stay visible, optionally collidable |
| | `FORCE_HEADLIGHTS` | 0x1826950dd | sets bit 0 of the remote car's light flags (`+0x1240`) every step |
| | `LOWER_SEND_RATE_FOR_STATIONARY_CARS` | 0x182699586 | the local car's update interval is set to 500 ms while its speed is under 0.1 m/s (the `onPhysicsStep` hook writes `ACClient + 0x418`) |
| | `MIN_TIME_BETWEEN_COLLISIONS` | 0x1826950f0 (seconds x 1000, floor 0.05) | collision events of one car closer than this are dropped from the 5 s flush queue (message 0x82) |
| | `JUMP_LIMIT`, `JUMP_PAUSE_COLLISIONS_FOR` | 0x1826950e4 (default FLT_MAX), 0x1826950e8 (default 2.0) | only when custom motion is on: if a remote car's new position is farther than `JUMP_LIMIT` from the prediction, its collisions are paused for that many seconds |
| | `ACTUAL_TRACK_TIME` | 0x1826926e8 | server time drives the track clock |
| | `RESET_DISCONNECTED_NAMES` | clears 3 driver strings of every non-AI car record | |
| | `RESET_RECONNECTED_LAPTIMES` | arms a one-time handler (0x10af3c0) | |
| `[PHYSICS_EXPERIMENTS]` | `ISOLATE_CARS`, `WHEEL_CAR_CONTACTS` | 0x1826950a1, 0x1826950a6 | online collision options |
| (same, build > 3302) | `EQUALIZE_PITS_CHECK`, `AWAKE_IN_PITS` | 0x18268bcb2, 0x18269509e | |
| `[CUSTOM_PHYSICS]` | `NEW_MOTION_PREDICTION`, `YIELDING_COLLIDERS` | 0x18268b332, 0x18268b333 | which remote-car prediction / yield mode |
| | `RAW_TIMESTAMPS` or `EXTRAPOLATE_STATE=RAW` | 0x1826950e1 | use the sender's timestamps unfiltered (the `onRemoteStateReceived` hook then skips its own pacing) |
| | `SMOOTHING_FACTOR` | `ACClient + 0x128` (the named tunable `remoteFactor`) | |
| | `REAL_MASS` | 0x1825e1658 (float); applied to every car through 0x629e30 | real mass instead of the 1000 kg box |
| | `DISABLE_SURFACE_SNAPPING` | list of slot IDs; sets byte +0x38a of the car's physics avatar | |
| | `CUSTOM_COLLISIONS` | per-car sub-section parsed by 0x683eb0 (the same loader as `colliders.ini`) | |
| [section name obfuscated] | `EXTRA_CONTACT_POINTS` | calls 0x8c71d0 | extra lateral contact points |
| | `AFK_AUTOKICK` | list: minutes (x60, min 1 s), optional keyword `SPEED` | client-side AFK watchdog: "AFK Timeout", "Disconnecting in %.1f s", "Disconnecting due to being AFK" (function 0x56a260) |
| | `DISABLE_RAIN_PHYSICS` | clears byte +0xb6 of every per-car extension object | "Rain physics: disabled" |

#### 1.3.4 Sections known only from consumers outside the welcome parser

| Section / key | Where read | Effect |
|---|---|---|
| `[CUSTOM_COLOR] ALLOW_EVERYWHERE`, `ALLOW_IN_PITS` | 0x97fb20 (session information), `chat.extras.canChangeCarColor` `[sdk]` | car colour change, synced to all clients as `customCarColor` |
| `[EXTRA_DATA] CAMBER_TOE_STATE`, `TYRES_BLOWN_STATE`, `ENGINE_MAP_STATE`, `VR` | 0x97fb20 ("Camber and toe: synced"), RVAs 0x226b0d8, 0x226b108; `lua/new-modes/p2p-1v1` sets `CAMBER_TOE_STATE` and `VR` `[lua]` | extra car data synced between clients |
| `[REPLAY_CLIPS] UPLOAD_URL` | 0x97fb20 ("Endpoint for replay clips: configured"); string `__CLIPS_UPLOAD_URL` 0x22b6eb0 | where replay clips are uploaded |
| `[SURFACES_FX]` | 0x97fb20 ("Surfaces FX: enabled") | |
| `[TELEPORT_DESTINATIONS]` (`POINT_*`, `_GROUP`) | RVA 0x2248b78..; `chat.extras.teleportDestinations()` `[sdk]` | server-defined teleports (`ac.teleportToServerPoint`) |
| `[MUMBLE_INTEGRATION]` `HOST`, `PORT`(64738), `PASSWORD`, `CHANNEL`('Root'), `POSITIONAL_AUDIO`, `POSITIONAL_AUDIO_DIRECT_PTT`, `POSITIONAL_MAX_DISTANCE`(50), `MUTE_DISTANCE`, `USERNAME_PREFIX`; macros `{ServerIP}`, `{ServerName}`, `{ServerPortHTTP/TCP/UDP}` | `internal/lua-apps/Mumble/Mumble.lua` `[lua]` | server-set voice chat; no `HOST` unloads the app |
| `[PORTALS]`, `CUSTOM_UPDATE_FORMAT` | module `ac_ext_online_plus` ("online_plus.ini", RVA 0x226b0f8); 0xd404b0, 0xd47d40 | portals to other servers, custom update layout |
| `VERIFY_RAIN_FX`, `CHECKSUM`, `REQUIRED_VERSION`, `_VERIFY_INTEGRITY` | zip comment (attach.md 1.9); track `surfaces.ini` loader 0x94dcd0 | integrity rules; the key name `VERIFY_RAIN_FX` is **not a plain string** of the image |

#### 1.3.5 Obfuscated part (stop point)

About 20 literals in the welcome handler are XOR/rotate-obfuscated at run time (the same trick as `attach.md` 1.9): one section name, three
key names (the first is guarded by the global byte `0x1826b8a18`), and a block of about a dozen names/messages around a loop
that walks a list of entries, reads sub-keys per entry and queues a closure (0x57c1a0, 0x30 bytes) on the main-thread queue; it ends in
objects of the class `ac_ext::server_lua_script` (RTTI 0x26229d8) and the thread name "CSP: updating server script" (0x226b8d0). That block is
the **server script loader**, and its key names are what an operator writes to make clients run a server Lua script. They guard
script download and execution; per project rule 2 they were not decoded. What a script can do once loaded is documented in the SDK (1.8).

### 1.4 What is added on the wire (details)

* **Type 7 of the `0xAB` envelope (TCP): car swap.** Payload `[u8 version = 0]` `[u8 slot id]` `[u8 n][n bytes car id]`
  `[u16 m][m bytes skin id]`. Unsupported version raises "Unsupported packet version"; the car id may not contain `/` or `\`
  ("Invalid car ID"); the skin is checked ("Invalid skin ID", "Invalid skin source") and loaded from `content/cars/<car>/skins/<skin>`
  or from the `remote_assets` cache; a per-slot counter at `0x1826a34b0` makes the newest request win; errors are logged
  "Failed to read car swap packet: ". Handler 0x572600 (1,478 B). Used by `altonline.driverConnected(carID, ...)` and by servers that rotate
  cars `[dll]`. Every other type is dispatched to whatever handler a CSP module (or a Lua `ac.OnlineEvent`) registered; types with no handler are
  dropped silently on UDP. Fixed payload sizes are checked ("Size for type N does not match (expected X bytes, got Y bytes)", function
  0x5744c0).
* **`$CSP0:` in chat.** The decoder is 0x574460 (called first by both chat hooks; if it returns true the original is not called, so the
  text never reaches the chat list). Prefix is UTF-16 `$CSP0:` (RVA 0x2214ae8). A 62-character alphabet function (0x56cae0, RVA 0x22150c8)
  is called at the end of `handshakeTCP` with `ACClient + 0x100`; its role (a random token, e.g. for the "server seed") is not read `[interp]`.
  Limits from the SDK: under 175 bytes per message, 200 ms spacing; the Lua side calls this "special chat messages".
* **Status bits.** The patch is applied to every 62-byte outgoing `0x46` datagram. The bits (AC's own: 0x20/0x40/0x100/0x200 per
  `docs/atlas/multiplayer.md`; the rest are CSP's) include headlights, high beams, brake lights, horn, ERS, DRS, wheel slipping,
  turn indicators, wipers, extra switches; consecutive packets alternate some of the extra bits ("odd and even calls return different values")
  `[sdk][dll]`. Function 0x112e450 (899 B).
* **`'H'` mega packet.** `BATCH_FIX_V2` replaces AC's parser for the datagram `u32 server time, u16 ping, u8 count, count x 57-byte
  states` (layout in `crates/rustyac-net/src/datagram.rs`) by 0x5709d0 (1,370 B). What AC's parser gets wrong is not read `[dll][interp]`.
* **Strings.** `UDPPacket::readStringW` (0x112f080) and `writeString` (0x112f1d0) are fully replaced: wide strings are
  `u8 count + count x u32 code points` as in AC, but surrogate pairs are handled (astral characters, i.e. emoji, round-trip), the
  count is capped at 255, and a protocol overrun raises "Protocol error" (RVA 0x228c150) instead of reading garbage.
* **Send rate.** `LOWER_SEND_RATE_FOR_STATIONARY_CARS` (see above); `ACCOUNT_FOR_PING=1` ("shift car inputs online to account for ping",
  `general.ini [REPLAY]`) and `QOS` (below) are local.
* **TCP receive gate.** `TCPSocket::receivePacket` hook (0x56cf60): when byte `0x182699587` is non-zero the call returns an empty packet
  without reading the socket; its writer is not in the decompiled set (looks like a debug / "network paused" switch) `[dll][interp]`.
* **Voting.** `ACClient::sendVote` hook (0x56f2f0) stores `{vote yes/no, "no vote is running" = 0x100}` in `0x1826c8bf8` before the original
  runs; `ACClientVotingManager::update` hook (0x56f320) does nothing unless a vote runs (timer `*(client+0x400)+0x68 > 0`), and then feeds the
  keys of the two controls `__CM_ONLINE_POLL_YES` / `__CM_ONLINE_POLL_NO` (default Y / N; manifest `CUSTOM_VOTING_BINDINGS=1`) into AC's manager as a
  2-bit mask `0x1826a3785`; a lock "Voting request is locked for {:.1f} s" (RVA 0x2220c68, function 0x699ef8) rate-limits requests;
  `ac.castVote`, `ac.canCastVote`, `ac.getCurrentVoteDetails`, `ac.timeToNextVote` expose it `[dll][sdk]`.
* **Session start time.** `ACClient::onNewSession` hook (0x573750) copies the new session's start stamp (`ACClient + 0x10520`) into the
  sessions table (entry +0x30, entry size 0x40) and fires the new-session listeners `(session index, "same session")`.

### 1.5 Remote cars (`NetCarStateProvider`)

CSP keeps AC's idea (`docs/atlas/multiplayer.md`: dead reckoning from the last three states) and adds:

* **State queue.** The constructor hook (0x625920) lets the original run with a thread-local "constructing" flag, then grows the state
  vector (`+0x160..0x170`, 0x48-byte entries) to 2,048 entries.
* **Timing statistic.** `onRemoteStateReceived` hook (0x62aff0): a guard (0x62aaf0, not read) can drop a state; a per-car average of inter-arrival
  time (`0x1826a4fe0[slot]`, factor 0.1, reset on gaps above 1 s or more than 10 lost sequence numbers) is kept; offline (alt-online) or with
  `RAW_TIMESTAMPS` the original is called with that average written into the provider (`+0x418`).
* **Motion.** With `CUSTOM_MOTION` off the original `step` runs; with it on, `step` (0x6296d0) is replaced: connection transitions (60-step grace, 120-step
  timer), velocity / angular-velocity corrections capped at 5 m and 3 rad of drift per step, collision resume after a jump, `FORCE_HEADLIGHTS`,
  and a custom stepper 0x627490 (3,576 B). `getSmoothBodyMatrix` (0x628e80) scales the smoothing by 2.5 x the timing statistic (clamped to
  `[1, remoteFactor]`, low-passed 5 %/frame), and below 40 km/h blends back to the previous matrix when the car is nearly still (no wobble
  of stationary cars). `getPhysicsState` (0x6292f0) keeps per-slot CSP data (object of 0x428 bytes, table `0x1826c7860`), fixes the 127 degree steer
  cap (`FIX_REMOTE_STEER_ANGLE`: rescales when the stored angle is exactly +/-127), and derives local velocity from successive positions.
  `stepLagging` (0x629680) and `setCarAvatar` (0x62aa30, applies `REAL_MASS` and custom collisions) complete it; `initWings` (0x62aa90) sets a
  thread-local car index while AC loads the wings so per-car CSP wing data is found.
* **Disconnected cars.** `update` (0x6257a0) implements `SHOW_DISCONNECTED` (keep visible; optionally keep the collision body awake).
* The same provider serves the **fake online** cars of new modes (`ALT_ONLINE_CARS`, `shared/sim/altonline`, `submitState`, `packCarState1`
  of 36 bytes), which is why the hooks test a global "online" byte (`0x18268b39f`, written by the `RaceManager::initOnline` hook 0x10edae0).

### 1.6 Rules, integrity and enforcement

* **Checksum join path** (`ACClient::onMessageTCP` hook 0x572e80, TCP `0x68` kick): after the original handles the message, if the stored
  reason equals "Checksum failed. You have been kicked out of the race" (53 characters) CSP walks the list of files the server had asked to
  hash. If the list contains exactly `system/data/surfaces.ini` and the 64-bit hash of the local file differs from the constant of the stock
  file (0xAEA304FE4647FE6F; hash function 0x15c5c80, unread), it ends with a fatal message "Failed to join: system surfaces configuration
  (system/data/surfaces.ini) doesn't match the server." with hints; otherwise "Failed to join: one of the files doesn't match the server:" plus the
  list. Both offer `steam://validate/244210` ("Would you like to run validation now?") `[dll]`.
* **Pre-join file checks** (function 0x56cfa0, 1,260 B): unpacked data of a Kunos car ("Reinstall the car"), unpacked data of a mod
  ("Look for packed data file: it might have been renamed", "Use Content Manager to pack the data"), a missing file from the server's list.
  `ACClient::getCarMD5` hook (0x56b250) returns an all-zero 8-byte digest for the car `nul` (spectator / non-car slot) and calls the original
  for every other car `[dll]`.
* **Track `surfaces.ini [_EXTENSION] _VERIFY_INTEGRITY`** (`CHECKSUM`, `REQUIRED_VERSION`, `_FROM`: "Required file ... is modified or damaged",
  function 0x94dcd0, `physics_documented.md` 2.x), custom-physics module loader integrity (0x15c9db0), **verified RainFX** (server-side rule
  forces the 101 embedded RainFX configs, `attach.md` 1.9), car "packed flags" with a version byte, size and checksum ("Invalid packed flags
  (checksum failed)", "car tweaks version v%d", "Penalty message: verification failed", function 0xee10d0, 5,971 B). The ordinary
  AC mod-integrity items are AC's.
* **Client signing for server-side verification:** `ac.verifyIntegrity` (18-byte fingerprint of `:self`, `:ac`, `:car`, `:track`, files),
  `ac.signBlob` (RSA, 384-byte signature; message includes `sha256(data)`, a hash of server IP;TCP port;Steam ID, the slot, the server seed),
  `ac.getUserSteamID` (online scripts only), `shared/utils/signing.lua` header substitutions `{ServerIP}`, `{ServerTCPPort}`, `{ServerSeed}`,
  `{VerifiedSteamID}`, `{CSPVersion}`, `{CSPVersionCode}` `[sdk][lua]`. This is how a server **plugin** can require "client runs CSP and this
  script"; it is a script-level API, not a protocol field.
* **Experiments off online.** Every `[PHYSICS_EXPERIMENTS*]` option of the client is ignored in an online session ("Experimental
  features are not available online", RVA 0x22aa228) except those the server turns on (1.3.3).

### 1.7 Chat, admin, shortcuts, setups

* **CSP's chat app** replaces AC's `FormChat` (`APP_AC_CHAT=1`): attachments, emoticons (`data_emoji.txt`), driver tags
  (`chat_driver_tags.ini`, CM `Friends.data`), car colour picker (`car_colors.ini`), teleport list, setup sharing (setups uploaded to
  `ss.acstuff.club`, link sent in chat; `shared_setup.ini`), admin panel. Hooks `ACChat::render` (0x564440, replaced: only records the
  rectangle `0x1826a2c1c..2c28`), `FormChat::FormChat` (0x5dbbf0), `FormChat::onChatMessage` (0x5dbc80), `RaceControlScreen::render` (0x6997e0, extra
  "Assists" and "Chat" panels).
* **Admin.** `/admin <password>` is detected in `ACClient::sendChat` (0x56f1b0; sets byte `0x18268a7ab`). Sign-in probes: "Checking if admin
  privileges are available", "Trying to signing in, response: ", "You are now Admin for this server", "Admin management is broken: response=`...`".
  The same chat commands AC's server understands are offered: `/next_session`, `/restart_session`, `/kick_id`, `/ban_id`, `/ballast`,
  `/restrictor`; `/client_list` is AC's local one. Passwords may be saved (`online_passwords.ini`, section `AC_CHAT`). `EXPLICIT_ADMIN_STATE`
  makes probing unnecessary; `sim.isAdmin` and `ac.checkAdminPrivileges()` `[sdk]`.
* **Chat shortcuts** (`chat_shortcuts.ini`, `ENABLED=0` by default): `[MESSAGES] MSG_1..MSG_0` (Hi, Bye, Sorry, My fault, Thanks, No problem,
  GG, Coming through, Gotta go to pits, Be careful please) while the `__EXT_CHAT_SHORTCUTS_BASE` key is held (`Oemtilde`),
  `[EXTRA_MESSAGES] MSG_EXTRA_1..0` (empty) while Alt is added; they go out through `ACClient::sendChat` `[ini]`.
* **Other chat extras:** TTS for incoming chat (`small_tweaks.ini USE_TTS_FOR_CHAT`), driver name colour (`ac.setDriverChatNameColor`),
  `ui.onChat`, `ui.mentionDriverInChat`, `ac.onChatMessage` / `ac.onOutgoingChatMessage` (return true to swallow), `ui.registerOnlineExtra`
  (adds an entry to the chat app's tool list; flags `Admin`, `Tool`).
* **Welcome message:** `ac.onOnlineWelcome`; the chat app can show or hide AC's original welcome (`show_welcome_message`,
  `hide_original_welcome_message`), system messages, server messages, voting messages (`hide_*` keys of the `AC_CHAT` ImGui state).

### 1.8 Lua: events, shared data and server scripts

* **Online events** (`ac.OnlineEvent(layout, callback[, namespace, udp, params])`, 3 module kinds can use `udp`): typed structs exchanged between the
  same script on different clients. With a CSP-aware server (`directMessagingAvailable`) they go in the `0xAB` envelope (TCP; UDP with
  `{range = metres}`, the server forwards only to cars within range); with a vanilla server they go as `$CSP0:` chat text (175 bytes, 200 ms).
  Target: `-1` everyone, a session ID, or `255` = the server (a plugin picks it up; builds before 2506 ignore targets). `repeatForNewConnections` re-sends
  for late joiners (TCP only). Car scripts talk only to car scripts, track scripts to track scripts (`ac.SharedNamespace`) `[sdk]`.
  Real use: the shipped `lua/cars/android_auto/apps/messenger/service.lua` (`messenger:message`, `messenger:readFlag`) `[lua]`.
* **`ac.connect`, `ac.broadcastSharedEvent`, `ac.onSharedEvent`, `ac.store/load` are local** (shared memory between scripts of one CSP process);
  nothing of them goes on the wire `[sdk]`.
* **Online script kind** (`ac_online_script`, SDK 1,947 functions of which 1,152 own, 108 classes, `rules.json` path `extension/lua/online/*`,
  namespace `server_script`): supplied by the server, runs on every client with `update`, `draw3D`, `drawUI`, `frameBegin`. Besides the common API
  it has: `ac.getUserSteamID`, `ac.signBlob`, `ac.reconnectTo{serverIP, serverPort, serverHttpPort, serverName, serverPassword, carID,
  trackID, trackLayoutID}` (the only kind that can send the player to another server), `ui.registerOnlineExtra`, `shared/utils/steam`
  (the old Steam wrapper; "online scripts can't use luasteam"), the `physics.*` API (107 functions, subject to the track's
  `[_SCRIPTING_PHYSICS]`), `ac.setDriverName/Team`, flag overrides (`ac.PenaltyType`, "physics-altering scripts like server scripts can override
  flag type") `[sdk]`. Not available to it: fake mouse events. No shipped online script exists (the folder is absent).
* **Alt-online** (`shared/sim/altonline.lua`, new-mode manifest `[TWEAKS] ALT_ONLINE_CARS`, `ALT_ONLINE_CAR_ID`, `ALT_ONLINE_CHAT`,
  `VERIFY_STEAM_API`): 29 functions to drive fake remote cars with real online machinery: `driverConnected`, `driverDisconnected`,
  `submitState`, `setDamage`, `setTyres`, `setPing`, `chatMessageFrom`, `serverChatMessage`, `getCarStatusBytes`, `setPitIndex`,
  `configureTimeReference` (NTP: `time.windows.com`, `time.cloudflare.com`, `time.apple.com`), `packCarState1` (36 bytes), `packLaps`,
  `onLapCompleted`, `blockPause`, `blockSessions`, `configure` (set the extended config), `onSendingExtraMessage` / `extraMessageFrom`
  ("Originally, CSP would use special chat messages") `[lua][sdk]`.
* **Weather controllers** `base-live` and `base-schedule` (`extension/weather-controllers`) are launcher-driven (race.ini
  `__CM_WEATHER_CONTROLLER`), not online features; online WeatherFX uses the server's sun angle (hook 0x56ae00), server timezone (keys
  `TIMEZONE`, `TIMEZONE_ID`, "Server requires an unknown timezone"), and the `WEATHERFX_Vn` features.

### 1.9 Voice, QoS, adapters, local servers

* **Mumble.** (a) `mumble_plugin.ini ENABLED=1`: module `ac_ext_mumble_plugin` writes the **MumbleLink** shared memory ("MumbleLink",
  name "Assetto Corsa", description "Assetto Corsa with CSP adding Mumble support.") for positional audio; disabled offline ("available in online
  race only"). (b) The **Mumble app** (`internal/lua-apps/Mumble`, `LAZY=0`, `FLAGS=DISABLED_OFFLINE`; `gui.ini APP_MUMBLE=1`) starts
  `internal/plugins/AcTools.MumbleClient.exe` (its own Mumble client: libopus, librnnoise, libspeexdsp), connects with plugin context
  `base64("Assetto Corsa\0" + "<ip>:<tcp port>")` and identity = session ID, uses `[MUMBLE_INTEGRATION]` of the server config (1.3.4) and
  can run a "PTT-aware" radio mode. `small_tweaks.ini [MUMBLE] AUTOCONNECT=1` (connect to linked servers), `AUTOCLOSE=1`.
  `mumble://` and `ts3server://` links in the welcome text are clickable (RVA 0x224c2f8, 0x224c308) `[dll][lua][ini]`.
* **QoS.** `general.ini [QOS_TWEAKS]` (hidden): `BASE_TOS_FLAG` (0), `QWAVE_CFG` (0), `QWAVE_TYPE` (5), `QWAVE_NON_ADAPTIVE` (1),
  `QWAVE_DSCP_VALUE` (46); log lines "[QOS] ..."; `qwave.dll` functions `QOSCreateHandle`, `QOSAddSocketToFlow`, `QOSSetFlow`, ... ; the
  `AC::socket_configure_qos::_QOS_TRAFFIC_TYPE` lookup. `QOSOverlay::renderHUD` hook (0x696850) hides AC's ping text when CSP's own ping HUD
  (`gui.ini HIDE_PING_TEXT`, `altonline.setPing`) is active (`0x1826a6ee0`) `[dll][ini]`.
* **Adapter binding** (`general.ini ADAPTER_NAME`, env `ACCSP_PREFERRED_ADAPTER_IP`): function 0x4977f0 lists adapters ("List of available
  adapters", "Found a fitting adapter"); `WSOCK32 bind` (0x498e10) and `connect` (0x498f90) detours bind outgoing sockets to the chosen
  address ("Socket is bound to target IP", "Failed to bind socket to target IP"); `LOOPBACK_SERVERS` (0x498bd0) reads
  `general.ini` ("AC starts two servers, one for extra commands and another for telemetry ... With this checkbox, they would become loopback
  servers") `[dll][ini]`.
* **AC's own UDP services** (telemetry 9996, command listener 9666; `docs/atlas/multiplayer.md`): the constructors
  `RemoteTelemetryUDP` (0x6ba340) and `UDPCommandListener` (0x6ba410) are **replaced by a base-class constructor only**
  (object names "RT_UDP" and "UDPCOMMAND_LISTENER"), `RemoteTelemetryUDP::update` is the shared no-op stub 0x2db1f0, and
  `UDPCommandListener::update` (0x6ba4e0) calls the original only when a flag read through 0xef1700 is zero, bracketed by byte `0x1826a2c46`
  (also tested by `ESCMenu::setRaceControlScreen`). So the original servers are probably never started by these constructors, which sits
  badly with the `LOOPBACK_SERVERS` text; the gate that decides is not decompiled `[dll][interp]` (open question 4).
* **P2P (not the AC protocol).** `libgns.dll` + CSP signaling ("CSP: GNS signaling", `P2P_CSP_Signaling_Service`, `P2P_CSP_LAN_Beacon_Port`),
  `steam.GNS.configure`, `connectP2PCustomSignaling`; used by `p2p-1v1`.
* **Portals.** Track-side portal objects (`accPortalBase_*.fxo`, "Use throttle and brake pedals together to teleport to a different
  server") use `/api/details` of the destination ("Failed to teleport to a different server: couldn't get server information") and the same
  reconnect path as `ac.reconnectTo` `[dll]` (0xd3f2e0, 0xd3ff86, 0xd404b0, 0xd50060).

### 1.10 Compatibility

| Pair | Result | Evidence |
|---|---|---|
| CSP client + vanilla `acServer` (no `__FEATURES`, plain welcome) | Handshake is AC's (`0x3d`, 0xCA, no ticket). All feature flags are 0 except `db` which is 1. Online events fall back to `$CSP0:` chat. CSP's wire-affecting tweaks stay off (they need a declared required build); client-only fixes stay on (UTF-32 surrogates, TCP/UDP QoS, remote-car smoothing of the original model, name/skin handling). Experimental physics blocked | `[dll][ini]` |
| CSP client + server that publishes only an extended config | Rules and tweaks of 1.3 apply; transport is unchanged | `[dll]` |
| CSP client + server speaking `CLIENT_MESSAGES` / `CLIENT_UDP_MESSAGES` / `BATCH_FIX_V2` / `CUSTOM_UPDATE` / `STEAM_TICKET` | channels 3, 7, 8, 6 of 1.1 | `[dll][sdk]` |
| vanilla `acs.exe` + CSP-aware server | Not decided by the client: AC ignores `0xAB`-type extras it does not know only if its dispatcher treats unknown ids as unknown (`ACClient::onMessage` default branch); the welcome text with the payload is shown as text. A server that requires CSP must test it itself (signing, `ac.signBlob`, online script) | `[interp]` |
| rustyAC today (`crates/rustyac-net`) | Unknown TCP ids stay whole and unacted; unknown datagram ids are ignored (`Incoming::Unknown`); the welcome text goes to the console; no `0xAB`, no ticket, no `'H'`-fix | `datagram.rs`, `messages.rs`, `docs/port/multiplayer.md` |

## 2. Game parts it replaces or extends

Category letters as in `physics_car_hooks.md`: A = pass-through that records state / fires CSP events, B = wraps the original and
changes inputs or outputs, C = replaces (original not called, or only conditionally), D = lookup. Slot RVA in brackets.

### 2.1 The online group (34 hooks, 3 lookups)

| Symbol | acs.exe VA | CSP RVA (slot) | Cat | What it changes |
|---|---|---|---|---|
| `ACClient::ACClient` | 0x14003a2c0 | 0x565cc0 (0x268a898) | A/B | constructor goes through helper 0x565cf0 (473 B); the object is then registered by 0x575140 (singleton `0x1826a2c30`) `[interp]` |
| `ACClient::getNetCarFromSessionID` | 0x140046ee0 | lookup (0x268a890) | D | used by chat and `ac.getCar.serverSlot` |
| `ACClient::onWelcomeMessageReceived` | 0x140050460 | 0x56ace0 (0x268a888) | B | original first, then `$CSP0:` parse and the whole config of 1.3 (13,254 B, function 0x566ac0) |
| `ACClient::renderHUD` | 0x140051650 | 0x56ad10 (0x268a880) | B | swaps the HUD message string `ACClient+0x10560` with CSP's (`0x1825e1488`) while the original draws |
| `ACClient::onRemoteSunAngleReceived` | 0x14004fef0 | 0x56ae00 (0x268a878) | B/C | listener list (`0x18268cd58`, reverse order) may swallow the event, else original (WeatherFX takes the server's sun) |
| `ACClient::getCarMD5` | 0x140046070 | 0x56b250 (0x26a2c48) | B | car `nul`: 8 zero bytes; else original |
| `TCPSocket::receivePacket` | 0x14025e530 | 0x56cf60 (0x26a2c78) | B | returns an empty packet while byte `0x182699587` is set |
| `ACClient::handshakeTCP` | 0x140048b50 | 0x56d490 (0x268a870) | B | wraps the original in an error translator: loading text "Exchanging handshakes"; failure "Failed to connect: handshake failed." with hints (firewall, internet, LAN ports); fires the handshake listeners (server name, ports); then wipes the copied digest vectors (`+0x3d8..0x3e0`) |
| `ACClient::sendChat` | 0x140052a90 | 0x56f1b0 (0x268a868) | B | `/admin ` detection, `COMMANDS_NONADMIN_FILTER` test, then original |
| `ACClient::sendVote` | 0x140052ba0 | 0x56f2f0 (0x26a2c80) | B | records vote bit and "no vote running" (0x100) in `0x1826c8bf8`, tail-calls original |
| `ACClientVotingManager::update` | 0x140055600 | 0x56f320 (0x26a2c88) | B/C | skipped when no vote runs; feeds the `__CM_ONLINE_POLL_YES/NO` keys |
| `ACClient::onPhysicsStep` | 0x14004f1d0 | 0x5706e0 (0x26a3498) | B | stationary send-rate (500 ms), collision-queue throttle (`MIN_TIME_BETWEEN_COLLISIONS`), custom update sender (0x570440) |
| `ACClient::onMessage` (UDP) | 0x14004b9d0 | 0x571db0 (0x26a34a0) | B/C | stamps arrival time (`0x18268ff28`); `0xAB` goes to the UDP table; `'H'` with `BATCH_FIX_V2` goes to 0x5709d0; else original |
| `ACClient::onMessageTCP` | 0x14004c840 | 0x572e80 (0x26a36b0) | B/C | `0xAB` goes to the TCP table (type 7 = car swap); other messages go to the original, then the checksum-kick analysis |
| `ACClient::onNewSession` | 0x14004e4a0 | 0x573750 (0x26a36c0) | A/B | see 1.4 |
| `NetCarStateProvider::update` | 0x14011d950 | 0x6257a0 (0x26a4fd0) | B | `SHOW_DISCONNECTED` |
| `NetCarStateProvider::NetCarStateProvider` | 0x140116c70 | 0x625920 (0x26a4fd8) | B | state vector to 2,048 entries |
| `NetCarStateProvider::getSmoothBodyMatrix` | 0x140119310 | 0x628e80 (0x26a57e0) | B/C | smoothing scaled by arrival jitter; low-speed hold |
| `NetCarStateProvider::getPhysicsState` | 0x140119220 | 0x6292f0 (0x268b348) | B | per-slot CSP object; steer cap fix; velocity from positions |
| `NetCarStateProvider::stepLagging` | 0x14011cf60 | 0x629680 (0x26a6bf0) | B | copies CSP's lag state when `CUSTOM_MOTION` |
| `NetCarStateProvider::step` | 0x14011bd00 | 0x6296d0 (0x268b340) | B/C | original only with custom motion off |
| `NetCarStateProvider::setCarAvatar` | 0x14011bc80 | 0x62aa30 (0x26a6bf8) | B | `REAL_MASS`, custom collisions |
| `NetCarStateProvider::initWings` | 0x14011aa80 | 0x62aa90 (0x268b338) | B | thread-local car index during load |
| `NetCarStateProvider::onRemoteStateReceived` | 0x14011b1e0 | 0x62aff0 (0x26a6c00) | B | jitter statistic, optional own pacing |
| `ACChat::onChatMessage` | 0x140035c00 | 0x564030 (0x268a860) | B/C | `$CSP0:` consumer (0x574460) before the original |
| `ACChat::render` | 0x140036380 | 0x564440 (0x268a858) | C | original not called (CSP's ImGui chat replaces it); records the window rectangle |
| `QOSOverlay::renderHUD` | 0x1400ac5d0 | 0x696850 (0x268b4a0) | B | skipped when CSP's ping HUD is active (`0x1826a6ee0`) |
| `QOSOverlay::QOSOverlay` | 0x1400ac470 | lookup (0x268b4a8) | D | |
| `RaceControlVoting::voteSpamCheck` | 0x1400ba480 | lookup (0x268b4d8) | D | called by CSP's vote lock (0x699ef8) |
| `RemoteTelemetryUDP::RemoteTelemetryUDP` | 0x1401468e0 | 0x6ba340 (0x26a8580) | C | calls `GameObject::GameObject` with "RT_UDP" only |
| `RemoteTelemetryUDP::update` | 0x140147660 | 0x2db1f0 (0x26a8588) | C | shared no-op stub |
| `UDPCommandListener::UDPCommandListener` | 0x1401d0f30 | 0x6ba410 (0x26a8590) | C | base constructor with "UDPCOMMAND_LISTENER" only |
| `UDPCommandListener::update` | 0x1401d1570 | 0x6ba4e0 (0x268b7f8) | B | original only if flag from 0xef1700 is zero, inside byte `0x1826a2c46` |
| `UDPPacket::send` #1 | 0x14025db70 | 0x112e7e0 (0x26ad190) | B | 62-byte `0x46`: writes extra status bits at +0x33 (0x112e450) |
| `UDPPacket::send` #2 | 0x14025db70 | 0x112f030 (0x26ad198) | B | handshake `0x3d`/0xCA: attaches the Steam ticket (0x112e830, 2,012 B) |
| `UDPPacket::readStringW` | 0x14025da90 | 0x112f080 (0x26ad1a0) | C | surrogate-safe, "Protocol error" |
| `UDPPacket::writeString` | 0x14025dca0 | 0x112f1d0 (0x26ad1a8) | C | surrogate-safe, `EMOJI` gate, 255 cap |

### 2.2 Other hooks that touch online play (not in the online group)

| Symbol | acs.exe VA | CSP RVA | Cat | Online role |
|---|---|---|---|---|
| `RaceManager::initOnline` | 0x14013d160 | 0x10edae0 | B | sets `0x18268b39f` (online), fires listeners, original, then 0xed8a20 / 0x10efb40 (online init) |
| `FormChat::FormChat`, `FormChat::onChatMessage` | 0x140308190, 0x14030bd90 | 0x5dbbf0, 0x5dbc80 | C, B | AC's chat form replaced, incoming text offered to 0x574150 |
| `RaceControlScreen::render` | 0x1400b29c0 | 0x6997e0 | B | extra panels |
| `ESCMenu::setRaceControlScreen` | 0x1400ff2b0 | 0x5ce310 | B | not while moving with `ENFORCE_BACK_TO_PITS_STOP` |
| `CarAvatar::setDriverInfo` | 0x1400da0b0 | 0x10ad5e0 | A/B | listeners may swallow (driver swap / name override) |
| `SplashScreen::setServerHints`, `setOnlinePlayerCar` | 0x1401a17e0, 0x1401a17a0 | 0x1116460, 0x1116520 | B | message pump while loading |
| `Car::addPenalty` | 0x14026f6a0 | 0x59dd40 | A | marks penalties |
| `FlagManager::checkforPenalty` | 0x14009c9b0 | 0x5cf180 | A | event after the original |
| `NetMonitorForm::NetMonitorForm` (and 7 other dev forms) | 0x140303400 | 0x305980 | C | stub returns 0: the net monitor is not built |
| `SteamInterface::*` (4 hooks) | 0x1401accd0 ... | 0x6d5660, 0x6f5100, 0x6f51c0, 0x6f5210 | B | gate Steam stats / achievements; **not** auth |
| `WSOCK32 bind / connect` | (import) | 0x498e10, 0x498f90 | B | adapter binding (1.9) |

### 2.3 CSP functions of the online module without a hook (by size; read from strings only)

| RVA | Size | Role |
|---|---|---|
| 0x566030 | 1,723 | welcome payload decode (base64 alphabet) `[interp]` |
| 0x566ac0 | 13,254 | extended-config applier (hook 0x56ace0) |
| 0x56a260 | 586 | AFK watchdog and messages |
| 0x56a800, 0x56cfa0 | 876, 1,260 | validation offer; pre-join file checks |
| 0x56b6d0 | 5,124 | not identified (one string "false") |
| 0x56d860 | 3,821 | slot choice: "Select first car slot", "Try different server"; `SLOT_INDEX` / `EXT_SLOT_INDEX` handling; a placeholder "[skipping_a_slot]" |
| 0x56e820, 0x56eac0, 0x56ed50 | 661, 645, 352 | `[REMOTE] REQUESTED_CAR` / `REMOTE` handling |
| 0x56fe60 | 1,492 | "Enough time has passed, alerting welcome message listeners" |
| 0x570440 | 672 | custom update sender |
| 0x5709d0 | 1,370 | `'H'` parser |
| 0x5710d0, 0x5718c0, 0x5719e0 | 275 each | message readers ("Unexpected end") |
| 0x571460, 0x571b00 | 1,084, 301 | other CSP message handlers |
| 0x572320 | 656 | `remote_assets` cache folder |
| 0x572600 | 1,478 | car/skin swap |
| 0x572bd0 | 501 | URL builder `"%s?%s"` |
| 0x574150 | 775 | chat interception ("You are not admin on this server") |
| 0x574460 / 0x5744c0 | 96 / 334 | `$CSP0:` decoder / size table |
| 0x575a70 | 442 | `SLOT_INDEX` |
| 0x57ca30 | - | Lua native-call reply helper ("Nothing to reply to") |
| 0x6ce4d0, 0x6cf640 | 370, 4,238 | connection failure reporters ("SERVER AUTH FAILED:", "Failed to connect: %ls", "Make sure Steam is running correctly") |
| 0x6d36d0 | - | `/api/details` fetch |
| 0xd47d40, 0xd46260, 0xd46eb0, 0xd49400 | 5,436, 2,621, 1,974, 343 | custom update format DSL |
| 0xd404b0, 0xd50060 | 4,808, 4,228 | portals; reconnect / `ac.reconnectTo` |
| 0x97fb20 | 6,174 | "Session information" panel of the main menu: lists every active server rule |

## 3. Config keys and Lua API

**User side (shipped files, `[ini]`).**

| File | Key (default) | Meaning |
|---|---|---|
| `general.ini [CONFIGURATIONS]` | `BLOCK_UNTRUSTED_NETWORKING` (0) | car / track scripts cannot use the web |
| | `ADAPTER_NAME` (empty), `LOOPBACK_SERVERS` (0) | outgoing adapter; AC's two local UDP servers on loopback |
| | `SYNC_SERVER_FEATURES` (1, hidden) | fetch features over HTTP |
| `general.ini [EXPERIMENTS]` | `HIGHBEAMS_FLASHING` (1) | needs server build >= 3336 online |
| | `ADAPTIVE_REMOTE_FACTOR` (0) | adaptive smoothing for remote cars |
| | `EXTRAPOLATE_STATE` (4: more precise for remote cars) | |
| `general.ini [ONLINE]` (hidden, commented) | `FIX_REMOTE_STEER_ANGLE` (1), `CUSTOM_MOTION` (0) | local defaults for the two server-side tweaks |
| `general.ini [REPLAY]` | `ALLOW_REPLAYS_ONLINE` (1), `ACCOUNT_FOR_PING` (1), `CLIP_DURATION` (30) | instant replays online; input shift for ping; clip length |
| `general.ini [QOS_TWEAKS]` | `BASE_TOS_FLAG` (0), `QWAVE_CFG` (0), `QWAVE_TYPE` (5), `QWAVE_NON_ADAPTIVE` (1), `QWAVE_DSCP_VALUE` (46) | UDP priority |
| `general.ini` | `PREFER_DISABLED_ABS2`, `PREFER_DISABLED_TC2` (0) | keep ABS / TC off even if the server forces them |
| `gui.ini` | `APP_AC_CHAT` (1), `APP_MUMBLE` (1), `HIDE_PING_TEXT` (0), `APP_AC_CHAT_EMOJI2` | chat and voice apps |
| `small_tweaks.ini [MUMBLE]` | `AUTOCONNECT` (1), `AUTOCLOSE` (1) | |
| `small_tweaks.ini` | `USE_TTS_FOR_CHAT` (0), `TTS_ENGINE` (0), `[CUSTOM_DRIVER_MODELS] LOAD_REMOTE` (SIMFRIENDLY), `DRIVER_NAMES_ON_LICENSE_PLATES_ONLINE` (0), `ALLOW_SCENE_ADJUSTMENTS_ONLINE` | |
| `mumble_plugin.ini [BASIC]` | `ENABLED` (1) | MumbleLink |
| `chat_shortcuts.ini` | `ENABLED` (0), `MSG_1..0`, `MSG_EXTRA_1..0` | 20 canned messages |
| `data_manifest.ini [FEATURES]` | `ONLINE_EXTRA_FEATURES=1`, `CSP_TEST_ONLINE=1`, `CUSTOM_VOTING_BINDINGS=1`, `NUL_CAR=1` | what CSP tells Content Manager it supports (manifest; "does not affect behavior") |
| `new_behaviour.ini` | (header) "all of those things won't affect online" | AI flood etc. never online |

**Server side:** sections and keys in 1.3. **race.ini:** 1.2.

**Lua API (counts from `lua_sdk_functions_union.txt`, 2,729 names in all):** 43 online-related names: server info 5
(`ac.getServerIP/Name/PortHTTP/PortTCP/PortUDP`), client events 3 (`ac.onClientConnected`, `onClientDisconnected`, `ac.onOnlineWelcome`),
config 1 (`ac.INIConfig.onlineExtras`), online events 1 (`ac.OnlineEvent`), local sharing 5 (`ac.connect`, `ac.broadcastSharedEvent`,
`ac.onSharedEvent`, `ac.store`, `ac.load`), chat 6 (`ac.sendChatMessage`, `onChatMessage`, `onOutgoingChatMessage`, `getUnreadChatMessages`,
`setDriverChatNameColor`, `ui.onChat`), `ui.mentionDriverInChat`, `ui.registerOnlineExtra`, votes and teleports 6 (`ac.castVote`,
`canCastVote`, `getCurrentVoteDetails`, `timeToNextVote`, `teleportToServerPoint`, `canTeleportToServerPoint`), admin / identity 5
(`ac.checkAdminPrivileges`, `checkIfServerRequiresPasswordAsync`, `getUserSteamID`, `signBlob`, `verifyIntegrity`), `ac.uniqueMachineKey`,
`ac.reconnectTo`, slot iteration 2 (`ac.getCar.serverSlot`, `ac.iterateCars.serverSlots`), loading `loading.serverHints`,
physics names 4 (`physics.setDriverName/Team`, `setAIDriverName/Team`: "announce the new name online"), `ac.onLuaScriptDisposal`.
Availability by SDK module: `ac.OnlineEvent`, `ac.connect`, `ac.onOnlineWelcome`, server info, `ac.INIConfig.onlineExtras`,
`ac.onClientConnected/Disconnected`, shared events: all 19 modules (18 for `OnlineEvent`: not `ac_splashscreen`); chat send/receive and
`checkAdminPrivileges`: `ac_apps`, `ac_new_modes`, `ac_online_script`, `ac_tools`; votes and teleports: `ac_apps` only; `reconnectTo`:
`ac_online_script` only; `signBlob`, `getUserSteamID`, `ui.registerOnlineExtra`: `ac_new_modes`, `ac_online_script`. Properties
`ac.StateSim`: `isOnlineRace`, `isAltOnlineRace`, `directMessagingAvailable`, `directUDPMessagingAvailable`, `isAdmin`, `pitsSpeedLimit`,
`isPitsSpeedLimiterForced`, `timeSinceLastUDPPacket`, `handshakeServerTime`, `currentServerTime`, `currentSessionTime`, `randomSeed` (synced),
`resultScreenTime`, `raceOverTime`, `connectedCars`; `ac.StateCar`: `sessionID`, `isRemote`, `isConnected`, `isHidingLabels`, `ping`,
`customCarColor`, `physicsAvailable` `[sdk]`. Shared Lua libraries: `shared/sim/altonline`, `shared/sim/chat`
(`extras.canChangeCarColor/changeCarColor/teleportDestinations/teleportTo/shareCarSetup`), `shared/utils/signing`, `shared/utils/steam`,
`shared/utils/luasteam` (GNS), `shared/utils/ntp`, `shared/utils/parcel` `[lua]`.

## 4. Files it reads

* `Documents\Assetto Corsa\cfg\race.ini` (`[REMOTE]` keys above, `__FEATURES`); `cfg\extension\*.ini` user overrides of the keys of section 3;
  `online_passwords.ini` (admin passwords, section `AC_CHAT`), `chat_driver_tags.ini`, `car_colors.ini`, `shared_setup.ini` (the chat app's own state under
  the CSP state folder); `AcTools Content Manager\Progress\Friends.data` (driver tags) `[dll]`.
* The local files that the server asks to be hashed: `content/cars/<car>/data.acd` (or `data/`), `system/data/surfaces.ini` and any path
  from the handshake reply; `content/cars/<car>/skins/<skin>`; `remote_assets` cache (skins, `remote_models`, `remote_animations`).
* Embedded: 101 verified RainFX configs (zip #3) for `VERIFY_RAIN_FX`; `data_emoji.txt`, `data_countries.ini`.
* Network (outgoing): server TCP/UDP port; `http://<ip>:<http port>/api/details`, `/INFO`; `https://ss.acstuff.club/` (setups); replay-clip
  upload URL from the config; Mumble server from the config; NTP hosts for alt-online; the signaling service of `P2P_CSP_Signaling_Service`.

## 5. How it is switched on

* No master switch of its own: the hooks exist whenever CSP is loaded (`general.ini [BASIC] ENABLED=1`). Per session the decision is
  made by **what the server says**: feature flags (1.2), the extended config (1.3), the required build (`0x18268cd08`). A session is "online" when
  `RaceManager::initOnline` runs (byte `0x18268b39f`).
* User switches: `APP_AC_CHAT`, `APP_MUMBLE`, `chat_shortcuts.ini ENABLED`, `mumble_plugin.ini ENABLED`, `SYNC_SERVER_FEATURES`,
  `ADAPTER_NAME`, `LOOPBACK_SERVERS`, QoS keys, `ALLOW_REPLAYS_ONLINE`, `ACCOUNT_FOR_PING`.
* Defaults on this PC (owner's config): chat app on, Mumble plugin enabled (loaded only online), chat shortcuts off, no
  `cfg/extension` override of the online keys was read; `ENABLED=1`.

## 6. Size

| Item | Number |
|---|---|
| Hooks / lookups of the group | 34 hooks (33 distinct replacement RVAs, one shared no-op stub 0x2db1f0), 3 lookups, 9 acs.exe classes (ACClient 12+1, ACChat 2, NetCarStateProvider 9, TCPSocket 1, UDPPacket 4, ACClientVotingManager 1, QOSOverlay 1+1, RemoteTelemetryUDP 2, UDPCommandListener 2, RaceControlVoting 0+1) |
| Pseudo-C read | 118,138 bytes (all 34 hook bodies; the big ones: 0x56ace0 46,855 B, 0x572e80 11,007 B, 0x6296d0 8,516 B, 0x628e80 7,196 B) |
| Online module code | 239 functions, 95,073 bytes in `0x565360 .. 0x57d000`; remote-car CSP code about 79 functions, 27 KB at `0x625900 .. 0x62c500`; online-plus (portals, custom format, reconnect) about 117 functions, 80 KB at `0xd3f000 .. 0xd52000` |
| Server config | 6 documented sections (`EXTRA_RULES`, `CHAT`, `EXTRA_TWEAKS`, `PHYSICS_EXPERIMENTS`, `CUSTOM_PHYSICS`, `MUMBLE_INTEGRATION`) + 5 consumer-side ones (`CUSTOM_COLOR`, `EXTRA_DATA`, `REPLAY_CLIPS`, `SURFACES_FX`, `TELEPORT_DESTINATIONS`) + 1 obfuscated section; about 75 key names (1.3) |
| Feature list | 12 flag bytes, 11 feature names found |
| Strings | 148 in the ACClient/chat pool 0x2214a00..0x2215f00; chat/admin app strings 0x2248700..0x224b900 |
| Lua | 43 online names; `ac_online_script` 1,947 functions; `altonline` 29 functions; 1 shipped plain online user (`android_auto` messenger), 1 voice app (1,730 lines) |

## 7. Port difficulty for rustyAC

**Staying joinable: M. Full parity: XL.** `crates/rustyac-net` already speaks AC's protocol 202 byte for byte and was proven bit-exact
against `acs.exe` (`docs/port/multiplayer.md`: 20 runs, 1.13 million steps); every CSP addition is optional and gated by a flag, so a client
that does nothing keeps working on any server that does not *require* CSP.

| # | Addition | Needed to... | Difficulty | Reuse | How to prove / what to capture |
|---|---|---|---|---|---|
| 1 | Strip and parse the `$CSP0:` payload of the welcome text (`WelcomeMessage` in `messages.rs`) | not print a blob; read server rules | S | `WelcomeMessage`, INI reader of `rustyac-content` | capture one `0x51` frame from a CM-style server; expected: base64 (and maybe compressed) INI text; run it through the decoder and compare with the server's source file |
| 2 | `__FEATURES` and `/api/details` | know which extras the server speaks | S | `join.rs` (race.ini `[REMOTE]`), `rustyac-gateway` HTTP | log `Server features (race.ini)` of a real CSP client; `GET /api/details` of the same server |
| 3 | Steam ticket in the handshake (feature `STEAM_TICKET`) | join servers that authenticate | M | `identity.rs` (loads the game's `steam_api64.dll`, id only) | capture the `0x3d` frame from CSP vs vanilla against the same server: the bytes after the password string; needs 0x112e830 decompile; a ticket needs `ISteamUser::GetAuthSessionTicket` |
| 4 | `0xAB` envelope receive (ignore, then dispatch) | not choke on server plugins | S | `messages.rs` keeps unknown ids whole | capture both transports while a CSP client joins an AssettoServer-class server; log the `0xAB` types and lengths |
| 5 | `'H'` `BATCH_FIX_V2` | read cars from a server that uses it | M | `datagram.rs` mega-packet parser | capture a few `'H'` datagrams with and without the fix flag; compare with CSP's 0x5709d0 (decompile) |
| 6 | Status bits (lights, horn, DRS ...) of `0x46`, in and out | show remote lights; be seen by CSP clients | S | `car_update.rs` has the u32 | capture CSP-client `0x46` datagrams (62 bytes, offset 0x33) with lights on/off; compare the u32 |
| 7 | `UDPPacket` strings with surrogates | emoji names and chat | S | `packet.rs` | send "x" + U+1F600 from a CSP client; expect 2 code points of 4 bytes, no '?' |
| 8 | Own remote-car motion (`CUSTOM_MOTION`, `SMOOTHING_FACTOR`, jitter statistic, `JUMP_LIMIT`) | match what CSP players see of each other | M-L | `net_car.rs` (AC's model, bit-exact) | tapes of a CSP client against a server (the `.nettape` flow of `car_oracle net`); but acs.exe + CSP in one process is untested (attach.md Q6), so compare against the CSP log / screenshots, not a bit-exact oracle |
| 9 | Server rules of 1.3.1-1.3.3 (back-to-pits, input scheme, chat filters, AFK, collision throttle, send-rate) | respect a server's rules | M (each S) | `online.rs`, `race_rules.md` machinery | one scenario per key against a local copy of `acServer.exe` (the AC `server/` folder) with a hand-made welcome payload |
| 10 | Car swap type 7, skin/car loading from `remote_assets` | follow servers that rotate cars | M | `content` loaders | capture type-7 messages; compare slot/car/skin |
| 11 | `CUSTOM_UPDATE` DSL (parser + sender/receiver) | join servers that replaced the car update | L | none | decompiles 0xd47d40, 0x570440; capture of the first custom datagram; a model server to produce the layout |
| 12 | `$CSP0:` chat events and Lua `OnlineEvent` | scripts online | L-XL | needs a Lua host (`lua.md`) | out of scope until Lua exists |
| 13 | Server Lua script loader (obfuscated keys) | scripts pushed by a server | XL | none | not decoded by rule; blocked unless the project owner decides otherwise |
| 14 | Mumble / voice | voice | L | none | separate: MumbleLink writer is S (a struct in shared memory), the client app is XL; both optional |
| 15 | QoS / adapter binding | latency tuning | S | `native.rs` sockets | `[QOS]` lines; local only |

What cannot be proven offline: the server side is outside `acs.exe`. The cheapest proof set is **a local `acServer.exe`** (the AC
`server/` folder holds one; the `re/scratch/task25` kit already runs copies) plus a CSP client on this PC with (a) the stock config,
(b) a welcome message carrying a hand-made `$CSP0:` payload (take the INI of 1.3 with one key at a time, observe `Server features`,
`[QOS]`, "Weather FX" and the new log lines in `custom_shaders_patch.log`), and (c) `STEAM_TICKET` listed in `__FEATURES`.
Capture TCP and UDP with the loopback capture of a packet sniffer (the repository's `--net-tape` of rustyAC can record a *rustyAC*
session, not a CSP one) and replay them through `car_oracle net` and `rustyac-net`'s decoders.

## 8. Open questions and not mapped

1. **Welcome payload grammar.** `$CSP0:` prefix and base64 are seen; whether the decoded bytes are compressed, signed, or split in chunks
   is not (0x566030 not decompiled). Leading whitespace in the text is unknown.
2. **Which flag is which.** Slots d2, d9, db, dc (1.2) are matched by code order and by one behavioural test (`writeString` -> `db`).
3. **What `CLIENT_MESSAGES` carries.** Boolean or message-type byte; the log prints a number per direction.
4. **AC's UDP telemetry and command servers** are constructed by a base-only constructor in this build; whether CSP starts them elsewhere
   (setting `LOOPBACK_SERVERS`) is not determined. The gate for the hooks is not decoded.
5. **Handshake ticket layout** and whether the CSP version is sent to the server (no plain string says so; `{CSPVersion}` exists only in
   script signing).
6. **Obfuscated keys** (1.3.5): the server script loader and one required-build rule; not decoded by rule.
7. **`TCPSocket::receivePacket` gate byte** `0x182699587`: writer unknown.
8. **The server's required build** (`0x18268cd08`): writer unknown, so the exact set of builds that unlock each tweak is only the two
   thresholds seen (3265, 3302) plus the documented 3336.
9. **`FUN_1815c5c80`** (a 64-bit hash with a side effect, used on the surfaces file, on the car digest and on short strings).
10. **Replay clips upload**, `ss.acstuff.club` protocol, setup share format: strings only.
11. **Key `VERIFY_RAIN_FX`** is not a plain string; where it is read is not found.
12. **Third-party servers.** Behaviour of AssettoServer-style plugins is assumed from the SDK text, not verified.

## Evidence

* `re/scratch/csp1/decomp_hooks/{564030,564440,565cc0,56ace0,56ad10,56ae00,56b250,56cf60,56d490,56f1b0,56f2f0,56f320,5706e0,571db0,572e80,573750,
  6257a0,625920,628e80,6292f0,629680,6296d0,62aa30,62aa90,62aff0,696850,6ba340,6ba410,6ba4e0,2db1f0,112e7e0,112f030,112f080,112f1d0,
  5dbbf0,5dbc80,305980,10edae0,10ad5e0,6d5660,6f5100,6f51c0,6f5210}.c`; `decomp_req1/1387350.c` (race.ini features, lines 2130-2380).
* `dll_strings.tsv` ranges 0x2214a00-0x2215f00, 0x2225400-0x2225a00, 0x2248700-0x224c500, 0x2269800-0x226b900, 0x2208800-0x2208cb8,
  0x21f1f00-0x21f2700, 0x21f6300-0x21f7000, 0x22a6280-0x22a6800, 0x2279600-0x2279c00; `dll_func_strings.tsv` for 0x566ac0, 0x56d490, 0x56cfa0,
  0x5744c0, 0x97fb20, 0x4977f0, 0xd47d40, 0xd404b0, 0x1319690, 0x10ab7e0, 0xee10d0, 0x112e830; `dll_funcs.tsv` sizes.
* `docs/atlas/multiplayer.md`, `docs/port/multiplayer.md`, `crates/rustyac-net/src/{messages,datagram,handshake}.rs`.
* SDK: `extension/internal/lua-sdk/ac_online_script/lib.lua` (online events, welcome, chat, admin, signing, reconnect), `rules.json`,
  `extension/internal/lua-shared/sim/{altonline,chat}.lua`, `utils/{signing,steam,luasteam}.lua`, `lua-apps/Mumble/*`,
  `lua/new-modes/p2p-1v1/*`, `lua/cars/android_auto/apps/messenger/service.lua`; configs `general.ini`, `small_tweaks.ini`, `gui.ini`,
  `chat_shortcuts.ini`, `mumble_plugin.ini`, `data_manifest.ini`; the owner's `custom_shaders_patch.log`.
* Obfuscated literals (not decoded, rule 2): welcome handler 0x56ace0 / 0x566ac0 and 1387350's `[REMOTE]` neighbourhood.

## Decompile requests

1. 0x566030 - welcome payload: prefix handling, base64, compression?, INI parse entry; exact grammar and the writer of `0x18268cd08`.
2. 0x574460 - `$CSP0:` chat decoder: alphabet, framing, checksum.
3. 0x5744c0 - per-type size table ("Size for type").
4. 0x574150 - chat interception for admin replies and `SYNTAX` probing.
5. 0x112e450 - which status bits CSP sets and how odd/even packets alternate.
6. 0x112e830 - where and how the Steam ticket is attached to the handshake.
7. 0x5709d0 - the `'H'` parser (what `BATCH_FIX_V2` fixes).
8. 0x570440 - `CUSTOM_UPDATE` sender.
9. 0xd47d40, 0xd46260, 0xd46eb0, 0xd49400 - custom update format DSL and rule application.
10. 0x1392770, 0x13927c0, 0x1392810, 0x1392880, 0x13928f0, 0x1392940, 0x13929b0, 0x1392a00 - the eight feature getters (names, parameters).
11. 0x5710d0, 0x5718c0, 0x5719e0, 0x571460, 0x571b00 - CSP message readers and their type ids.
12. 0x572600, 0x572320, 0x572bd0 - car/skin swap, `remote_assets`, URL builder.
13. 0x56d860, 0x56e820, 0x56eac0, 0x56ed50, 0x575a70 - slot and requested-car handling.
14. 0x56fe60 - welcome listeners dispatcher (timing).
15. 0x56cae0 - the 62-character-alphabet function called at the end of the handshake.
16. 0x498e10, 0x498f90, 0x4977f0, 0x498bd0 - bind / connect detours, adapter choice, loopback rule; 0x499120, 0x499210 - QoS.
17. 0x6ba340 / 0x6ba410 gate: function 0xef1700 and who sets its byte; why the constructors are base-only.
18. 0x62aaf0, 0x627490, 0x628b60, 0x629e30 - remote-car guard, custom step, custom smoothing, real mass / collisions.
19. 0x8c71d0 - what `EXTRA_CONTACT_POINTS` online switches.
20. 0x6cf640, 0x6ce4d0 - connection failure reporters.
21. 0x6d36d0, 0xd3f850, 0xd3fb10 - `/api/details` and `/INFO` clients (JSON shape).
22. 0x97fb20 - the rules panel; section of every key of 1.3.4.
23. 0xee10d0 - packed flags / car tweaks / verification messages.
24. 0x15c5c80 - the hash function.
25. 0x56b6d0 - unidentified 5 KB function in the online module.
