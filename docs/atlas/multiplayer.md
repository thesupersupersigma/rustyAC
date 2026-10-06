# Multiplayer (online client, network interfaces)

Source: `re/decomp/` pseudo-C (folders `ACClient`, `ACClientVotingManager`, `ACChat`, `NetCarStateProvider`,
`RemoteCarStateProvider`, `TCPSocket`, `UDPSocket`, `UDPPacket`, `IPAddress`, `ksNetwork`, `RemoteTelemetryUDP`,
`UDPCommandListener`, `QOSOverlay`, `NetMonitorForm`, parts of `RaceManager`, `SteamInterface`), `re/types/*.txt`,
`re/index/strings.tsv`, `tools/re_query.py` (callers / callees / strings / globals / vtable). Read-only: nothing was
modified. "Confirmed" means read in the pseudo-C; "interpretation" is marked as such. Message names starting with
`ACP_` are taken from log strings in the binary; the PDB carries no enum for the ids, so all other names are mine.

## 1. What it is

`acs.exe` is only ever a client. When `race.ini` has `[REMOTE] ACTIVE=1`, `RaceManager` builds one `ACClient`,
which connects to a dedicated server over TCP, performs a blocking handshake during the loading screen, downloads
the list of cars in pages of ten, and creates one remote car per server slot.
After loading, two sockets are used. TCP carries everything that must arrive: session changes, lap and sector
times, chat, votes, kicks, damage, tyre compound, weather, penalties, setup and checksum data. UDP carries the
high-rate car positions in both directions plus a ping exchange. Each message is one id byte followed by packed
little-endian fields; TCP messages are framed with a 16-bit length.
Networking runs on the physics thread: after every 3 ms physics step `ACClient::onPhysicsStep` sends the local car
state if the server-chosen send interval has elapsed and drains up to ten packets from each socket. Anything that
touches game objects is posted to the main thread.
Remote cars do not run car physics. Each is a `NetCarStateProvider`: it keeps the last three received states,
extrapolates position from velocity and estimated acceleration (dead reckoning), blends out the correction error,
places wheels on the ground with ray casts, and moves a collision body built from the car's `collider.kn5` so the
local car can hit it. The local car is always simulated locally; the server does not correct it.
The server dictates assists, tyre blankets, damage / fuel / tyre-wear rates, grip, weather, sun angle, ballast and
restrictor, fixed setups, DRS zones and penalties, and can ask for MD5 checksums of files.
Two unrelated local UDP services live in the same code family: a telemetry broadcaster (port 9996) and a remote
command listener (port 9666).

## 2. Main classes

| Class / unit | Source file (library) | Size | Role |
|---|---|---|---|
| `ACClient` | ACClient.obj | 0x10818 | The online session. Sockets (`sok` UDP, `tcpSock` TCP), `serverInfo` (ip, tcp / udp / http ports), `serverIP`, `sessionID` (own slot), `netCars`, `sendInterval`, `lastSendTime`, `pakSequenceIndex`, `isAssociated`, `guid`, `playerCarMD5`, `checksumResults`, `serverDrivingAssists`, `sessions` / `currentSession`, lap and split tables for every driver, `wreckerProtection`, `damageReport`, `qos`, pit window, result-screen timers; events `evOnChatMessage`, `evOnOnlineNewSession`, `evOnOnlineEndSession`, `evOnLapCompleted`, `evOnVoteReceived`, `evOnVoteNotPassed`, `evOnMandatoryPitDone`. |
| `NetCarStateProvider` | NetCarStateProvider.obj | 0x15b8 | One remote car. Implements `ICarPhysicsStateProvider` (so `CarAvatar` renders it like any car). `netStates[3]`, `incomingStateQueue`, `state` (a full `CarPhysicsState` that it fabricates), `bodyMatrix`, wheel / suspension matrices, `errorVector`, `body` (`IRigidBody*`), `colliderModel`, `rayCastProvider`, `slipStream`, wing / DRS / push-to-pass state, `ping`, laps, `isDisconnected`. |
| `NetCarState` | (struct) | 0x88 | One received sample: `pos`, `rotation` (euler), `velocity`, `timeStamp`, `rcvTime`, `pakSequenceId`, `tyreAngularSpeed[4]`, `engineRPM`, `steerAngle`, `wheelAngle`, `gearIndex`, `statusBytes`, `ping`, `performanceDelta`, `gas`. |
| `RemoteCarStateProvider` | RemoteCarStateProvider.obj | - | No real code in this build (only compiler-generated statics). |
| `ACClientVotingManager` | ACClientVotingManager.obj | 0x90 | Shows the running vote, reads Y / N keys, sets up three items named `vote_next`, `vote_restart`, `vote_kick` (commands or controls; not checked which), calls `ACClient::sendVote`. |
| `ACChat` | ACChat.obj | 0x200 | Chat widget of the pit / race-control screen (list + text input). The in-car chat app is `FormChat` (SystemApps.lib). |
| `UDPSocket` | UDPSocket.obj (ksNetwork.lib) | 0x38 | Non-blocking datagram socket with listener callbacks; answers the server's ping itself and keeps `ping`, `lastPingTime`. |
| `TCPSocket`, `TCPQueue` | TCPSocket.obj (ksNetwork.lib) | 0x10078 | Stream socket with a 64 KB reassembly buffer; delivers whole length-prefixed messages to listeners. |
| `UDPPacket` | UDPPacket.obj (ksNetwork.lib) | 0x20 | Byte buffer with typed `read<T>` / write, ANSI and wide strings; `send` to a UDP or TCP socket. Used for both transports. |
| `IPAddress`, `InitKSNetwork` | IPAddress.obj, ksNetwork.obj (ksNetwork.lib) | 0x10 | `sockaddr_in` wrapper; `WSAStartup` / cleanup. |
| `RemoteTelemetryUDP` | RemoteTelemetryUDP.obj | 0x1d0 | UDP telemetry server on port 9996: `listeners`, `rti` (`RTCarInfo`, 328 bytes), lap notifications (`RTLap`, 212 bytes). |
| `UDPCommandListener` | UDPCommandListener.obj | 0x68 | UDP socket on port 9666 accepting one-letter commands (restart session, start, back to pits ...). |
| `QOSOverlay` | QOSOverlay.obj | 0x68 | HUD text with the ping. |
| `NetMonitorForm` | NetMonitorForm.obj (SystemApps.lib) | 0x1e0 | Debug app: graphs of velocity, error vector and ping of a remote car. |

## 3. Entry points

Start-up (main thread, during loading)
- `InitKSNetwork` 0x14025e7c0 from `wWinMain`.
- `RaceManager::loadSessions` 0x14013ecb0 -> `RaceManager::initOnline` 0x14013d160 when `[REMOTE] ACTIVE` is set.
  In order (confirmed from the callee list): `ACClient::ACClient` 0x14003a2c0, `ACClient::handshakeTCP`
  0x140048b50, `Sim::loadTrack`, `ACClient::getRemoteCarList` 0x140046f80 (retried), `ACClient::getCarMD5`
  0x140046070, `Sim::addCar` for the player, grid position from the handshake, `Sim::addNetCar` 0x140197c30 for
  every other slot (creates `NetCarStateProvider::NetCarStateProvider` 0x140116c70 and registers it with
  `ACClient::addNetCar` 0x1400442c0), `ACClient::beginUpdateMode` 0x1400446e0 (installs `ACClient::onMessage`
  0x14004b9d0 on the UDP socket and `ACClient::onMessageTCP` 0x14004c840 on the TCP socket, switches TCP to
  non-blocking), `ACClient::askForP2Pvalue` 0x140044470.
- `ACClient::ACClient` also creates `ACClientVotingManager::ACClientVotingManager` 0x140054460 and
  `QOSOverlay::QOSOverlay` 0x1400ac470 and subscribes to `PhysicsEngine::evOnStepCompleted`.

Physics thread, after every step (`evOnStepCompleted` handlers)
- `ACClient::onPhysicsStep` 0x14004f1d0: `ACClient::sendCarPosition` 0x140052230; `UDPSocket::receive`
  0x14025d400 (max 10 datagrams); `TCPSocket::receive` 0x14025e410 (max 10); every 5 s flushes queued collision
  events (message 0x82); `ACClient::updateQOS` 0x1400541d0.
- `NetCarStateProvider::step` 0x14011bd00 for each remote car.
- Handlers that must touch game objects wrap the work in a lambda and call `Sim::executeOnMainThread` 0x140199240.

Main thread, every frame
- `ACClient::update` 0x140053b30: sends the checksum answer once, sends a small UDP "session check" every 2 s,
  shows "The server seems to be unavailable." when no server ping arrived for 20 s, counts wrong-way infractions.
- `ACClient::renderHUD` 0x140051650, `NetCarStateProvider::update` 0x14011d950, `ACClientVotingManager::update`
  0x140055600, `QOSOverlay::renderHUD` 0x1400ac5d0, `ACChat::render` 0x140036380.
- `ACClient::shutdown` 0x140052d30 sends the disconnect message.

Handshake (`ACClient::handshakeTCP`, confirmed)
- Reads `cfg/race.ini`: `[REMOTE] SERVER_IP, SERVER_PORT, SERVER_HTTP_PORT, SERVER_NAME, NAME, TEAM, PASSWORD,
  REQUESTED_CAR` and `[CAR_0] NATION_CODE`. `SERVER_PORT` is the TCP port; the UDP port is told by the server.
- Sends id `0x3d`, a 16-bit protocol version `0x00ca` (202), then six strings; the first is the player GUID
  (`SteamInterface::playerGUID`, the Steam community id set by `SteamInit` 0x1401a7950), the rest are name, team,
  nation, requested car and password (order of the last five not checked).
- Reply ids: `0x3e` accepted; `0x3b` `ACP_BLACKLISTED`; `0x3c` `ACP_WRONG_PASSWORD`; `0x41`
  `ACP_SERVER_STILL_BOOKING` (seconds to wait); `0x42` `ACP_UNSUPPORTED_PROTOCOL`; `0x45`
  `ACP_NO_SLOTS_AVAILABLE`; `0x6e` `ACP_SESSION_CLOSED`; `0x6f` `ACP_AUTH_FAILED` (with a reason string).
- The accepted reply carries, in order: server name, UDP port (u16), requested send rate in Hz (u8, stored as
  `sendInterval` in ms), track, track config, car model, skin, sun angle, allowed tyres out, tyre blankets, TC
  mode, ABS mode, stability allowed, auto-clutch allowed, jump-start penalty mode (lock on grid / teleport to pit
  / drive-through), mechanical damage rate, fuel rate, tyre wear rate, one flag stored in `Sim::useMousePitstop`,
  max contacts per km (wrecker protection), race-over time, result-screen time, extra-lap flag, gas-penalty
  flag, pit window start / end, inverted grid positions, the assigned `sessionID`, the session list, the current
  session (`ACClient::onNewSession` 0x14004e4a0: name, type, time, laps, grip, grid, start time), a list of file
  paths to checksum, a random seed and the server time.

Messages seen in the code (hex id; all multi-byte fields little-endian)

| Id | Transport, direction | Meaning |
|---|---|---|
| 0x0d | TCP both | `ACP_CE_P2P`: push-to-pass count / activation for a car. |
| 0x0e | TCP both | `ACP_CE_MANDATORY_PIT`: mandatory pit stop done. |
| 0x3b | TCP out | Sent once when the wrecker-protection contact ratio is exceeded (`blackListRequested`). |
| 0x3d | TCP out | Handshake request. |
| 0x3f, 0x40 | TCP out / in | Car list request (start index) and `ACP_CAR_LIST_RESPONSE` (up to 10 cars per reply; per car a slot id, five strings including driver, team and nation, and damage zone levels; exact order not decoded). |
| 0x43 | TCP out | Disconnect. |
| 0x44 | TCP out | Checksum answer: the MD5 digests in list order, the player's car digest last. |
| 0x46 | UDP both | Car update for one car (layout below). |
| 0x47 | TCP both | Chat (car id + wide string). |
| 0x48 | UDP in | "Mega packet": server timestamp, ping, then several car updates in one datagram. |
| 0x49 | TCP both | Lap completed (out: own lap with cut count; in: any driver, updates leaderboards). |
| 0x4a | TCP in | New session (`onNewSession`). |
| 0x4b | TCP in | `ACP_RACE_OVER` with the result table. |
| 0x4d | TCP in | `ACP_CLIENT_DISCONNECTED`. |
| 0x4e | UDP both | Association: the client repeats it instead of car updates until the server echoes it (`isAssociated`), which ties the UDP address to the TCP slot. |
| 0x4f | UDP out | Session check every 2 s (carries the current session type). |
| 0x50 | TCP both | `ACP_TYRE_COMPOUND_CHANGED`. |
| 0x51 | TCP in | `ACP_TEXT_FILE` (type 0 = welcome message). |
| 0x52 | TCP in | Setup from the server (fixed setup; loaded through `SetupScreen::loadSetupAbsolutePath`). |
| 0x53 | TCP in | DRS zones. |
| 0x54 | TCP in | Sun angle. |
| 0x56 | TCP both | `ACP_DAMAGE_UPDATE`: damage zone levels (out at most every 2 s). |
| 0x57 | UDP in | `ACP_RACE_START_UPDATE`: race start time, applied if the packet is fresh relative to the ping. |
| 0x58 | TCP both | Sector split. |
| 0x5a | TCP in | A driver connected to a slot (driver info, system message, damage reset). |
| 0x5b | TCP in | Driver names for a list of slots. |
| 0x64, 0x65, 0x66 | TCP both | Vote next session / restart session / kick user (in: vote status, raised as `evOnVoteReceived`). |
| 0x67 | TCP in | Vote not passed. |
| 0x68 | TCP in | Kick notice with reason (voted kick, ban until restart, blacklist, checksum failed, admin kick). |
| 0x70 | TCP in | Ballast and restrictor for a car. |
| 0x78 | TCP in | `ACP_WEATHER_UPDATE`: ambient and road temperature, weather graphics name, wind. |
| 0x82 | TCP both | Client events. Out: batched collisions (sub-type 0x0a with another car's slot, 0x0b with the environment, speed and positions). In: sub-type 0x0c penalty notices ("Jumpstart detected for ...", "Drive through", "Starts from pits"). |
| 0xf9 / 0xf8 | UDP in / out | Ping from the server (carries the last measured ping) and the reply with local time; handled inside `UDPSocket::receive`. |

Car update payload (decoded from the receive side of 0x46; the sender mirrors it): slot id (in), sequence number
(u8), timestamp (u32, ms), ping (u16, in), position (3 floats), euler rotation (3 floats), velocity (3 floats),
four wheel speeds (1 byte each, signed log scale: `sign * (10^(|b - 100| / 20) - 1)`), steer angle (u8 - 127),
front wheel angle ((u8 - 127) / 2), rpm (u16), gear (u8), status bit field (u32: lights, limiter, KERS charging
and similar flags), performance delta (i16, ms), gas (u8 / 255). About 60 bytes per car per update. The rate is
whatever the server requested in the handshake; `sendCarPosition` is evaluated every physics step.

Remote car motion (`NetCarStateProvider`, confirmed in outline)
- `onRemoteStateReceived` 0x14011b1e0 accepts a state if its sequence number is newer (with wrap-around) or its
  timestamp differs by more than a second, and shifts the three-slot history.
- `projectNetStatePos` 0x14011b900: `p + v * t + 0.5 * a * t^2`, with `a` = difference of the last two velocities
  over their time difference and `t` = current physics time minus the sample's timestamp.
- `getSmoothBodyMatrix` 0x140119310: adds the remaining `errorVector` scaled by a factor that decays over
  `ACClient::remoteFactor`, snaps when the error exceeds 3 m, and eases orientation (lerp 0.1).
- `step`: ray cast under the car for ground height, update of the collision `body`, wheel matrices from
  wheelbase / track / radius, wheel spin from the received speeds, steering, wings / DRS (`stepWings`
  0x14011d7c0), push-to-pass (`stepP2P` 0x14011d750), slipstream source, `stepLagging` 0x14011cf60 when packets
  stop. `getPhysicsState` 0x140119220 hands the fabricated `CarPhysicsState` to the avatar, so audio, graphics and
  leaderboards treat remote cars like local ones.

Checksums (anti-tamper)
- `ACClient::getCarMD5`: MD5 of `content/cars/<car>/data.acd`, or of the `data/` folder (`md5Folder` 0x14023a450)
  when the car is unpacked.
- The handshake reply lists files; each must exist and is hashed with `md5` 0x14023a290. A missing file aborts the
  join ("Problems during checksum"). The digests are sent with message 0x44; the server answers a mismatch with a
  kick (reason "Checksum failed"). The code is `MD5CheckSum.obj`, counted in `config.md`.

Chat, votes, Steam
- `ACClient::sendChat` 0x140052a90 (at most one message per second; `handleLocalAdminMessages` 0x140048670
  intercepts `/client_list` locally). Callers: `ACChat`, `FormChat`, and the Python function `ac_sendChatMessage`.
  Incoming chat raises `evOnChatMessage` -> `ACChat::onChatMessage` 0x140035c00.
- `ACClient::sendVote` 0x140052ba0 (type, yes / no, target slot).
- Steam: only the player's id string is used by the client. `SteamInterface` itself (achievements, leaderboards,
  stats) is not part of this area. No authentication-ticket call was found in the client code.

Local UDP interfaces (created in `Sim::Sim` 0x140192070, independent of online play)
- `RemoteTelemetryUDP::RemoteTelemetryUDP` 0x1401468e0 binds UDP 9996. `RemoteTelemetryUDP::listen` 0x140146cc0
  handles a request datagram whose operation field is 0 (handshake: reply with car name, driver name, identifier 4242,
  version, track name and config), 1 (subscribe to per-frame `RTCarInfo`), 2 (subscribe to lap `RTLap` "spot"
  events), 3 (dismiss), plus two further operations (4, 5) not decoded. `updateRTCarInfo` 0x1401477c0 fills the
  struct from the player car, `updateListeners` 0x140147680 sends it, `onLapUpdateEvent` 0x1401473a0 sends laps.
- `UDPCommandListener::UDPCommandListener` 0x1401d0f30 binds UDP 9666; `UDPCommandListener::update` 0x1401d1570
  switches on the first byte `'B'`..`'Q'`: restart session (`'D'`), start (`'E'`), send a car to the pits (`'K'`,
  `'L'`, `'O'`), gentle stop (`'P'`), skip session (`'Q'`), others not decoded. Interpretation: remote control for
  event rigs.

## 4. What it reads from disk

| Path | Read by | Use |
|---|---|---|
| `cfg/race.ini` (Documents) | `RaceManager::loadSessions`, `ACClient::handshakeTCP` | `[REMOTE] ACTIVE, SERVER_IP, SERVER_PORT, SERVER_HTTP_PORT, SERVER_NAME, NAME, TEAM, PASSWORD, REQUESTED_CAR`; `[CAR_0]`. |
| `system/cfg/assetto_corsa.ini` | `ACClient::ACClient`, `NetCarStateProvider` constructor | `[DEBUG] MULTI_LOG` (write `Documents/Assetto Corsa/logs/aclient.log`). |
| `system/cfg/options.ini` | `ACClient::ACClient` | `[OPTIONS] IGNORE_RESULT_TELEPORT`. |
| `cfg/client.ini` (Documents) | `ACClient::ACClient` | `[CLIENT] USE_TV_MODE`. |
| `content/cars/<car>/data.acd` or `data/` | `ACClient::getCarMD5` | MD5 of the player's car data. |
| any path sent by the server | `ACClient::handshakeTCP` | MD5 checksum list. |
| `content/cars/<car>/collider.kn5` | `NetCarStateProvider` constructor | Collision mesh of a remote car. |
| `content/cars/<car>/data/{suspensions,tyres,car,aero,drs,engine}.ini` | `NetCarStateProvider::initPhysicsValues` 0x140119fe0, `initWings` 0x14011aa80, `initDRS` 0x140119a60, `initP2P` 0x140119d30 | Only geometry for display: `[BASIC] WHEELBASE, CG_LOCATION`, `[FRONT]/[REAR] TRACK, BASEY`, tyre `RADIUS`, `GRAPHICS_OFFSET`, `GRAPHICS_PITCH_ROTATION`, wing angles and controllers, DRS wings, push-to-pass times, rev limiter. |

Nothing is written except the optional log and setups received from the server.

## 5. Size

`python tools/re_query.py size ACClient ACClientVotingManager ACChat NetCarStateProvider RemoteCarStateProvider
IPAddress TCPSocket UDPPacket UDPSocket ksNetwork RemoteTelemetryUDP UDPCommandListener QOSOverlay`

| Object file | Functions | Code bytes | | Object file | Functions | Code bytes |
|---|---|---|---|---|---|---|
| ACClient.obj | 210 | 128,330 | | UDPPacket.obj (ksNetwork.lib) | 16 | 2,050 |
| NetCarStateProvider.obj | 37 | 30,973 | | UDPSocket.obj (ksNetwork.lib) | 6 | 1,994 |
| ACChat.obj | 27 | 9,057 | | IPAddress.obj (ksNetwork.lib) | 2 | 153 |
| ACClientVotingManager.obj | 27 | 7,529 | | ksNetwork.obj (ksNetwork.lib) | 3 | 125 |
| RemoteTelemetryUDP.obj | 14 | 5,293 | | QOSOverlay.obj | 3 | 728 |
| UDPCommandListener.obj | 18 | 3,384 | | RemoteCarStateProvider.obj | 7 | 469 |
| TCPSocket.obj (ksNetwork.lib) | 14 | 2,345 | | | | |

**Total: 384 hand-written functions, 192,430 code bytes** (13 object files).

Not counted here: `NetMonitorForm.obj` (10 functions, 2,541 bytes) and `FormChat.obj` (36, 22,123) are
SystemApps.lib forms (UI area); `RaceControlVoting.obj` and `RaceControlSessionServer.obj` are pit-screen UI;
`MD5CheckSum.obj` is counted in `config.md`; `SteamInterface.obj` (66 functions, 38,068 bytes) is achievements
and leaderboards; `RaceManager::initOnline` lives in `RaceManager.obj` (session logic).

## 6. Port difficulty

**L.** The transport and the dead reckoning are small; the bulk is `ACClient` (128 KB), which is session logic
spread over thirty message handlers and tightly coupled to race management, leaderboards, penalties, the pit
screen and the physics thread.
- Depends on: physics engine events and clock (`evOnStepCompleted`, `physicsTime`, session start time), the
  rigid-body layer (kinematic collision bodies for remote cars, `docs/map/physics_engine.md`), track ray casts,
  `CarAvatar` and its state-provider interface, `RaceManager` / sessions, system messages and UI, ini reader,
  MD5 of files and of `data.acd`.
- To stay compatible with existing servers the byte layouts and ids must be reproduced exactly, including the
  protocol version 202 and the checksum behaviour. Open-source community server implementations exist and can be
  used to cross-check packet layouts.
- Off-the-shelf: sockets from `std::net` (non-blocking, polled once per physics step exactly like the original)
  or `mio`; `md-5` crate for digests; `byteorder` or hand-written cursor for packets; `steamworks` crate if a
  Steam id is wanted. No third-party SDK is linked for networking (plain Winsock).
- `RemoteTelemetryUDP` and `UDPCommandListener` are each an afternoon of work and independent of the rest.

## 7. Open questions

- Field-exact layouts were decoded only for the incoming car update; handshake request string order, the car-list
  entry, lap / split / result / session messages and the outgoing car update still need a line-by-line read.
- Status bit field of the car update: bits 0x20, 0x40, 0x100, 0x200 are tested in `NetCarStateProvider::step`
  (0x200 = KERS charging); the full bit list was not decoded.
- The handshake flag stored in `Sim::useMousePitstop` may be a different server option (the summary printed by
  `RaceManager::initOnline` mentions "Virtual Mirror: Forced On"); not resolved.
- `remoteSpring`, `remoteDamper`, `remoteFactor`: three named tunables are set up in the `ACClient` constructor;
  only `remoteFactor` was seen in use. Where their values come from was not traced.
- How the remote collision body is driven (set position and velocity every step versus a spring towards the target)
  and what `PhysicsEngine::addAdditionalPhysicsProvider` adds was not read in detail; this matters for contact feel.
- `SERVER_HTTP_PORT` is read into `serverInfo.httpPort`, but no HTTP request code was found in the client.
- `ACP_AUTH_FAILED` exists as a reply, yet the client sends only the Steam id string: any ticket check must happen
  between the launcher / server and Steam, outside `acs.exe` (interpretation).
- `UDPCommandListener`: whether it is always active or gated by a setting, and the full command list.
- `RemoteTelemetryUDP` operations 4 and 5, and the exact `RTCarInfo` field list (328 bytes; see
  `re/types/RTCarInfo.txt`).
- Choice made: SystemApps forms and the pit-screen voting / server screens are left to the UI area;
  `SteamInterface` is left to the miscellaneous area.
