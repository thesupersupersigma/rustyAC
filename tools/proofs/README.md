# Proofs

Every proof the reports claim reruns from here, one area at a time or all at once:

    bash tools/proofs/run_all.sh --quick            # every area on a small sample (tools/proofs/quick/), ≈ 30 min
    bash tools/proofs/run_all.sh --changed v0.38.4  # the areas the files changed since a tag ask for, every case
    bash tools/proofs/run_all.sh [--full]           # every area, every case, against the game's kept results (milestones), ≈ 4.5 h
    bash tools/proofs/run_all.sh --rerecord         # the same, and the game's results made again and held against the kept ones
    bash tools/proofs/run_all.sh gui net            # only these areas (with any of the above)
    bash tools/proofs/render.sh                     # one area by itself (prints PROOF <area> cases= identical= different= errors= skipped= expected=)

Each task runs `--quick` and `--changed <the previous tag>`; `--full` is for milestones.
A crash, out of memory or error exit is ERROR, not DIFFERENT: run_all.sh runs each ERROR case again
alone at the end, and with less than 12 GB free it runs half the lanes (lib.sh's `room` holds a heavy
case until 6 GB are free). A case the game cannot run on purpose is listed in `expected_skips.txt`
with its reason and counted as an expected skip, not red; any other skip is red.
`--dry-run` only says which areas would run (and, with `--changed`, why: each changed file and
the rule of `areas.map` it matched; a file no rule matches runs every area; a `Cargo.toml` or
`Cargo.lock` whose only change is rustyAC's own version asks for nothing; any other `Cargo.lock`
change asks for the areas of the crates it reaches, `lock_crates.py`, or every area when it
can't tell). `bash tools/proofs/areas_test.sh` checks the rules on made-up lists of files.

No proof reads or writes a player's folders (Task 46c): every rustyac.exe run goes through lib.sh's
`rustyac <exe> <args>` (fresh `--cfg-dir` and `--documents` under `$out/own/`), files a player keeps
come from `re/gold/docs/`, rustyac.exe refuses to start under lib.sh's `RUSTYAC_PROOF_RUN=1` without
both options, and an area whose run changed `target/release/cfg` (or the rest beside the exe) is
DIFFERENT. docs/port/proofs.md lists the files.

The game's recordings that `fresh` compares against are made once and kept (xz) in
`re/gold/fresh/rec/<name>/<key>/` (the key covers the oracle's arguments, the files they name, the
source of `tools/car_oracle` and the install: `acs.exe`'s bytes and the name, size and time of every
file of `system/`, `content/cars/`, `content/tracks/`; a change to any of them records again).
`--quick`, `--changed` and `--full` compare against those; `--rerecord` records every one again and
counts it DIFFERENT if its bytes are not the kept ones,
apart from the game's heap address (the `bounce_vel` the game never writes into its mesh contact
joints: the header lines `mesh_bounce_vel` / `mesh_geom_high`, and in a several-car recording the
words `world.contactHash` / `world.meshBounceVel` of a step where that word is each run's own
address; `same_rec.py`) and the port's own `*.result.txt` (nothing kept is ever replaced; below 18 GB of free disk, `PROOF_KEEP_MIN_GB`, nothing new is
kept and those cases are recorded on the spot as before). The port's crates are not in the key: a
port change can only turn a kept result's case DIFFERENT, never hide one. `dumps` builds the last
release's rustyac.exe once per tag into `target/proofs/old_exe/<tag>/` and keeps its dump of each
drive in `re/gold/dumps/` (keyed on that exe's bytes, the drive and the install; `--rerecord`
dumps again and holds it against the kept one).

Kept results whose key today's sources no longer make (an older car oracle, an older install) are
never compared again. `archive_kept.sh` lists them with their size (it asks `fresh.sh` for today's
keys with `PROOF_LIST_KEYS`, nothing is built or run); `archive_kept.sh --move` sends them to the
server (`ARCHIVE_DEST`, default `thesupersupersigma@10.0.0.140:/storage/rustyac-archive/kept/`, a
folder per day; tar over ssh), checks every file's SHA-256 on both sides and only then removes them
here. Today's keys stay, so `fresh` records nothing again after a move.

Every program more than one area runs (the four oracles, rustyac.exe, replay_hash, track_survey,
rustyac-launcher) is built once before any area starts, with 8 jobs, so no area relinks an exe
another lane is running. Then four lanes run side by side, the areas of a lane one after the other:
`render gui python` (the render oracle), `fresh sound replays launcher` (the car oracle and the
other game-side tools), `physics dumps shaders` and `net browser ai_page` (never load the game's
code); each lane builds with 2 jobs. Each area writes `times.tsv` (seconds per case). A case that
cannot run is never dropped: the area counts it as skipped with its reason, and the run prints
every skipped case after the table (in red when there is one) and in `target/proofs/skipped.txt`.
A case the last run of the same mode had and this one has not is printed as vanished (red, and in
`target/proofs/vanished.txt`; each area's list is kept in `target/proofs/cases/<mode>/`); a case
retired on purpose goes in `tools/proofs/retired.txt` with its reason.

What a run needs: this repository, the Assetto Corsa install (read only; `AC_ROOT` if it is not the
Steam default), the ignored `oracle/` folder (the saved game recordings of Tasks 9 to 34) and the
ignored `re/gold/` folder (what was re-made in Task 38c, see `re/gold/README.md`). Nothing in
`re/scratch/` is read. Output goes to `target/proofs/<area>/` (`PROOF_OUT` to move it); each area
deletes its own output before it starts. One render-oracle area at a time: two render compares at
once make the render oracle's game side flaky (the render oracle and the car oracle side by side
are fine: Task P1's full runs).

| Area | Script | What is compared | Inputs |
|---|---|---|---|
| physics | `physics.sh` | the saved one-car recordings (`chassis_compare run --dir oracle/car*`) and several-car recordings (`car_oracle multi-check` at 0, 1, 8 threads) | `oracle/` |
| fresh | `fresh.sh` | the game's recordings (kept, or made when their key is new) against the port: the F2004's scenarios, conditions, 4WD, KERS/ERS, suspensions, contacts, the road under the wheels on every track, the referee's micro drives, race.ini, the several-car runs of `multi_fresh.txt` | AC, `re/gold/fresh/` |
| sound | `sound.sh` | the audio oracle's 18 tapes (game's FMOD calls against the port's), the DSP check, three drives end to end | `oracle/audio*`, `oracle/game` |
| render | `render.sh` | the 107 frames and sequences of `render_list.txt` (Tasks 20 to 23), call log and pixels | `oracle/audio`, `oracle/render_tapes` |
| gui | `gui.sh` | the 122 GUI sequences of `gui_list.txt` (Tasks 29 to 34) | `cardata/`, `oracle/t32` |
| python | `python.sh` | AC's Python apps (Task 42): `py_quick`, `py_apps`, the game's PythonInterface and rustyac-python on the same apps | `oracle/t32`, the player's `system/x64/python33.dll` |
| replays | `replays.sh` | the replay, ghost and telemetry sessions of `replay_list.txt`, a player's replays (the kept copy re/gold/docs), rustyac.exe's replay at 0, 1, 8 workers | AC, re/gold/docs |
| net | `net.sh` | the 20 multiplayer goldens, four ways (`tools/net_gold/check.sh`) | `re/gold/net/` |
| browser | `browser.sh` | every drive of `oracle/game` desktop against wasm32-wasip1 (`replay_hash`); `BROWSER_DIGEST=1` adds `math_proof digest` | `oracle/game`, wasmtime |
| ai_page | `ai_page.sh` | Task 37c's Quick Race: desktop replay, native `race_proof`, the page in headless Chrome | `re/gold/ai_page/`, `re/gold/web/pack/` |
| shaders | `shaders.sh` | `dxbc_reflect`, `shader_table`, `shader_proof` on SwiftShader and WARP against Task 38b's tables | `re/gold/shaders/` |
| launcher | `launcher.sh` | the 12 request lists of `re/gold/launcher` (8 recorded from the click scripts of `launcher/` by `launcher_record.sh`, 4 written by hand), the game's own handlers (`tools/launcher_oracle`) against the port, answers and the player folder left behind | `re/gold/launcher/`, the .NET SDK |
| dumps | `dumps.sh` | every drive's state dump, the last release's rustyac.exe (kept dumps) against this one | `oracle/game`, `re/gold/dumps/` |

Not an area (no same-or-different verdict, a GPU and SwiftShader differ by rounding):
`render_wgpu.sh` (Task 38h) draws the cases of `render_list.txt` once more with the port on kgl's
wgpu backend and gives, per case, the kgl call log against the Direct3D 11 port's (must be
identical) and PSNR / largest difference against the game's WARP pixels, with side-by-side
pictures. `ADAPTERS`, `SWIFTSHADER`, `SEQ_EVERY` and `ONLY` are in its header.

The lists say for each line where it came from; where a lost script's exact values (a tape frame,
a `--set` frame) could not be read back from the reports they were chosen anew, so call counts can
differ from a report's numbers while the proof (game = port) is the same. `docs/port/proofs.md`
has the table of every scratch path the reports named and what replaces it.
