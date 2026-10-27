# Task 43: player-facing leftovers, and pack updates by delta

## Resume here

- Done: step 1 (`web_pack --delta`), step 4 (votes and chat on the wire), step 6 (every skin's
  suits in packs), step 7 (refusals in plain words).
- Next: step 2 (`race_out.json` after an online session on desktop), then steps 3, 5.
- Scratch: `re/scratch/task43/`. Rerun scripts: `tools/web_checks/task43/`.
- Disk at the start: 67 GB free.

## Step 1: `web_pack --delta <server manifest>`

The pack on the server is about 28.6 GB, too big for a second copy on this PC. `--delta` builds the
same pack as before, but it only writes files the live pack doesn't have yet. A file counts as
unchanged when it has the same path, size and SHA-256 as in the live `manifest.json`; such a file
is read for its hash but not written. The new full `manifest.json` goes with the changed files.
The tool prints how many files changed and how big they are, and which files of the live pack the
new pack no longer has. `--out` must be an empty folder, so it holds exactly the upload.

How to use it (the pack on the server under `/srv/rustyac/preview`, for example):

```sh
scp me@server:/srv/rustyac/preview/manifest.json live_manifest.json
cd tools/web_pack && cargo run --release -- --out ../../dist-web/delta --delta ../../live_manifest.json
# prints: delta: N new or changed files, X MB (plus manifest.json) in ../../dist-web/delta ...
#         the new pack no longer has M files of the live pack (...): delete them there
#           gone: content/tracks/...
scp -r ../../dist-web/delta/. me@server:/srv/rustyac/preview/
# then delete the "gone:" files on the server (optional: the page never asks for them)
```

`--config`, `--ac` and `--dry-run` work with `--delta` as without it (`--dry-run` writes nothing).

Proof (`sh tools/web_checks/task43/s1/delta.sh`, output in `re/scratch/task43/s1`): an old pack
(F2004 at Laguna Seca and Magione) and a new one (F2004 and Z4 at Laguna Seca: a car more, a track
less). One file of the old pack (`content/cars/ks_ferrari_f2004/sfx/ks_ferrari_f2004.bank`) is
changed, both its bytes and its hash in the manifest, so the delta has to carry it again. Then:
old pack + delta copied over it − the "gone" files = the server's update. That result is compared
with `diff -r` against the full new pack.

| Pack | Files | Size |
|---|---|---|
| old (full) | 1871 + manifest | 1451.4 MB |
| new (full) | 1926 + manifest | 1234.4 MB |
| delta | 100 + manifest | 179.5 MB |
| gone (to delete on the server) | 44 | 319.8 MB |
| old + delta − gone vs new, `diff -r` | 1927 vs 1927 | **identical** (the changed `.bank` is in the delta) |

Test: `a_manifest_reads_back` (`tools/web_pack`): a manifest written by `Manifest::to_json`, with
`"`, `\` and a control character in its paths, reads back with every path, size and hash.

## Step 4: votes and chat on the wire

`car_oracle net <tape>` now also hands every chat line and vote rustyAC sent on the tape to the
game's own `ACClient::sendChat` @ 0x140052a90 / `sendVote` @ 0x140052ba0 (on the in-process
`ACClient`, with the stand-in sockets) and compares what the game sends, byte for byte, with what
rustyAC sent (`NetGame::send_as_game` in `tools/car_oracle/src/game/net.rs`). A kick vote's target
goes in as the target car's avatar guid, as the game's console and the vote window give it, and
the game's `getSessionIdFromCarAvatarID` turns it back into a slot.

The tape: a live session on 127.0.0.1 (acServer, Laguna Seca), rustyac.exe "a" taped and "b";
a says three lines (short, long, not ASCII) and votes seven times
(`sh tools/web_checks/task43/s4/votes.sh`):

| message | count | game vs rustyAC |
|---|---|---|
| chat `0x47` (`hi`, a 36-character line, `ciao, è già`) | 3 | identical |
| vote next session `0x64` (yes, no) | 2 | identical |
| vote restart `0x65` (no, yes) | 2 | identical |
| vote kick `0x66` (a driver not there: target 0xff; b: slot 1) | 2 | identical |

acServer took them: its log has the three lines, "Creating vote to NEXT SESSION from a", the
answers, and "ACP_VOTE_KICK_USER failed, cannot find car with session id= 255" for the kick of
nobody. The rest of the same run (200 steps, net_watch) stayed identical. The three goldens' tapes
(`rich_*`) carry one chat line each, which the quick proofs' net runs now also compare.

## Step 6: every skin's suits in packs

A pack now holds every skin's `skin.ini` and the driver's and crew's suit, gloves, helmet and
brand folders it names (`content::suit_files` in `crates/rustyac-web/src/content.rs`, called by
`tools/web_pack` for each skin folder of each car). The skins' own textures stay out, as before:
a car in another skin is drawn with the model's own livery, but its driver and crew now wear that
skin's suits. The page itself is unchanged: it fetches a suit only when the renderer reads it.

Preview pack (`web/preview.toml`, `--dry-run` before / after):

| | before | after |
|---|---|---|
| shared files | 1234, 140.7 MB | 1301, 182.4 MB |
| bmw_z4_gt3 | 49 files listed, 121.2 MB | 57 files, 86.0 MB |
| ks_ferrari_f2004 | 54 files listed, 179.8 MB | 51 files, 136.1 MB |
| total | 1404 files, 1743.1 MB | 1482 files, 1784.8 MB |

The per-car lines were too big before: a file both the drive and the picture read was counted
twice there (the total and the manifest never had duplicates). web_pack now counts each file once.
`--delta` of the new pack against the old one: 78 files, 41.7 MB: the Z4's 11 other `skin.ini`
and 67 suit files (crew helmets in six colours, `crew_suit/type1`, two glove and six helmet
folders, `driver_suit/sparco`). The F2004's skins name no other suits. For all 207 cars of this
install the suit folders grow from 77 (84.9 MB, first skins) to 139 (189.4 MB, every skin)
(`re/scratch/task43/s6/suits.py`, which reads the skins' `skin.ini` outside `data.acd`).

Page check (`sh tools/web_checks/task43/s6/suits.sh`): the Z4 in its `fanatec` skin at Laguna
Seca, Full mode, `window.rustyac.driveRace`, from the old pack and then the same pack with the
delta copied over it:

| pack | suit files fetched | fanatec's own (`skin.ini`, red suits, `arrow_rg7` gloves, white helmet, red crew) |
|---|---|---|
| old | 18 (the first skin's) | 0: the driver and crew in the first skin's black |
| new | 26 | 8: `skin.ini`, then its 7 suit files; the screenshot shows the red driver and crew |

Not changed:
- The player's own `Documents\Assetto Corsa\cfg\driverskin.ini`: only acs.exe reads it
  (`DriverModel::DriverModel` 0x1400f76d0); neither AC's launcher (`AssettoCorsa.exe`, its theme)
  nor rustyAC's page writes one, and the page's Documents hold only `cfg/video.ini` for the
  renderer. A pack built from one PC's `driverskin.ini` would give every visitor that PC's
  suit, so it stays out.
- The Python car values in the page: the page runs no Python apps (they need the player's
  `python33.dll`, `docs/port/python_apps.md`), so nothing would read them. The doc comment in
  `crates/rustyac-web/src/full.rs` that said rustyac.exe had none either is fixed.
- The page still fetches the first skin's suits at the start of every Full drive (about 15 MB
  for the Z4), whichever skin is drawn.

## Step 7: "Session is closed to new connections"

It is acServer's rule, not rustyAC's. The same race-only server (Laguna Seca, `WAIT_TIME=20`) was
run twice, only `[RACE] IS_OPEN` differing; desktop "a" joined 4 s after the server started,
desktop "c" and the page "b" 45 s after (`sh tools/web_checks/task43/s7/closed.sh 2`, then `1`):

| `[RACE] IS_OPEN` | desktop a | desktop c | page b |
|---|---|---|---|
| 2 | `ERROR :: Session is closed to new connections` | the same | the same (plain words shown) |
| 1 | let in, slot 0 | let in, slot 2 | let in, drives |

The answer is the server's handshake byte 0x6e (`id::SESSION_CLOSED` in
`crates/rustyac-net/src/handshake.rs`), which the game turns into that text. With a race-only
server and `IS_OPEN=2` joins were refused already 4 s after the server started (it starts in the
race's countdown); exactly when acServer closes is not checked further.

The page now says what each refusal means and keeps the game's text in a small grey line below
(`PLAIN_WORDS` in `web/app.js`; one table for the join refusals of `handshake.rs` and the kick
reasons of `online.rs`): closed session, wrong password, full, still booking, banned / blacklisted,
voted out, checksum failed (files not the server's), other protocol, auth failed, no answer.
`window.rustyac.error` stays the game's text. The panel is unchanged. Page b's text at IS_OPEN=2:
"This race has already started and the server doesn't let new players join during it. Try again
when the next session starts." / "ERROR :: Session is closed to new connections".
