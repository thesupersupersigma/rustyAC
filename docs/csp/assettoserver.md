# AssettoServer: what rustyAC needs to join it (with AI traffic)

Task NH (research only: no code, no builds, no servers run, no network). Goal: rustyAC with CSP on,
playing on **our own, unmodified** AssettoServer with AI traffic, No Hesi style. No Hesi's own servers
and their anti-cheat are a later step.

* **Source read:** `ignored/AssettoServer/` (a clone of `compujuckel/AssettoServer`), `git describe --tags`
  = **`v0.0.55-pre49`** (commit `563a17f`, 2026-11-14). Older stable compared: **`v0.0.54`** (`51737e2`,
  2023-12-21); 319 commits lie between the two. All paths below are relative to `ignored/AssettoServer/`
  unless they start with `docs/` or `crates/`. Line numbers are pre49's.
* **Client side cross-checked with** `docs/csp/online.md` (CSP build 4184). rustyAC targets build 4305
  (`docs/csp/version.md`), above every CSP version gate in this server (the highest is 0.2.8 = 3424,
  `AssettoServer/Utils/CSPVersion.cs:8-12`).
* Tags used below: `[src]` read in the server's source; `[interp]` inferred, needs a capture to confirm.

## Summary in plain English

AssettoServer speaks Kunos' protocol 202 and adds a small CSP layer on top. Almost everything in that
layer is switched by `extra_cfg.yml`. **pre49's defaults switch most of it on**, though: Steam auth, CSP
client messages, WeatherFX and custom car updates, with a minimum CSP version of 0.2.0. v0.0.54's
defaults had all of these off. **AI traffic uses no new wire format.** Traffic cars are ordinary
entry-list slots driven by the server, and their positions go out in the ordinary `'H'` (0x48) batched
car-update datagram, which `crates/rustyac-net` already parses. CSP is "needed for traffic" for three
reasons:

1. The server **requires** it in the handshake by default (`CLIENT_MESSAGES` and `WEATHERFX_V1` are
   mandatory features, plus the minimum-version trick in the track name).
2. A few traffic niceties travel only as CSP messages: hiding AI nametags and minimap dots, AI colour
   changes, and turning collisions off for a slot.
3. The server's Lua script (an info/admin window, the reset-car key, collision toggles) runs only in
   CSP's Lua host.

A **fork is not needed**. Every CSP requirement can be switched off in config, and the server already
has debug switches that dump what it sends. That gives a clear order of work. First join a local server
with the CSP extras switched off, which needs nearly nothing new. Then switch the extras back on one at
a time until rustyAC joins the **default** config like real CSP does.

## 1. The handshake

**The client's request (TCP `0x3D`).** `AssettoServer.Shared/Network/Packets/Incoming/HandshakeRequest.cs:17-42` `[src]`:
AC's fields (`u16` protocol, Steam ID as a UTF-8 string, name as UTF-32, team, nation, requested car,
password), then **two CSP tail fields**, each read only if more than 2 bytes remain:

* `Features`: a UTF-8 string with a **u16 length** (`ReadUTF8String(true)`). It is a comma-separated
  list. **Its last element is the client's CSP build number**, parsed with `int.TryParse`
  (`AssettoServer/Network/Tcp/ACTcpClient.cs:393-402`, `:433-437`). This answers `online.md` open
  question 5: **CSP does send its version, in this field.** Which features CSP 4305 lists is not in the
  server's source. It needs a capture.
* `SessionTicket`: `i16` length, then that many bytes. It is accepted only if the length equals exactly
  the rest of the packet (`HandshakeRequest.cs:32-40`).

The requested car may carry `:<slot index>` (`AssettoServer/Server/EntryCarManager.cs:221-237`; feature
`SLOT_INDEX`, added after v0.0.54).

**The checks, in order** (`ACTcpClient.cs:404-424`) `[src]`:

1. Protocol must be `0x3D` with version **202**, else `UnsupportedProtocol` (0x42).
2. A Steam ID of 0 is refused unless `EnableACProSupport` is on.
3. Blacklist, password, session open, non-empty name.
4. **"Missing CSP features. Please update CSP and/or Content Manager."** This is `AuthFailed` (0x6F),
   sent when a feature marked mandatory is missing from the client's list
   (`AssettoServer/Server/CSPFeatureManager.cs:14-25`). Mandatory features in pre49:
   * `CLIENT_MESSAGES`, when `EnableClientMessages` is on (`AssettoServer/Server/ACServer.cs:61-70`);
   * `WEATHERFX_V1`, when `EnableWeatherFx` is on
     (`AssettoServer/Server/Weather/Implementation/WeatherFxV1Implementation.cs:15`);
   * `STEAM_TICKET`, **mandatory** whenever `UseSteamAuth` is on (`AssettoServer/Server/Steam/SteamService.cs:27-31`).

   Non-mandatory features that are only advertised: `SPECTATING_AWARE`, `LOWER_CLIENTS_SENDING_RATE`,
   `EMOJI`, `SLOT_INDEX` (`ACServer.cs:56-59`), `CLIENT_UDP_MESSAGES` (`:72-80`), `CUSTOM_UPDATE`
   (`:82-85`), and `FREQUENT_TRACK_CHANGES` from a plugin
   (`Plugins/VotingPresetPlugin/VotingPresetPlugin.cs:67`).
5. Open-slot filters (`ShouldAcceptConnectionAsync`). With `UseSteamAuth` on, `SteamSlotFilter` runs
   (`AssettoServer/Startup.cs:131-138`, `AssettoServer/Server/OpenSlotFilters/SteamSlotFilter.cs:20`).
   It calls Steam's `BeginAuthSession(ticket, steamId)` and waits up to 10 s for Steam's answer. A
   missing ticket gives "Missing session ticket". Family-sharing owners are blacklist-checked, and
   `ValidateDlcOwnership` app IDs are checked too (`SteamService.cs:68-157`). The check needs the
   server's own connection to Steam, so it **cannot work offline**. For our local tests,
   `UseSteamAuth: false`.
6. A free slot. Then `HandshakeResponse` (0x3E) goes out with the checksum file list (`ACTcpClient.cs:448-501`).

**`CUSTOM_UPDATE` per client** is on only when the server enables it **and** the client lists it
(`ACTcpClient.cs:431`).

**The minimum CSP version is enforced by the client, through the track name.**
`AssettoServer/Server/Configuration/ACServerConfiguration.cs:78-97` merges `extra_cfg`'s
`MinimumCSPVersion` with the one encoded in `server_cfg`'s track. It **rewrites the track name** to
`csp/<build>/../<track>`, or `csp/<build>/../<flag letter>/../<track>`
(`AssettoServer/Server/Configuration/CSPTrackOptions.cs:12-63`; flag letter = 'A' + bits
`CustomCarPhysics=1`, `CustomTrackPhysics=2`, `HidePitCrew=4`). The handshake reply carries that name.
Vanilla AC then fails to find the track. CSP parses the prefix, compares the build, and loads `<track>`
`[interp]`: the client parser is not in `online.md`, and the CSP global `0x18268cd08` ("required build",
`online.md` 1.2 item 4 and open question 8) is very likely what this sets. The default
`MinimumCSPVersion` is **2651 (0.2.0)** in pre49 (`AssettoServer/Server/Configuration/Extra/ACExtraConfiguration.cs:16`).
v0.0.54 had no extra_cfg key; the minimum existed only if the track name already carried it.

**The server's welcome to CSP (TCP `0xAB 0x03`, after the first update).** It is sent when client
messages are on (`ACTcpClient.cs:897-904`; `AssettoServer.Shared/Network/Packets/Outgoing/CSPHandshakeIn.cs:10-18`):
`AB 03 FF` + `u16 0` (HandshakeIn) + `u32 MinVersion` + `bool RequiresWeatherFx`. CSP answers with
**HandshakeOut** (type 1): `u32 version, bool weatherFxActive, u8 inputMethod, bool rainFxActive,
u8 pad, u64 uniqueKey` (`AssettoServer.Shared/Network/Packets/Incoming/CSPHandshakeOut.cs:13-21`). The
server **only logs it** (`AssettoServer/Network/CSPClientMessageHandler.cs:174-181`; it keeps
`InputMethod`). The "HWID" is a hardware ID that our client must make up or derive. It is not
checked here.

**Checksums.** The handshake reply lists files to hash. The client answers with TCP `0x44` carrying
MD5s, one per listed file in order, then **one more for the car** (`ACTcpClient.cs:638-669`). The list
is built in `AssettoServer/Server/ChecksumManager.cs`: `system/data/surfaces.ini`, the track's
`data/surfaces.ini` and `models*.ini` (per layout), and every `surfaces.ini` / `*.kn5` in the track
root (`:68-90`, `:175-192`). Car checksums are of `data.acd`, plus `collider.kn5` with
`EnableAlternativeCarChecksums` (`:104-128`). Paths in the list use the **virtual**
`content/tracks/csp/<build>/../<track>` name (`:72-87`).

**CSP surfaces quirk.** When a minimum CSP version is set, the server hashes the track's `surfaces.ini`
with the first `SUR` of `SURFACE_0` replaced by `CSP`, giving `CSPFACE_0` (`ChecksumManager.cs:135-149`,
`surfaceFix`). So a CSP client must hash that modified text. CSP very likely renames the section in
memory so vanilla AC finds no surfaces `[interp]`. rustyAC has to reproduce it byte for byte. This
check was added after v0.0.54.

**Timing.** The checksum must arrive before the server answers the first `0x46` (UDP car update):
`AssettoServer/Network/Udp/ACUdpServer.cs:140-148`. A client is kicked after
`PlayerChecksumTimeoutSeconds` = 40 s (`ACTcpClient.cs:914-925`), after `PlayerLoadingTimeoutMinutes`
= 10 min without spawning (`:503-510`), and after 15 s without a ping answer (`ACServer.cs:173-182`).

## 2. CSP's extra messages

All of them use the `0xAB` envelope (`AssettoServer.Shared/Network/Packets/Protocol.cs:48`, `:88-102`) `[src]`.

| Transport | Id | Name | Layout | Direction | Needed |
|---|---|---|---|---|---|
| TCP | `AB 00` | SpectateCar | `u8 session id` (`Incoming/SpectateCar.cs`; handler `ACTcpClient.cs:625-636`) | C to S | feature |
| TCP | `AB 02` | CarVisibilityUpdate | `u8 session id, u8 0 = visible / 1 = invisible` (`Outgoing/CSPCarVisibilityUpdate.cs:14-20`) | S to C | traffic nicety (`HideAiCars`, default **on**) |
| TCP | `AB 03` | ClientMessage | `u8 session id` (255 = server), `u16 type`, then type-specific bytes (`Shared/CSPClientMessage.cs:15-26`) | both | yes: carries HandshakeIn |
| TCP | `AB 04` | SystemMessage | enum only, not sent in core | | no |
| TCP | `AB 05` | KickBanMessage | `string` (UTF-8, 4-byte length) (`Outgoing/CSPKickBanMessageOverride.cs:8-13`) | S to C | nicety (kick reason) |
| UDP | `AB 01` | WeatherUpdate | `u64 unix time, u8 type, u8 upcoming type, u16 transition, 10 x f16` (ambient, road, grip, wind dir, wind speed, humidity, pressure, rain intensity, wetness, water) (`Outgoing/CSPWeatherUpdate.cs:20-38`) | S to C | yes, if WeatherFX is on |
| UDP | `AB 03` | CustomUpdate | `u8 count`, then per car the layout of `CSPPositionUpdate.CustomUpdateFormat` (`Outgoing/CSPPositionUpdate.cs:5-42`) | S to C | only if the client lists `CUSTOM_UPDATE` |
| UDP | `AB 05` | ClientMessage | as TCP `AB 03` | both | feature |

Client-message types (`u16`, `Shared/CSPClientMessage.cs:35-59`): HandshakeIn 0, HandshakeOut 1,
Signature In/Out/VerificationIn/Out 2-5, ConditionsV1/V2 1000/1001, ChatSharedSetup 2000,
TrackSharedTrigger(Allow) 10000/10001, DriverNameChange 14000, **CarColorChange 15000** (RGBA bytes,
`Shared/CSPCarColorUpdate.cs:15-21`), CarTyreState 15001, CarPartsState 15002, NewModeChase*
30000/30001, AdminPenalty 50000 (`Shared/CSPAdminPenalty.cs`), **LuaMessage 60000-60003**.

* Lua messages (`CSPClientMessageHandler.cs:120-172`): `[f16 range if ranged][u8 target if targeted]
  u32 event key, payload`. The server handles the keys it knows and forwards the rest: to everyone, to
  the target, or to cars within range (UDP).
* **Event key** (`AssettoServer/Network/ClientMessages/OnlineEventGenerator.cs:38-53`): XXH3-64 of
  `"server_script"` + the layout text with all whitespace removed, folded as
  `(u32)h ^ (u32)(h >> 32)`. The layout text follows CSP's `ac_struct_item.lua` field reordering
  (`:55-...`). The same rule tells us how CSP keys every `ac.OnlineEvent` `[src]`.
* Unknown types are rebroadcast (`CSPClientMessageHandler.cs:94-116`). The server never answers or
  asserts on them.
* When `EnableClientMessages` is **off**, client messages go as chat (`0x47`) with text
  `\t\t\t\t$CSP0:<base64 without padding>` (`Outgoing/CSPClientMessageOutgoing.cs:30-37`). This matches
  `online.md` 1.1 channel 4.

**Welcome text (`0x51`) payload.** `AssettoServer.Shared/Utils/CSPServerExtraOptionsParser.cs:10-67` `[src]`:
the visible message, then **32 tabs** + `$CSP0:` + base64 (padding cut) of the **zlib-compressed**
UTF-8 INI. This answers `online.md` open question 1. The server always adds `[EXTRA_TWEAKS]
MIN_TIME_BETWEEN_COLLISIONS = 2`, plus `[EXTRA_DATA] CUSTOM_UPDATE_FORMAT = '...'` when custom updates
are on, and `[EXTRA_TWEAKS] VERIFY_STEAM_API_INTEGRITY = 1` when **Steam auth is off**
(`AssettoServer/Server/Configuration/CSPServerExtraOptions.cs:30-43`). The script entries come next (§4).

**What is needed only to stay connected:** answering pings, sending the checksum `0x44` in time, and
sending `0x46` updates. Nothing CSP-specific is needed after the handshake. HandshakeOut is not
required: the server never times out waiting for it. Unknown `0xAB` frames only have to be skipped.
Everything else in the table is for features.

**Server feature discovery.** `/api/details` returns `Features = cspFeatureManager.Features.Keys`
(`AssettoServer/Network/Http/HttpController.cs:96-150`, `:146`), which matches `online.md` 1.2 item 2.

## 3. AI traffic

* AI cars are **ordinary entry-list slots** with an AI mode (`AssettoServer/Server/EntryCar.cs:122`;
  `EnableAi`, default off). They join through the usual `CarConnected` (0x5A) with the AI's name and
  leave through `CarDisconnected` (`AssettoServer/Server/EntryCarAi.cs:301-337`). A player taking a slot
  despawns its AI (`ACTcpClient.cs:439-440`).
* **Positions:** every server tick, each AI slot produces a normal `PositionUpdateOut` for each player
  (`ACServer.cs:185-205`). AI updates are always batched. Clients without `CUSTOM_UPDATE` get the plain
  `'H'` / 0x48 mega packet (`u32 time, u16 ping, u8 n, n x state`,
  `Outgoing/BatchedPositionUpdate.cs:16-25`), in chunks of 20 (`ACServer.cs:209-230`). **No new message
  is needed to see traffic.**
* **One slot, many AI cars ("overbooking").** A slot holds several AI states. Each player is sent the
  state closest to them (`EntryCarAi.cs:166` `GetBestStateForPlayer`; `EntryCar.cs:240-288`), with a
  **per-viewer** sequence counter (`EntryCar.cs:306-308`). When the chosen state changes, the same slot
  **jumps** elsewhere. If `AiEnableColorChanges` is set, the server sends CarColorChange at that moment
  (`EntryCar.cs:271-285`). For rustyAC's remote-car model (`net_car.rs`, AC's dead reckoning) these jumps
  look like teleports. CSP has its own jump and collision-resume handling (`online.md` 1.5) `[interp]`.
* Cars outside `NetworkBubbleDistance` (500 m) are updated at 4 Hz (`EntryCar.cs:294-304`;
  `ACExtraConfiguration.cs:40-42`).
* **What needs CSP for traffic, and why:**
  1. Nothing in the traffic stream itself. The gates come from §1 defaults (mandatory
     `CLIENT_MESSAGES`, `WEATHERFX_V1`, the `csp/` track name).
  2. `HideAiCars` (default **true**, `AssettoServer/Server/Configuration/Extra/AiParams.cs:63-64`:
     "Hide AI car nametags and make them invisible on the minimap") sends `AB 02` for every AI slot. A
     client that ignores it shows nametags and minimap dots for traffic.
  3. Collisions: `SetCollisions` sends the Lua online event `AS_CollisionUpdate`
     (`EntryCar.cs:336-349`; "broken on CSP < 0.2.8"). It is applied on the client by the **Lua
     script** with `physics.disableCarCollisions` (`AssettoServer/Embedded/Lua/assettoserver.lua:81-89`).
     Pending toggles are re-sent on `AS_LuaReady` (`ACTcpClient.cs:247-258`). Without a Lua host, the
     "no collision" state of a car is not applied.
  4. AI colour changes (optional) need CarColorChange 15000.
  5. Car reset (`EnableCarReset`, default off) needs CSP 0.2.8, client messages and an AI spline
     (`ACServer.cs:88-95`).
* The traffic **car models** must exist on the client like any entry-list car
  (`AutoAssignTrafficCars` picks AI slots by car folder name, `AiParams.cs:10-12`).

## 4. Server Lua scripts

* **How they ship.** `AssettoServer/Server/CSPServerScriptProvider.cs:79-125` adds one INI section per
  script to the welcome payload: `[SCRIPT_<10+n>-<name>]` `SCRIPT = 'http://{ServerIP}:{ServerHTTPPort}/api/scripts/<n>'`,
  plus optional extra keys (e.g. `CHECKSUM`). CSP substitutes the placeholders and downloads over HTTP
  from `GET /api/scripts/{n}` (`HttpController.cs:152-155`, `text/x-lua`). A `DebugScriptUserGroup`
  gets local file names instead (`:41-52`). This is the plain format **as the server writes it**.
  `online.md` 1.3.5 lists CSP's *reader* of these keys as obfuscated and blocked, but we now have the
  format from open-source server code, without decoding CSP.
* **Scripts in pre49:** core `assettoserver.lua` (284 lines, always loaded, `ACServer.cs:87`).
  `ai_debug.lua` (51) is loaded only for AI debugging (`AssettoServer/Server/Ai/AiBehavior.cs:54`).
  Plugins add `fasttravel.lua` (798), `automoderation.lua` (47), `replay.lua` (42),
  `reconnectclient.lua` (43) and `tagmode.lua` (20).
* **What `assettoserver.lua` uses of CSP's Lua API:** `ac.OnlineEvent` + `ac.StructItem.key/string/vec3/boolean/byte`
  (events `AS_ApiKey`, `AS_TeleportCar`, `AS_CollisionUpdate`, `AS_TeleportToPits`, `AS_RequestResetCar`,
  `AS_LuaReady`); `web.get/post`, `JSON.parse`, `stringify`; `ac.getServerIP`, `ac.getServerPortHTTP`,
  `ac.getTrackID`, `ac.log/debug`, `ac.INIConfig.onlineExtras():get`, `ac.ControlButton(...):onPressed`,
  `ac.SpawnSet.Pits`; `physics.setCarPosition`, `setCarVelocity`, `disableCarCollisions`, `teleportCarTo`;
  `car.sessionID`, `sim.isAdmin`; `os.openURL`; and the ImGui subset `ui.registerOnlineExtra`, `tabBar`,
  `tabItem`, `childWindow`, `image` (from a URL), `text*`, `pushFont`, `treeNode`, `checkbox`, `beginCombo`,
  `selectable`, `inputText`, `button`, `addIcon`, `toast`, `setTooltip`, `pushStyleColor` (lines 20-284).
  The last line, `luaReadyEvent({})`, tells the server that the scripts loaded. The server then sends
  `AS_ApiKey` (used for the admin config HTTP API) and the collision states.
* **Is the Lua host needed early?** Not to connect or stay connected: nothing kicks a client that never
  sends `AS_LuaReady`. It is needed for collision toggles, the reset key, teleports (FastTravel) and the
  admin window. The online-script kind is CSP-11 (Lua host) + CSP-17 (`ui.*`) + the `physics.*` subset
  + `web` / `JSON` (`online.md` 1.8).

## 5. Weather / time sync

* `EnableWeatherFx` defaults to **true** in pre49 (`ACExtraConfiguration.cs:50`; v0.0.54: false). It
  picks `WeatherFxV1Implementation` over the vanilla one (`AssettoServer/Server/Weather/WeatherModule.cs:17-27`).
  It makes `WEATHERFX_V1` **mandatory** and sends UDP `AB 01` (layout in §2) to everyone, or to one
  client on join (`WeatherFxV1Implementation.cs:18-46`; join: `ACTcpClient.cs:866`).
* In FX mode the time of day travels **as a unix timestamp** in that packet. Vanilla mode sends AC's
  `WeatherUpdate` (0x78) + `SunAngleUpdate` (0x54) (`AssettoServer/Server/Weather/Implementation/VanillaWeatherImplementation.cs:20-70`).
  `CSPHandshakeIn.RequiresWeatherFx` tells the client too.
* For rustyAC: honouring `AB 01` properly is CSP-13 (weather + true night). A stop-gap is to map the FX
  type onto a vanilla weather and compute the sun from the timestamp. For our own server, the cheap
  path is `EnableWeatherFx: false` until then. Traffic servers in the wild (No Hesi style) do use
  WeatherFX `[interp]`: it is the default.

## 6. Hard-wired vs configurable

| Requirement | Switch (`extra_cfg.yml`) | Default pre49 / v0.0.54 | In code only |
|---|---|---|---|
| Steam ticket (`STEAM_TICKET` mandatory) | `UseSteamAuth` | true / false | Off adds `VERIFY_STEAM_API_INTEGRITY = 1` to the welcome payload (CSP-side check, meaning not read) |
| Minimum CSP build (`csp/` track name, surfaces fix) | `MinimumCSPVersion` (`uint?`, null allowed), or the track name in `server_cfg.ini` | 2651 / none | |
| `CLIENT_MESSAGES` mandatory | `EnableClientMessages` | true / false | Mandatory whenever on. Startup refuses it with a minimum below 1937 (`ACServer.cs:63-66`), but a **null** minimum passes that check (C# lifted `<` gives false) `[interp]` |
| `CLIENT_UDP_MESSAGES` | `EnableUdpClientMessages` | true / false | needs min 2651 (`ACServer.cs:72-80`) |
| `WEATHERFX_V1` mandatory | `EnableWeatherFx` | true / false | |
| `CUSTOM_UPDATE` | `EnableCustomUpdate` | true / false | never mandatory |
| AI hide / colours / reset | `AiParams.HideAiCars`, per-entry `AiEnableColorChanges`, `EnableCarReset` | true, false, false | |
| Base features (`SPECTATING_AWARE`, `EMOJI`, `SLOT_INDEX`, `LOWER_CLIENTS_SENDING_RATE`) | none | always advertised | never mandatory, harmless |
| `assettoserver.lua` | none | always added | the client may ignore it |
| Protocol 202, Steam ID ≠ 0 | `EnableACProSupport` for ID 0 | | yes; rustyAC already sends 202 and a Steam ID |
| Client security level | `MandatoryClientSecurityLevel` (`internal set`) | 0 | reset to 0 unless the closed `ClientSecurityPlugin` is installed (`ACServerConfiguration.cs:349-353`; used in `ACUdpServer.cs:148`) |

**Fork: not needed.** Every CSP requirement above has a switch, so an unmodified server can be set up
from "vanilla-like" to "default". The server already ships the logging we need:
`DebugWelcomeMessage: true` writes `debug_welcome.<slot>.txt` and `debug_csp_extra_options.<slot>.ini`
(`CSPServerExtraOptions.cs:63-67`), `DebugClientMessages: true` logs every client message
(`CSPClientMessageHandler.cs:58-61`, `:106-109`, `:139-144`), and the log level can be raised to
Verbose for every packet id (`ACTcpClient.cs:368-371`). A fork would only be worth it for test hooks
(e.g. forcing an AI state switch on command), and the `CustomCommandPlugin` / Rcon may already cover
that. No Hesi runs its own build and plugins, so a fork would not teach us their behaviour anyway.

## 7. Licence

`LICENSE` is the **GNU AGPL v3** (661 lines). `README.md:39-50` adds two section-7 permissions:
combining with the Steamworks SDK, and with the project's Patreon plugins. The same text is in
`assettoserver.lua:1-18`. What this means:

* **Running it unmodified** for other players carries no extra obligation.
* **Running a modified copy that other players connect to** triggers AGPL section 13 (`LICENSE:540`):
  every player who interacts with it over the network must be offered the **complete corresponding
  source of the modified version**, e.g. a link in the welcome message or a public fork. The
  `assettoserver.lua` window also shows the licence in game.
* **rustyAC talking to it is interoperability, not a derived work.** We describe the protocol in our
  own words (this file) and do **not copy AGPL code** (C# or Lua) into rustyAC.
* Super's own server config, passwords and plugins stay out of the repo (rules 7, 8).

## 8. Versions: v0.0.54 (stable) vs v0.0.55-pre49 (newest pre-release)

Things between them that change what a client must do `[src]` (from `git diff v0.0.54 v0.0.55-pre49`
and `git log`):

* **Defaults:** v0.0.54 has `UseSteamAuth`, `EnableClientMessages`, `EnableUdpClientMessages`,
  `EnableWeatherFx` and `EnableCustomUpdate` all **false** and no `MinimumCSPVersion` key. pre49 has
  them **true** with minimum 2651 (commits `84330f1` "Enable Steam auth by default", `6d01c08` "update
  default min CSP version to 0.2.0"). Out of the box, v0.0.54 accepts a near-vanilla client and pre49
  does not.
* **Handshake:** the `Features` + ticket tail exists in both. pre49 adds the `car:slot` form
  (`a19de3a`, feature `SLOT_INDEX`) and tolerates a non-numeric Steam ID (`TryParse`). The ticket-length
  fix (`4fc795a`) is only in the server's own writer.
* **Checksums:** pre49 adds the `CSPFACE` surfaces fix and the virtual `csp/` paths (`ChecksumManager.cs`
  changes). It also waits for checksums before the first update and the welcome (`8af59c1`).
* **Lua:** `AS_LuaReady`, `AS_CollisionUpdate`, `AS_TeleportCar` and `AS_RequestResetCar` are new
  (`2fabb97`, `16647b8`). The online-event key generator was reworked (`OnlineEventGenerator.cs`, 123
  lines changed; `68c37e1` "uint never worked"). The embedded `assettoserver.lua` moved into
  `Embedded/`.
* **Other CSP packets:** AdminPenalty 50000 (`1fab95f`), `JumpStartPenalty` client event 0x0C. Protocol
  id 0x3B was renamed `Blacklisted` (same value).
* Steam auth moved to a single Steamworks implementation (`0e60c6c`, `5237c0d`). The wire stays the
  same: the ticket is in the handshake.

**Target pre49** (or newer), since it is what current traffic servers run. v0.0.54 is the easy first
target if we want "connect with zero CSP".

## Proposed task list (rustyAC side)

Sizes are hours for one session. "CSP-n" refers to `docs/csp/plan.md` §2; online pieces refer to
`docs/csp/online.md` §7 rows.

| # | Task | Delivers | Depends on | Size |
|---|---|---|---|---|
| **NH-1** | Join a "vanilla-like" local AssettoServer | No code expected: `crates/rustyac-net` speaks 202. Config `UseSteamAuth/EnableClientMessages/EnableWeatherFx/EnableCustomUpdate: false`, `MinimumCSPVersion: null`, `EnableAi: true`. Proof that traffic shows through `'H'`, plus a list of the `0xAB` frames still arriving (`AB 02` from `HideAiCars`) | none | 2 h |
| **NH-2** | CSP handshake basics | `Features` tail (list + build number); `csp/<build>/../[X/../]<track>` name parse; `CSPFACE` surfaces checksum and virtual checksum paths; `$CSP0:` welcome decode (tabs, cut base64, zlib, INI); `0xAB` TCP/UDP envelope receive, unknown ids skipped; HandshakeIn read and HandshakeOut reply. Then join with `MinimumCSPVersion: 2651` and `EnableClientMessages: true` | `online.md` rows 1, 2, 4; CSP-1 (`--csp` switch); INI reader (not full INIpp) | 5 h |
| **NH-3** | Traffic niceties without Lua | `AB 02` visibility (hide nametag / minimap); CarColorChange 15000; `AS_CollisionUpdate` read natively as a stop-gap until the Lua host exists. The event key hash (XXH3 of the layout) gets a unit test against keys computed from the server's generator rule | NH-2 | 4 h |
| **NH-4** | WeatherFX V1 | `WEATHERFX_V1` in the feature list; `AB 01` parsed; stop-gap mapping to vanilla weather + sun from the timestamp; the full look comes with CSP-13 | NH-2; CSP-13 for parity | 3 h (+ CSP-13) |
| **NH-5** | Remote-car jumps (AI state switching) | rustyAC's `net_car.rs` handling of slot jumps / per-viewer sequence restarts matched to CSP's jump handling (`online.md` row 8) | NH-1; `online.md` row 8 | 5 h |
| **NH-6** | Server Lua scripts | `[SCRIPT_n-name] SCRIPT=` entries; HTTP fetch from the server's port (localhost only in tests); run as the online-script kind with the API in §4; `AS_LuaReady` sent | CSP-11 (Lua host), CSP-17 (`ui.*`), `physics.*` subset, `web`/`JSON` | 5 h x2 (after CSP-11/17) |
| **NH-7** | `CUSTOM_UPDATE` | the format DSL from `[EXTRA_DATA] CUSTOM_UPDATE_FORMAT`, `AB 03` UDP parse; advertised only once done | NH-2; `online.md` row 11 | 5 h |
| **NH-8** | Steam ticket | `STEAM_TICKET` + ticket in the handshake via the game's `steam_api64.dll`. Needs Steam running, and a server with `UseSteamAuth: true` needs the network to validate. Last: it is only needed for public servers | `online.md` row 3 | 4 h |

Order: NH-1, NH-2 (connect, stay connected), NH-3 + NH-4 (default config joins, traffic looks right),
NH-5, then NH-6 once the Lua host exists, then NH-7 / NH-8.

## What to record from real AC + CSP (local AssettoServer only, loopback)

One kit, `re/scratch/taskNH*/`: a **local** AssettoServer pre49 (its own folder in the repo,
made-up passwords, `UseSteamAuth: false`, `DebugWelcomeMessage` + `DebugClientMessages` on, log level
Verbose), a small traffic track, real AC + CSP 4305 joined through Content Manager, and a loopback
packet capture. Never a public server.

| Proves | Capture |
|---|---|
| NH-1 | Server config A (all CSP off) + rustyAC: server log "has connected", `'H'` datagrams with AI slots, `CarConnected` names |
| NH-2 | Config B (defaults minus Steam): CSP's `0x3D` frame (the exact `Features` string and order; with and without `STEAM_TICKET`); the `0x44` checksum frame (order and the `CSPFACE` hash); the `0x51` frame vs `debug_welcome.N.txt`; the `AB 03` HandshakeIn / HandshakeOut pair; CSP's log lines "Server features ..." |
| NH-3 | `AB 02` per AI slot; a colour change with `AiEnableColorChanges`; an `AS_CollisionUpdate` frame + server log (event key) |
| NH-4 | `AB 01` datagrams across a weather change and a time jump, with CSP screenshots at the same moments |
| NH-5 | A drive past several AI cars sharing a slot: datagrams + a CSP video / log of what CSP shows at each switch |
| NH-6 | `GET /api/scripts/n` requests and bodies; the `AS_LuaReady` / `AS_ApiKey` frames |
| NH-7 | Config with `CUSTOM_UPDATE`: the `[EXTRA_DATA]` format line and `AB 03` UDP datagrams |
| NH-8 | Later, with Steam: the `0x3D` frame with a ticket (length field and size only; the ticket bytes are never kept in the repo) |

## Not done / open questions

* No capture was made and no server was run (research only). Every `[interp]` above needs the captures
  in the previous section.
* **Which features CSP 4305 lists** in the handshake, and whether it lists only what `/api/details`
  advertised or everything it supports.
* How CSP parses `csp/<build>/../` and applies `CSPFACE` (client side, no decompile here). Also what
  `VERIFY_STEAM_API_INTEGRITY` checks on the client.
* The exact meaning of CarVisibility on CSP's side, beyond the server's description (nametags, minimap).
* The plugin scripts (`fasttravel.lua` and others) were not read API by API. No Hesi's scripts are
  unknown.
* The `ClientSecurityPlugin` (closed source, Patreon) is not in the clone. It may matter for No Hesi.
* `re/scratch/taskNH/` was not created: research needed no scratch files.

## Resume here

Task NH is finished. Next: NH-1 (needs a local AssettoServer copy and Super's go-ahead to run it on
localhost).
