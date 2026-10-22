# Proofs

Every proof the reports claim reruns from here, one area at a time or all at once:

    bash tools/proofs/run_all.sh --quick            # every area on a small sample (tools/proofs/quick/), ≈ 30 min
    bash tools/proofs/run_all.sh --changed v0.38.4  # the areas the files changed since a tag ask for, every case
    bash tools/proofs/run_all.sh [--full]           # every area, every case, recordings made again (milestones)
    bash tools/proofs/run_all.sh gui net            # only these areas (with any of the above)
    bash tools/proofs/render.sh                     # one area by itself (prints PROOF <area> cases= identical= different=)

Each task runs `--quick` and `--changed <the previous tag>`; `--full` is for milestones.
`--dry-run` only says which areas would run (and, with `--changed`, why: each changed file and
the rule of `areas.map` it matched; a file no rule matches runs every area). `bash
tools/proofs/areas_test.sh` checks the rules on made-up lists of files.

The game's recordings that `fresh` compares against are made once and kept (xz) in
`re/gold/fresh/rec/<name>/<key>/` (the key covers the oracle's arguments, the files they name and
the source of `tools/car_oracle`). `--quick` and `--changed` compare against those; `--full` and
`--rerecord` record every one again and count it DIFFERENT if its bytes are not the kept ones
(nothing kept is ever replaced; below 18 GB of free disk, `PROOF_KEEP_MIN_GB`, nothing new is
kept and those cases are recorded on the spot as before). `dumps` builds the last release's rustyac.exe once per tag into
`target/proofs/old_exe/<tag>/`. The shared oracles are built once per run. The areas that never
load the game's code (net, browser, ai_page, shaders) run beside the others, which still run one at
a time; each lane builds with 4 jobs.

What a run needs: this repository, the Assetto Corsa install (read only; `AC_ROOT` if it is not the
Steam default), the ignored `oracle/` folder (the saved game recordings of Tasks 9 to 34) and the
ignored `re/gold/` folder (what was re-made in Task 38c, see `re/gold/README.md`). Nothing in
`re/scratch/` is read. Output goes to `target/proofs/<area>/` (`PROOF_OUT` to move it); each area
deletes its own output before it starts. One game-loading area at a time: the oracles load the game
into their own process, and two at once make the render oracle's game side flaky.

| Area | Script | What is compared | Inputs |
|---|---|---|---|
| physics | `physics.sh` | the saved one-car recordings (`chassis_compare run --dir oracle/car*`) and several-car recordings (`car_oracle multi-check` at 0, 1, 8 threads) | `oracle/` |
| fresh | `fresh.sh` | recorded again from the game and compared on the spot: the F2004's scenarios, conditions, 4WD, KERS/ERS, suspensions, contacts, the road under the wheels on every track, the referee's micro drives, race.ini, the several-car runs of `multi_fresh.txt` | AC, `re/gold/fresh/` |
| sound | `sound.sh` | the audio oracle's 18 tapes (game's FMOD calls against the port's), the DSP check, three drives end to end | `oracle/audio*`, `oracle/game` |
| render | `render.sh` | the 107 frames and sequences of `render_list.txt` (Tasks 20 to 23), call log and pixels | `oracle/audio`, `oracle/render_tapes` |
| gui | `gui.sh` | the 122 GUI sequences of `gui_list.txt` (Tasks 29 to 34) | `cardata/`, `oracle/t32` |
| replays | `replays.sh` | the replay, ghost and telemetry sessions of `replay_list.txt`, the user's own replays (read in place), rustyac.exe's replay at 0, 1, 8 workers | AC, Documents (read only) |
| net | `net.sh` | the 20 multiplayer goldens, four ways (`tools/net_gold/check.sh`) | `re/gold/net/` |
| browser | `browser.sh` | every drive of `oracle/game` desktop against wasm32-wasip1 (`replay_hash`); `BROWSER_DIGEST=1` adds `math_proof digest` | `oracle/game`, wasmtime |
| ai_page | `ai_page.sh` | Task 37c's Quick Race: desktop replay, native `race_proof`, the page in headless Chrome | `re/gold/ai_page/`, `re/gold/web/pack/` |
| shaders | `shaders.sh` | `dxbc_reflect`, `shader_table`, `shader_proof` on SwiftShader and WARP against Task 38b's tables | `re/gold/shaders/` |
| launcher | `launcher.sh` | the 12 request lists of `re/gold/launcher` (8 recorded from the click scripts of `launcher/` by `launcher_record.sh`, 4 written by hand), the game's own handlers (`tools/launcher_oracle`) against the port, answers and the player folder left behind | `re/gold/launcher/`, the .NET SDK |
| dumps | `dumps.sh` | every drive's state dump, the last release's rustyac.exe against this one | `oracle/game` |

The lists say for each line where it came from; where a lost script's exact values (a tape frame,
a `--set` frame) could not be read back from the reports they were chosen anew, so call counts can
differ from a report's numbers while the proof (game = port) is the same. `docs/port/proofs.md`
has the table of every scratch path the reports named and what replaces it.
