# Every proof again (Task 38c)

## Resume here

Every proof the reports claim reruns from committed scripts in `tools/proofs/`, with recordings in
the ignored `oracle/` (saved game recordings of Tasks 9 to 34, as they always were) and the ignored
`re/gold/` (what Task 38c re-made or moved out of scratch). Nothing reads `re/scratch/`.

    bash tools/proofs/run_all.sh --quick            # every area on a sample, ≈ 25 min (Task 38f)
    bash tools/proofs/run_all.sh --changed v0.38.4  # what the files changed since a tag touch
    bash tools/proofs/run_all.sh                    # (= --full) every area, every case; table in target/proofs/table.md
    bash tools/proofs/run_all.sh gui                # one area
    RENDER_ONLY='r21_.*' bash tools/proofs/render.sh    # part of one area

`tools/proofs/README.md` says what each area compares and needs; `re/gold/README.md` how each
file under `re/gold/` was made and how to make it again.

## Task 38f: fast proofs (v0.38.5)

A full run took about 10 hours, too long for every task. Now:

- **`--quick`**: every area on a small fixed sample (`tools/proofs/quick/<area>.txt`, one regex a
  line over each area's case names): one-car and several-car recordings with AI, the referee, a pit
  case, a jump start, contacts, a few tracks, crashes, 8 referee micro drives, 4 of the 20 net
  goldens (one of each kind) four ways, two render frames per task, 15 GUI sequences across Tasks
  29-34, 6 browser drives and 6 dumps, one launcher list, every shader check, 4 sound cases.
  `ai_page` and `shaders` are small enough to run whole. 23 minutes.
- **`--changed <tag>`**: the areas the files changed since `<tag>` (committed or not) ask for,
  each run whole. `tools/proofs/areas.map` maps paths to areas (first match wins); a path no rule
  matches runs everything; a `Cargo.toml` / `Cargo.lock` whose only change is rustyAC's version asks
  for nothing. Another `Cargo.lock` change asks for the rules of the crates whose dependencies
  changed or reach a changed outside package (`tools/proofs/lock_crates.py`, Task 38i), every area
  when it can't tell. It prints each file, the rule it matched and the areas. `areas_test.sh`
  checks 20 made-up lists and two tag-to-tag locks.
- **`--full`** (also the default): every case of every area, as before, plus the game's recordings
  made again and held against the kept ones. Per-area time and the wall clock in the table.

What makes the slow areas cheaper without weakening them:

- `fresh` records each game recording once into `re/gold/fresh/rec/` (xz, keyed on the oracle's
  arguments, the files they name and `tools/car_oracle`'s source) and compares the port against
  that copy. `--full` and `--rerecord` record again and count a case DIFFERENT unless the new bytes
  equal the kept ones (`same_rec.py`). The only allowed difference is the game's heap address. The
  game never writes `bounce_vel` into its mesh contact joints, so they take the upper half of a
  geom's address, which changes every run. It shows in the header lines `mesh_bounce_vel` /
  `mesh_geom_high` and, in several-car recordings, in the row words `world.contactHash` /
  `world.meshBounceVel` of the steps where a contact bounces (that word must be each run's own
  address). The port is fed the value from the recording it compares against, as it always was.
  Found because the first re-recording of 47 cases gave 10 such differences: 7 in the header only,
  3 (four-car contact runs) in those two words too.
- The areas that never load the game (net, browser, ai_page, shaders) run beside the others, which
  still run one at a time; each lane builds with 4 jobs. The shared oracles and rustyac.exe /
  replay_hash are built once per run; `browser` runs replay_hash once a drive (it was twice);
  `dumps` builds the last release's exe once per tag into `target/proofs/old_exe/<tag>/`.
- `chassis_compare game-replay` rewrites a drive in `oracle/game` only when its bytes change, so
  `browser` can read the folder while `fresh` runs.
- The release workflow marks a release latest only when its tag is the highest version.

| Area | Full before (Task 38c) | Full now | Quick now (cases) |
|---|---|---|---|
| physics | 32 min | 37 min | 0.5 min (9) |
| fresh | about 2 h 50 min | 3 h 02 min | 5.6 min (47) |
| sound | 4 min | 3 min | 0.3 min (4) |
| render | 39 min | 40 min | 3.4 min (10) |
| gui | 112 min | 70 min | 8.3 min (15) |
| replays | 43 min | 45 min | 2.8 min (5) |
| net | 17 min | 21 min (beside) | 3.5 min (16, beside) |
| browser | 78 min (50 min in Task 38d) | 42 min (beside) | 0.9 min (6, beside) |
| ai_page | 8 min | 8 min (beside) | 6.7 min (3, beside) |
| shaders | 1 min | 1 min (beside) | 0.7 min (4, beside) |
| launcher | 5 min | 5 min | 0.6 min (1) |
| dumps | 33 min (156 drives) | 71 min (223 drives) | 2.5 min (6) |
| **wall clock** | **about 10 h** | **7 h 32 min** | **23 min** |

The full run (on the final code, nothing else building): 1,601 cases, all identical, 0 different.
That includes `fresh` 657 (Task 38c's 656 plus one, with `pit_mouse` seed 2 identical this time)
and `dumps` against v0.38.4's exe, 223 drives byte-identical. 25 kept recordings were made again
and matched their kept bytes; 95 were not kept because free disk fell below 18 GB
(`PROOF_KEEP_MIN_GB`), so `fresh` records those on the spot, as before, until there is room. A
quick run with the kept recordings: 126 cases, all identical, 23 minutes (32 minutes the first
time, when it records them). `fresh` in a full run is a little slower than before (it compresses
and compares what it records). `dumps` is slower because `oracle/game` has 67 more drives since
Task 38c. `--changed v0.38.3` on this tree picks every area, correctly, because this task changed
`lib.sh` and `run_all.sh`, which every area uses. For the files of the v0.38.4 release alone it
picks `net browser ai_page` (the server panel and `web/app.js`; the lock files changed only the
version).

Clippy `-D warnings` is clean natively (`--all-targets`), for wasm32-unknown-unknown, for
wasm32-wasip1 and with the wgpu feature, and every `--locked` build passes, the tools' own lock
files included.

`re/gold/fresh/rec` is 4.9 GB. 1.2 GB of it (45 `.gz` folders) are the first quick run's recordings,
kept under the key used before the oracle's lock file left the key. Nothing reads them, and they can
be deleted (done in Task 38g: `re/gold/fresh/rec` is now 3.8 GB). Free disk: 27 GB at the start, 17 GB at the end.

## Why

The disk cleanups of 2026-10-15 (`re/scratch/task20` to `task33b`) and 2026-10-18 (`task34` to
`task37c`) deleted the scratch folders, and with them the batch scripts, several recordings, the
launcher's request lists and the lists of GUI sequences. The reports still pointed into them 334
times. Task 38b's end checks could then only rerun what was left: 4 frames and 2 sequences of the
render oracle, 60 GUI sequences, 30 launcher requests.

## What was done

1. **Audit** (table at the end): every `re/scratch/task…` path named in `docs/` and `tools/`, what
   it was, whether it is still there, whether a proof needs it.
2. **Brought back**, as lists and scripts in `tools/proofs/` (committed):
   - `render_list.txt`: 107 frames and sequences of Tasks 20 to 23, from the reports' tables;
   - `gui_list.txt`: every GUI sequence name of Tasks 29 to 34 (with the setup cars, the `@short`
     and `#lang` variants);
   - `multi_fresh.txt`, `survey_list.txt`, `replay_list.txt`: the several-car, every-track and
     replay runs whose recordings lived only in scratch, re-recorded from the game on each run;
   - the launcher's request lists, re-recorded from the port's window with click scripts and
     answered by the game's own handlers (`tools/launcher_oracle`);
   - the inputs of Task 37c's page proof, the SwiftShader driver, the test pack, Task 15's TC4
     setup and Task 27's version-6 fast lane, moved or re-made into `re/gold/`.
3. **`tools/proofs/run_all.sh`**, one script per area.
4. **The rc_jump difference of Task 38b** (below): a wrong recording, and behind it a port bug, fixed.
5. **Docs**: every "how to rerun" scratch command now names its `tools/proofs` command, the old
   path kept after it as "(was …, deleted)"; each report with scratch paths opens with a note that
   they are history. `docs/release.md` has the proof step.

## rc_jump at step 2717

Task 38b's end check ran `car_oracle multi --scenario rc_jump --track magione` without `--cars`,
so four F2004s (the scenario's default) raced; Task 28 recorded and proved rc_jump with four BMW
M3 E30s (`race_rules.md`: "cars: BMW M3 E30 unless said otherwise"; `oracle/t28/f_jump`, 862
values a step). Re-recorded today with Task 28's cars it is identical at 0, 1 and 8 threads, every
one of 34,480,000 values, race_out.json identical (2,106 bytes), the same contact step (27,939) and
damage as Task 28's own result. So the difference was the wrong recording, and the proof scripts now
always pass the report's cars (`multi_fresh.txt`).

With the scenario's own F2004s the port did differ, and that was a port bug: at step 2717 the AI
of car 2 kept `desiredOffset` at 0 where the game has -6.7392488. Car 0 jump-starts and the game's
`Car::stepJumpStart` puts it into its pit box in the middle of its own step, before car 2's driver
looks at the cars around it; the port asked every AI driver first and ran the heavy half of each
car's step (where the jump start is checked) after, so car 2 still saw car 0 on the grid, 14 m
ahead, and did not move over. (With the E30s car 2's driver, which looks only every 5th step, never
looked in the one step that mattered.) The fix (`engine/mod.rs` `step_cars`,
`chassis.rs` `jump_start_possible`): while a jump start can still put the player's car into its
box, the cars are stepped one after the other, as for a black flag. The F2004 recording is now
identical at 0, 1 and 8 threads (40,000 steps, 30,320,000 values, race_out.json 2,339 bytes), the
E30 one still is, and every saved recording and every other proof below is unchanged; it is in
`multi_fresh.txt` as `rc_jump magione - 40000`.

## The areas today

One run of each area (`sh tools/proofs/run_all.sh <area>`), on the final code. The PC went down
three times during the runs (a power problem, not the work), so some areas ran more than once; the
table has each area's last complete run, and `fresh` is the sum of two runs that together cover
every part once (`SURVEY_FROM` finished the survey).

| Area | Cases today | Identical | Different | Time | What the reports claimed |
|---|---|---|---|---|---|
| physics | 113 | 113 | 0 | 32 min | every saved recording: 49 one-car scenarios of `oracle/car*` and 64 several-car recordings of Tasks 24-34 (Task 38b checked 5 of the latter) |
| fresh | 656 | 655 | 1 (see below) | about 2 h 50 min | see the next table |
| sound | 22 | 22 | 0 | 4 min | Task 19's 18 drives, the DSP check and 7 drives end to end (3 here: the other 4 drives' .ryin were lost) |
| render | 107 | 107 | 0 | 39 min | 113 frames and sequences of Tasks 20-23 (38b: 6) |
| gui | 123 | 123 | 0 | 112 min | 103 sequence runs by Task 33b, 82 in Task 37b (38b: 60) |
| replays | 34 | 34 | 0 | 43 min | 14 replay, 6 ghost and 6 telemetry sessions, 12 of the user's replays (7 there today; the game rotates them), rustyac.exe at 0, 1, 8 workers |
| net | 80 | 80 | 0 | 17 min | 20 goldens four ways = 80 |
| browser | 223 | 223 | 0 | 78 min | 158 drives (156 were left; the fresh survey and crash runs added 67) |
| ai_page | 3 | 3 | 0 | 8 min | Task 37c's Quick Race: desktop, native, page |
| shaders | 4 | 4 | 0 | 1 min | dxbc_reflect, shader_table 72 of 72, shader_proof on SwiftShader and WARP |
| launcher | 12 | 12 | 0 | 5 min | 12 lists, 21,371 requests (Task 37b); today 12 lists, 15,888 requests |
| dumps | 156 | 156 | 0 | 33 min | every drive's states against v0.38.1 (38b: every 4th) |

`fresh`, by part (recorded from the game on the spot, compared, deleted):

| Part | Identical | Claimed by the reports |
|---|---|---|
| F2004 flat road (Tasks 9-13) | 11 | 11 |
| conditions (Task 15) | 7 | 7 |
| 4WD (Task 16) | 24 | 24 |
| KERS / ERS (Task 16) | 44 | 44 |
| suspensions (Task 17) | 64 | 64 |
| contacts (Task 13) | 9 | collide on Spa and 8 worlds |
| the road under the wheels (Tasks 14, 18) | 52 | 44 layouts that load (10 refused, as in Task 18, are not cases) |
| every-track survey (Task 18) | 142 | the lists of Task 18 (one trk_run replay is skipped by `game-replay` itself: the script calls the car's lock functions) |
| crashes (Task 22b) | 10 | 5 cars, recording and rustyac.exe's replay |
| the referee's micro drives (Task 28) | 200 | 201 (the seed/car pairs were in a lost script; 25 seeds x 8 lists now) |
| race.ini (Tasks 24b, 28) | 61 | every saved race.ini |
| several cars (Tasks 27, 28, 28b, 34) | 31 of 32 | the reports' tables: passing, pit stops, pit windows, pit feed seeds 1-8, rc_jump (E30 and F2004), the version-6 fast lane, mouse pit seeds 1-5 |

The one difference: `pit_mouse` seed 2 differed at step 584 in the referee's values once; the same
run recorded again five times was identical every time, as the saved Task 34 recording of it is
(open question 2).

`re/gold/` is 1.5 GB (the test pack 1.4 GB, the net goldens 59 MB, the rest small). Free disk:
32 GB at the start, 27 GB at the end.

## Not done

- Six render rows (Task 22: 8, 18-20; Task 22c: 5, 16) need made-up car data files that were lost;
  they are not in `render_list.txt`.
- Four sound drives end to end (e30 wall slide, f40, vrc, trk_lap): their `.ryin` files were lost.
- Task 36's lobby recording (9,000 servers) and the online launcher lists: they need the real lobby
  or a recording of it; not made again (the rules forbid real server requests).
- The Race Weekend tape of Task 37c (seed 31337): lost; only the Quick Race is in `re/gold/ai_page`.
- The `guid` launcher list: the oracle refuses `getguid` (Steam); `getguid` and `getachievements`
  are left out of every recorded list.
- Measurements (frame rates, timings, latency) and live online runs are not proofs that rerun to the
  same bits; their scripts were not rebuilt.

## Open questions

1. With `--telemetry` the several-car comparison itself differs at the first lap event (the
   game's referee values), in v0.38.1 too; the telemetry's own verdict is identical, which is what
   Task 28c claimed. Probably the oracle's hooks on the game's lap events; not chased.
   **Answered in Task 38d** (`docs/port/web_fixes.md`): the oracle replaced the game's whole
   `RaceManager::getLapCount` with car 0's count, so the referee read the wrong laps for every car;
   now only the recorder's own call is redirected, and the comparison is identical again.
2. `pit_mouse` seed 2 differed once at step 584 and not in five reruns: something on the game's
   side of that run is not repeatable (the mouse pit menu is driven by clicks; load?). Not chased.
3. The launcher's career list differed once in `utility/get` (the game's display modes came back
   three times); alone it was identical. The display's mode list changed during that run, probably a
   remote desktop connecting.
4. The render rows' tape frames and `--set` frames were chosen anew where the lost scripts had
   them, so call counts differ from the reports' while every row is identical.
5. `oracle/` stays where it is (15 GB, ignored, never in scratch) instead of moving into `re/gold/`,
   which would then be far over 5 GB. The fresh runs write their game-replay drives into
   `oracle/game` (67 new, a few rewritten), as Task 18 did.

## Every scratch path the reports named

One row per `re/scratch/task…` mention in `docs/` and `tools/` (334, from
`git grep -n "re/scratch/task" -- docs tools` before this task). "Still there?" is the disk on the
day of Task 38c; "A proof needs it?" says whether a rerun needs that file; "Mention" says whether the
doc line was history (left as it was) or a "how to rerun" command (now naming `tools/proofs`, the
old path kept after it). Lines of a report that point into a folder the cleanups deleted keep the
path: they say where the work was done, not where it is now.

### Physics oracle modes (63 references; now `tools/proofs/physics.sh, fresh.sh`)

| Doc line | Task | Scratch path | What it was | Still there? | A proof needs it? | Mention |
|---|---|---|---|---|---|---|
| rolling_chassis.md:8 | 08 | `re/scratch/task08/` (spec_bodies.md ...) | notes + scripts (fill_doc.py, q1-3.py, rec.py, patch*.py) | yes | no | historical |
| drivetrain.md:22 | 09 | `spec_controller_brakes.md`, `spec_engine.md` | spec | yes | no | historical |
| drivetrain.md:32 | 09 | `re/scratch/task09/` | notes + .asm listings + dispscan.py | yes | no | historical |
| whole_car.md:25 | 10 | `spec_aero.md`, `spec_aids.md`, `spec_car_step.md` | spec | yes | no | historical |
| whole_car.md:26 | 10 | `review_*.md` | notes | yes | no | historical |
| whole_car.md:36 | 10 | `re/scratch/task10/` | notes (332 entries) | yes | no | historical |
| whole_car.md:705 | 10 | `review_aero.md`, `review_aids.md`, `review_car_step.md` | notes | yes | no | historical |
| docs/game/first_drive.md:11 | 11 | `re/scratch/task11/` (spec_*.md three briefs) | notes/spec | yes | no | historical |
| docs/game/first_drive.md:257 | 11 | `re/scratch/task11/shm_idle.csv` | recording (shared-memory idle csv) + "unchanged script" | yes | yes | historical |
| docs/game/first_drive.md:344 | 11 | `spec_pad_keyboard.md` s10, `spec_wheel_ffb.md` s9 | spec (the sections list what unit tests cover) | yes | no | historical |
| conditions.md:15 | 15 | `re/scratch/task15/` (spec_conditions/spec_setups/spec_acd/spec_rende… | notes/spec; folder also holds recording scripts/ini | yes | no | historical |
| conditions.md:260 | 15 | `spec_setups.md` | spec | yes | no | historical |
| awd_ers.md:18 | 16 | `re/scratch/task16/spec_awd.md`, `spec_kers.md`, `spec_ers.md`, `spec… | spec briefs | yes | no | historical |
| awd_ers.md:193 | 16 | `final_checks.sh` | script (final_compare.sh, 4 game-replay pairs, 8 faults16 pairs, 2 coverage runs) | yes | yes | rerun (replaced) |
| awd_ers.md:20 | 16 | logs "all in re/scratch/task16/ written by final_checks.sh" | build output (final_compare.txt/.sh, final_game_replay.txt, final_faults.txt, coverage_*.txt, rec_*.txt, cmp_*.txt) | yes | no | historical |
| awd_ers.md:283 | 16 | `coverage16.py`, output `coverage_hyt.txt` | script + build output | yes | no | historical |
| awd_ers.md:354 | 16 | `record_awd.sh`, `record_hybrid.sh`, `record_testcars.sh` (+ more in… | scripts (batches) | yes | yes | rerun (replaced) |
| awd_ers.md:390 | 16 | `alpha2026_csp.md` | notes | yes | no | historical |
| awd_ers.md:471 | 16 | `review_task16.md` | notes | yes | no | historical |
| grip_investigation.md:118 | 16 | `csp_survey.md` (full table of every key) | notes | yes | no | historical |
| grip_investigation.md:202 | 16 | `oracle_vs_session.md` | notes | yes | no | historical |
| grip_investigation.md:4 | 16 | `re/scratch/task16/` briefs | notes | yes | no | historical |
| suspensions.md:19 | 17 | `record_sus.sh`, `compare_sus.sh`, `record_col.sh`, `record_k.sh`, `t… | scripts | yes | yes | rerun (replaced) |
| suspensions.md:25 | 17 | `spec_strut.md`, `spec_axle.md`, `spec_ml.md`, `spec_rearsteer.md` | spec | yes | no | historical |
| several_cars.md:14 | 24 | `re/scratch/task24/spec_*.md`, `review_*.md` | notes | no | no | historical |
| several_cars.md:15 | 24 | `more_proofs.sh`, `final_check.sh` | scripts (batches) | no | yes | rerun (replaced) |
| several_cars.md:204 | 24 | `one_car_same.py` (one car vs v0.23.0) | script | no | yes | historical |
| several_cars.md:320 | 24 | `spec_4_race_ini_spawn.md` | spec | no | no | historical |
| several_cars.md:360 | 24 | `spec_5_draw_hear.md` | spec | no | no | historical |
| several_cars.md:400 | 24 | `review_*.md` | notes | no | no | historical |
| several_cars.md:427 | 24 | `mcrec.py <file.mcrec> [every]` | script (speeds, pedals, damage, pit switch dump) | no | no | rerun (replaced) |
| race_rules.md:248 | 26 | `re/scratch/task26/proof.py --again` (20 tapes four ways) | script | no | yes | historical |
| ai.md:19 | 27 | `re/scratch/task27/final.sh` (+ `final/` log dir) | script (batch, about 12 min; labels skipped when finished) | no | yes | rerun (replaced) |
| ai.md:20 | 27 | `re/scratch/task27/final/` | build output (logs per label) | no | no | rerun (replaced) |
| ai.md:21 | 27 | `fill_report.py "<state>"` | script (fills doc from template + logs) | no | no | rerun (replaced) |
| ai.md:211 | 27 | `re/scratch/task27/same.py` | script (39 recorded single-car drives, every 4th of `oracle/game/*.ryin`, replayed by v0.26.1 rustyac.exe and the new one => byte-identical state dum… | no | yes | historical |
| ai.md:22 | 27 | `report_template.md` | notes/template | no | no | rerun (replaced) |
| ai.md:23 | 27 | `re/scratch/task27/t.sh oracle/t27f/<label>/<scenario>.mcrec 0,1,8` | script (port vs recording, no game) | no | yes | rerun (replaced) |
| ai.md:24 | 27 | `events.py <mcrec>` | script (what AI drivers did in a recording) | no | no | rerun (replaced) |
| ai.md:25 | 27 | `spec_spline.md`, `spec_harness.md` | spec briefs | no | no | historical |
| ai.md:26 | 27 | `re/scratch/task27/asm/` | listings | no | no | historical |
| ai.md:380 | 27 | `re/scratch/task27/` subagent briefs | notes | no | no | historical |
| pit_stops.md:307 | 27 | `re/scratch/task27/final.sh` (16 AI scenarios: 15 replayed from T27 g… | script | no | yes | historical |
| race_rules.md:11 | 28 | `sh re/scratch/task28/micro.sh 25` | script (200 made-up drives through game referee and port) | no | yes | rerun (replaced) |
| race_rules.md:13 | 28 | `final_a.sh; final_b.sh` | scripts (whole sessions) | no | yes | rerun (replaced) |
| race_rules.md:15 | 28 | `bin/car_oracle_final.exe` | build output | no | no | rerun (replaced) |
| race_rules.md:172 | 28 | `re/scratch/task28/ini/` and T24b ini: 14 race.ini files IDENTICAL (g… | request list / ini files | no | yes | historical |
| race_rules.md:20 | 28 | `re/scratch/task28/` + 3 briefs | notes | no | no | historical |
| race_rules.md:223 | 28 | `re/scratch/task28/ini/cm_weekend.ini` | ini (practice 1 min, quali 1 min, race 2 laps, 4 cars, PENALTIES=1, JUMP_START_PENALTY=1) | no | yes | historical |
| pit_stops.md:15 | 28b | `sh re/scratch/task28b/final.sh a`, `b`, `c` (3 lanes, ~25 min) | script | no | yes | rerun (replaced) |
| pit_stops.md:17 | 28b | `re/scratch/task28b/bin/car_oracle_final.exe` | build output (copy of CO so a build can proceed in parallel) | no | no | rerun (replaced) |
| pit_stops.md:18 | 28b | `tables.py` | script (prints result table from logs) | no | no | rerun (replaced) |
| pit_stops.md:20 | 28b | `recheck.sh a\|b\|c` | script (port replayed over T27 + T28 recordings, no acs.exe) | no | yes | rerun (replaced) |
| pit_stops.md:24 | 28b | `render_crew.sh` | script (lollipop man + marker, game renderer vs port) | no | yes | rerun (replaced) |
| pit_stops.md:25 | 28b | `bash online.sh <label> <seconds>` | script (live run vs local acServer copy with pit window) | no | yes | rerun (replaced) |
| pit_stops.md:27 | 28b | `re/scratch/task28b/` + 3 briefs (spec_pitstop, spec_window_online, s… | notes | no | no | historical |
| pit_stops.md:297 | 28b | `final.sh` ("two tables are one batch") | script | no | yes | historical |
| pit_stops.md:309 | 28b | `tape_proof.py --again` (20 multiplayer tapes of T25+26, four ways: d… | script | no | yes | historical |
| pit_stops.md:315 | 28b | `sh re/scratch/task28b/exe.sh` | script (builds/prepares rustyac.exe run) | no | no | historical |
| pit_stops.md:324 | 28b | `bash online.sh` (copy of game's acServer.exe) | script | no | yes | historical |
| pit_stops.md:336 | 28b | `sh render_crew.sh` | script (frames of lollipop man/marker, game vs port) | no | yes | historical |
| pit_stops.md:353 | 28b | `python same.py`, `sh nopit.sh` (4 AI recordings of earlier versions… | scripts | no | yes | historical |
| pit_stops.md:440 | 28b | `spec_crew.md` | spec | no | no | historical |

### Contacts (1 references; now `tools/proofs/fresh.sh (collide)`)

| Doc line | Task | Scratch path | What it was | Still there? | A proof needs it? | Mention |
|---|---|---|---|---|---|---|
| contacts.md:21 | 13 | `re/scratch/task13/` | notes + logs (rerec_a/b/c.log, final_collide.log, collision_table.txt, report_howto.txt) | yes | no | historical |

### Tracks and the survey (7 references; now `tools/proofs/fresh.sh (rays)`)

| Doc line | Task | Scratch path | What it was | Still there? | A proof needs it? | Mention |
|---|---|---|---|---|---|---|
| track.md:121 | 12 | `re/scratch/task12/` | notes | yes | no | historical |
| track.md:20 | 12 | `re/scratch/task12/` five briefs | spec | yes | no | historical |
| tracks.md:22 | 18 | `re/scratch/task18/` three briefs | spec | yes | no | historical |
| tracks.md:347 | 18 | `rays_all.sh` (batch of `CO rays --track <t> [--layout <l>] --count 2… | script | yes | yes | rerun (replaced) |
| tracks.md:901 | 18 | `sh rays_all.sh 200000` then `python rays_table.py` | script + table printer | yes | yes | rerun (replaced) |
| tracks.md:910 | 18 | `batch.sh <car> <root name> list_main.txt` ("needs copies of the thre… | script + request list | no | yes | rerun (replaced) |
| tracks.md:911 | 18 | `re/scratch/task18/bin` + `tables.py` | build output (3 exe copies) + table printer | no | yes | rerun (replaced) |

### Sound (12 references; now `tools/proofs/sound.sh`)

| Doc line | Task | Scratch path | What it was | Still there? | A proof needs it? | Mention |
|---|---|---|---|---|---|---|
| audio.md:105 | 19 | `listen/` table: `f2004_onboard_la_source_eau_rouge.wav` (20.0 s) and… | recordings (WAVs) | no | no | historical |
| audio.md:113 | 19 | `spec_*.md` six briefs | spec | yes | no | historical |
| audio.md:14 | 19 | `re/scratch/task19/record_tapes.sh` | script (16 tapes, ~10 min) | yes | yes | rerun (replaced) |
| audio.md:17 | 19 | `re/scratch/task19/listen/` | recordings (2 listening WAVs, 3.3M and 3.7M) | yes | no | historical |
| audio.md:18 | 19 | `re/scratch/task19/` scratch dir, `spec_*.md` | notes (7 spec files exist, doc says six) | yes | no | historical |
| audio.md:222 | 19 | `sh re/scratch/task19/compare_all.sh` | script (18 `audio_oracle compare` rows, ~15 min) | yes | yes | rerun (replaced) |
| audio.md:306 | 19 | `sh re/scratch/task19/e2e.sh <scenario> <camera> [car ...]` | script (game side vs real rustyac.exe replay, logdiff) | yes | yes | rerun (replaced) |
| audio.md:455 | 19 | `review_*.md` | notes (3 files: review_car, review_engine_track, review_game_ffi) | yes | no | historical |
| audio.md:512 | 19 | `record_tapes.sh` (section 10 command list) | script | yes | yes | rerun (replaced) |
| audio.md:513 | 19 | `compare_all.sh` | script | yes | yes | rerun (replaced) |
| audio.md:516 | 19 | `e2e.sh spa_launch cockpit` | script | yes | yes | rerun (replaced) |
| tools/audio_oracle/src/main.rs:103 | 19 | re/scratch/task19/root | AC-root stand-in for audio_oracle (default `--root`) | yes | yes | rerun (replaced) |

### Render oracle (86 references; now `tools/proofs/render.sh`)

| Doc line | Task | Scratch path | What it was | Still there? | A proof needs it? | Mention |
|---|---|---|---|---|---|---|
| damage_sparkles.md:338 | 19 | task19/compare_all.sh task22b/audio_rows.txt | script (EXISTS) with a rows output file in a deleted dir | yes | yes | historical |
| damage_sparkles.md:412 | 19 | task19/compare_all.sh task22b/audio_rows.txt | as :338 | yes | yes | rerun (replaced) |
| damage_sparkles.md:425 | 19 | task19/compare_all.sh | script (EXISTS); "18 of 18 ok" | yes | yes | historical |
| postfx.md:333 | 19 | task19/compare_all.sh | script (EXISTS); 18 drives, 16,704,341 calls | yes | yes | historical |
| postfx.md:559 | 19 | task19/compare_all.sh | EXISTS | yes | yes | rerun (replaced) |
| renderer_leftovers.md:309 | 19 | task19/compare_all.sh | EXISTS; 16,704,486 calls | yes | yes | historical |
| renderer_leftovers.md:483 | 19 | task19/compare_all.sh | EXISTS | yes | yes | rerun (replaced) |
| renderer_scene.md:239 | 19 | task19/compare_all.sh | EXISTS | yes | yes | historical |
| renderer_scene.md:471 | 19 | task19/compare_all.sh | EXISTS | yes | yes | rerun (replaced) |
| damage_sparkles.md:422 | 20 | task20/batch.sh | script: 21 frames; "21 of 21" | no | yes | historical |
| renderer_car.md:12 | 20 | task20/batch.sh | 21 frames | no | yes | rerun (replaced) |
| renderer_car.md:358 | 20 | task20/batch.sh | script | no | yes | rerun (replaced) |
| renderer_core.md:149 | 20 | task20/batch.sh | script: one `render_oracle compare` per line | no | yes | rerun (replaced) |
| renderer_core.md:16 | 20 | task20/ (spec_1_kgl.md .. spec_12_skinned.md, patch_*.py, batch.sh, b… | folder description | no | yes | historical |
| renderer_core.md:19 | 20 | task20/out/, shots/ | oracle logs/pictures (build output), deleted by design | no | no | historical |
| renderer_core.md:314 | 20 | task20/spec_*.md | briefs | no | no | historical |
| renderer_core.md:333 | 20 | task20/out | `--out` of `compare --track spa --view chase` | no | no | rerun (replaced) |
| renderer_core.md:334 | 20 | task20/poses/e30_magione_turn.pose, task20/out | pose input + out | no | yes | rerun (replaced) |
| renderer_core.md:335 | 20 | task20/batch.sh ("monza, 1M at rest" filter) | script, 21 frames ~6 min | no | yes | rerun (replaced) |
| renderer_leftovers.md:482 | 20 | task22c/batch20.sh | Task 20's 21 | no | yes | rerun (replaced) |
| renderer_scene.md:470 | 20 | task20/batch.sh | script | no | yes | rerun (replaced) |
| damage_sparkles.md:423 | 21 | task21/batch.sh | script: 27 seqs; "27 of 27" | no | yes | historical |
| renderer_car.md:11 | 21 | task21/batch.sh | 27 seqs ~25 min | no | yes | rerun (replaced) |
| renderer_car.md:13 | 21 | task21/ (spec_*.md, p_*.py, out/, results) | briefs, patches, output | no | no | historical |
| renderer_car.md:16 | 21 | task21/record_tapes.sh -> oracle/audio + oracle/render_tapes | script that wrote the tapes | no | no | historical |
| renderer_car.md:343 | 21 | task21/ | reader specs (two of 14 cut off) | no | no | historical |
| renderer_car.md:357 | 21 | task21/batch.sh ("launch" filter) | script; `sh batch.sh "launch"` ran only rows matching a word | no | yes | rerun (replaced) |
| renderer_leftovers.md:481 | 21 | task22c/batch21.sh | Task 21's 27 | no | yes | rerun (replaced) |
| renderer_scene.md:11 | 21 | task21/batch.sh | 27 seqs | no | yes | rerun (replaced) |
| renderer_scene.md:469 | 21 | task21/batch.sh | script | no | yes | rerun (replaced) |
| damage_sparkles.md:424 | 22 | task22/batch.sh | script: 23 seqs; "23 of 23" | no | yes | historical |
| renderer_leftovers.md:480 | 22 | task22c/batch22.sh | Task 22's 23 on 22c's oracle | no | yes | rerun (replaced) |
| renderer_scene.md:10 | 22 | task22/batch.sh | 23 seqs ~25 min | no | yes | rerun (replaced) |
| renderer_scene.md:13 | 22 | task22/ (nine spec_*.md, p_*.py, bench.sh, cars/, out/) | briefs, patches, test cars, outputs | no | yes | historical |
| renderer_scene.md:340 | 22 | task22/bench.sh | performance table: `rustyac.exe --car ks_ferrari_f2004 --track <track> --camera <view> --no-race-ini --autodrive --audio-null --headless --duration 2… | no | no | rerun (replaced) |
| renderer_scene.md:401 | 22 | task22/spec_5_track_survey.md 4.3, 4.4 | brief (pit crew mechanics) | no | no | historical |
| renderer_scene.md:450 | 22 | task22/ | nine reader briefs | no | no | historical |
| renderer_scene.md:468 | 22 | task22/batch.sh ("clouds" filter) | script | no | yes | rerun (replaced) |
| renderer_scene.md:472 | 22 | task22/bench.sh | perf script | no | no | rerun (replaced) |
| renderer_scene.md:476 | 22 | `O=<exe copy> ROOT=re/scratch/render_oracle/root2 sh task22/batch.sh` | env contract (see :487 above) | no | yes | rerun (replaced) |
| damage_sparkles.md:105 | 22b | task22b/control/control_result.txt | result: heading +40.5 deg damage on vs +0.7 off | no | no | historical |
| damage_sparkles.md:115 | 22b | task22b/toe.py | analysis script: toe from hub matrices of a .carrec | no | no | historical |
| damage_sparkles.md:185 | 22b | task22b/spkdiff.py, spk/shots.txt | script + results: sparkle pixel count card vs WARP (`rustyac.exe --no-race-ini --autodrive --at 12 --screenshot`, with and without `--warp`, 1280x720… | no | yes | historical |
| damage_sparkles.md:274 | 22b | task22b/fps/rows_pass1.txt (+rows_pass2) | frame-rate table, this PC | no | no | historical |
| damage_sparkles.md:294 | 22b | task22b/fps.sh | script: 20 s `--autodrive` bench per graphics cell | no | no | rerun (replaced) |
| damage_sparkles.md:296 | 22b | task22b/fps_breakdown.sh, fps/breakdown.txt | script + output: cost per setting at Spa | no | no | historical |
| damage_sparkles.md:369 | 22b | task22b/audio_recheck.txt | result of two re-run drives | no | no | historical |
| damage_sparkles.md:389 | 22b | task22b/fps.sh 20 | command line | no | no | rerun (replaced) |
| damage_sparkles.md:396 | 22b | task22b/crash_all.sh | script: five cars compare, faults, replay, toe | no | yes | rerun (replaced) |
| damage_sparkles.md:397 | 22b | task22b/toe.py + oracle/t22b_f2004/spa_crash.carrec | analysis script + recording | no | no | rerun (replaced) |
| damage_sparkles.md:404 | 22b | task22b/pop (out dir) + oracle/t22b_pop/spa_launch.audiotape | render compare of a burst tyre | no | yes | rerun (replaced) |
| damage_sparkles.md:407 | 22b | task22b/spk_shots.sh | script: sparkle screenshots card vs WARP | no | yes | rerun (replaced) |
| damage_sparkles.md:409 | 22b | task22b/hw (out dir); task20/poses/f2004_spa_rest.pose | hardware-device compare | no | yes | rerun (replaced) |
| damage_sparkles.md:5 | 22b | task22b/ (reader_r0..2.md, review_r0..2.md), oracle/t22b_* (~1.3 GB) | notes + recordings; says `sh task22b/crash_all.sh` brings every table back | no | no | historical |
| damage_sparkles.md:57 | 22b | task22b/walls_result.txt | result file: F2004 spa_wall_low/high/slide/gravel zone numbers | no | no | historical |
| damage_sparkles.md:60 | 22b | task22b/bent/result.txt | result: 11 frames of crashed F2004 from the front pixel-identical | no | yes | historical |
| postfx.md:342 | 22b | task22b/spkdiff.py | sparkle diff script | no | yes | historical |
| renderer_leftovers.md:11 | 22c | task22c/batch.sh | script: 18 seqs ~12 min | no | yes | rerun (replaced) |
| renderer_leftovers.md:14 | 22c | task22c/ spec_*.md (seven) | briefs | no | no | historical |
| renderer_leftovers.md:406 | 22c | task22c/spec_7_time_attack_gates.md sec. 10 | brief (ghost car needs) | no | no | historical |
| renderer_leftovers.md:466 | 22c | task22c/ | reader briefs (seven) | no | no | historical |
| renderer_leftovers.md:479 | 22c | task22c/batch.sh ("ideal" filter) | script | no | yes | rerun (replaced) |
| renderer_leftovers.md:487 | 22c | `O=<exe copy> ROOT=re/scratch/render_oracle/root2 sh task22c/batch22.… | env contract of the batch scripts: `O` = a private copy of render_oracle.exe, `ROOT` = `--root` | no | yes | rerun (replaced) |
| docs/map/yebis.md:266 | 23 | `yebis_3_params.md`, `yebis_2_frame.md` s9 | notes | no | no | historical |
| docs/map/yebis.md:6 | 23 | `re/scratch/task23/` (yebis_1_compilands.md ... reader briefs "with t… | notes + extraction scripts | no | no | historical |
| postfx.md:10 | 23 | task23/batch_msaa.sh, batch_post.sh | scripts: 8 + 16 = 24 seqs, ~15 min | no | yes | rerun (replaced) |
| postfx.md:11 | 23 | task23/batch20.sh, 21, 22, 22c | scripts: older batches on the Task 23 oracle | no | yes | rerun (replaced) |
| postfx.md:14 | 23 | task23/ (spec_*.md, yebis_*.md, p_*.py, out*/, spk/, fps/) | briefs, patch scripts, outputs | no | no | historical |
| postfx.md:424 | 23 | task23/fps.sh | frame-rate script | no | no | historical |
| postfx.md:551 | 23 | task23/batch_msaa.sh | 8 MSAA seqs | no | yes | rerun (replaced) |
| postfx.md:552 | 23 | task23/batch_post.sh | 16 post-processing / HUD-pass seqs | no | yes | rerun (replaced) |
| postfx.md:553 | 23 | task23/batch22c.sh | Task 22c's 18 on Task 23's oracle | no | yes | rerun (replaced) |
| postfx.md:554 | 23 | task23/batch22.sh | Task 22's 23 | no | yes | rerun (replaced) |
| postfx.md:555 | 23 | task23/batch21.sh | Task 21's 27 | no | yes | rerun (replaced) |
| postfx.md:556 | 23 | task23/batch20.sh | Task 20's 21 | no | yes | rerun (replaced) |
| postfx.md:557 | 23 | task23/spk_aa.sh | sparkle counts card vs WARP with AA | no | yes | rerun (replaced) |
| postfx.md:558 | 23 | task23/fps.sh | frame rates | no | no | rerun (replaced) |
| draw_hear_cars.md:15 | 24b | `sh re/scratch/task24b/batch.sh` | script (19 picture sequences of several cars, ~25 min; needs `oracle/t24b` tapes) | no | yes | rerun (replaced) |
| draw_hear_cars.md:17 | 24b | `audio_batch.sh` | script (16 drives, ~15 min) | no | yes | rerun (replaced) |
| draw_hear_cars.md:18 | 24b | `car_oracle race-ini re/scratch/task24b/race/race6.ini --root re/scra… | test race.ini + root | no | yes | rerun (replaced) |
| draw_hear_cars.md:20 | 24b | `re/scratch/task24b/` dir (3 spec, 3 review, patch scripts, batches,… | notes + scripts | no | no | historical |
| draw_hear_cars.md:301 | 24b | `python re/scratch/task24b/one_car_same.py` | script (one car + several-car physics vs v0.24.0: 6-car 12 s and 12-car 12 s drives recorded by v0.24.0 replayed with 0 and 8 workers) | no | yes | historical |
| draw_hear_cars.md:386 | 24b | `sh re/scratch/task24b/perf.sh` | script (timing, pool of 8 workers, acs.exe not running) | no | no | historical |
| draw_hear_cars.md:494 | 24b | `review_*.md` | notes | no | no | historical |
| draw_hear_cars.md:551 | 24b | `car_oracle race-ini <file.ini> [--pit-slots n] [--seed n] --root re/… | usage line | no | yes | rerun (replaced) |
| draw_hear_cars.md:554 | 24b | `batch.sh`, `audio_batch.sh`, `old/*.sh` | scripts | no | yes | historical |

### GUI sequences (62 references; now `tools/proofs/gui.sh`)

| Doc line | Task | Scratch path | What it was | Still there? | A proof needs it? | Mention |
|---|---|---|---|---|---|---|
| apps_car_data.md:72 | 22 (cited by 33b) | re/scratch/task22/cars/lithium_panels/ | made-up car folder with a fixed digital_panels.ini (Formula Lithium) | no | yes | rerun (replaced) |
| gui_hud.md:16 | 29 | task29/gui_all.sh | script: 14 GUI seqs, ~3 min | no | yes | rerun (replaced) |
| gui_hud.md:17 | 29 | task29/gui_all.txt | result lines | no | no | rerun (replaced) |
| gui_hud.md:18 | 29 | task29/root, task29/out | `--root` scratch and `--out` | no | no | rerun (replaced) |
| gui_hud.md:22 | 29 | task29/end.sh, task29/end/ | script (all end-of-task checks) + results | no | yes | rerun (replaced) |
| gui_hud.md:222 | 29 | task29/root/documents/... | Documents stand-in inside the root; forms_layout writes its layout there | no | yes | historical |
| gui_hud.md:266 | 29 | task29/end.sh, end/ | results table source | no | no | historical |
| gui_hud.md:28 | 29 | task29/spec_*.md | reader briefs | no | no | historical |
| gui_hud.md:29 | 29 | task29/p*.py | patch scripts used while developing | no | no | historical |
| menus.md:31 | 29 | task29/gui_all.sh; "never two at once" | script + rule | no | yes | rerun (replaced) |
| setup.md:268 | 29 | task29/gui_all.sh (table row: 14 seqs, 1,262 frames) | script | no | yes | historical |
| menus.md:142 | 30 | task30/root/documents | the oracle's Documents stand-in | no | no | historical |
| menus.md:28 | 30 | task30/gui_all.sh | script: all 38 menu seqs, ~1 h | no | yes | rerun (replaced) |
| menus.md:29 | 30 | task30/gui_all.txt | result lines | no | no | rerun (replaced) |
| menus.md:30 | 30 | task30/root, task30/out | root + out | no | no | rerun (replaced) |
| menus.md:32 | 30 | task30/end.sh | script (end checks) | no | yes | rerun (replaced) |
| menus.md:33 | 30 | task30/tgt_v029 (v0.29.0 build, deleted: disk full) | build output for physics dump comparison | no | no | historical |
| menus.md:41 | 30 | task30/spec_*.md (spec_timetable.md unused) | reader briefs | no | no | historical |
| setup.md:267 | 30 | task30/gui_all.sh (table row: 38 seqs, 2,316 frames) | script | no | yes | historical |
| setup.md:137 | 31 | task31/root/documents | the oracle's Documents stand-in | no | no | historical |
| setup.md:149 | 31 | task31/review_a.md, review_b.md | reviewer notes | no | no | historical |
| setup.md:28 | 31 | task31/final_b.sh | script: Task 30's 38 + Task 29's 14 GUI seqs + every setup seq on every car (>1 h) | no | yes | rerun (replaced) |
| setup.md:30 | 31 | task31/gui_final_30.txt, gui_final_29.txt, gui_final_31.txt, cars2.txt | result files | no | no | rerun (replaced) |
| setup.md:32 | 31 | task31/cmp.sh (`cmp.sh setup_tabs@bmw_z4_gt3`) | script: one GUI seq | no | yes | rerun (replaced) |
| setup.md:33 | 31 | task31/final_a.sh, tape_proof.py | scripts: clippy 4 ways, tests, tools, dumps; tape proof | no | no | rerun (replaced) |
| setup.md:336 | 31 | task31/tgt_old (0.24 GB), oracle recordings (6.4 GB) | disk-space note | no | no | historical |
| setup.md:34 | 31 | task31/tgt_old (v0.30.0 build, deleted) | build output for dump comparison | no | no | historical |
| setup.md:341 | 31 | task31/spec_presets.md | brief | no | no | historical |
| setup.md:35 | 31 | task31/tape_proof.py | script | no | no | rerun (replaced) |
| setup.md:37 | 31 | `CARGO_TARGET_DIR=re/scratch/task31/tgt` | build dir when rustyac.exe locks target/release | no | no | rerun (replaced) |
| setup.md:40 | 31 | task31/root/seed_setups/<car>, task31/seed.sh | input: user's own setups of eight cars copied into the root; seed.sh = the copier | no | yes | rerun (replaced) |
| setup.md:44 | 31 | task31/spec_*.md | briefs (spec_presets.md unused, spec_escstart.md B.2, spec_setupmanager.md) | no | no | historical |
| apps_user.md:17 | 32 | task32/cmp.sh | script (one GUI seq: `cmp.sh app_volumes`, `@short` = shorter) | no | yes | rerun (replaced) |
| apps_user.md:18 | 32 | task32/final_b.sh, task32/end/ | script (all app_* + earlier GUI seqs, ~1 h) + results dir | no | yes | rerun (replaced) |
| apps_user.md:188 | 32 | task32/end/apps.txt | result log of final run | no | no | historical |
| apps_user.md:21 | 32 | task32/croot (a `--root` for car_oracle) + oracle/t32/race | recording command | no | yes | rerun (replaced) |
| apps_user.md:215 | 32 | task32/older.sh | script: re-run Task 29-31 GUI seqs on final oracle | no | yes | historical |
| apps_user.md:223 | 32 | task32/final_a.sh, tape_proof.py | scripts: clippy/tests/tools/dumps; tape proof | no | no | historical |
| apps_user.md:25 | 32 | task32/*.md | reader briefs | no | no | historical |
| apps_car_data.md:247 | 33 | re/scratch/task33/tyre_apps.md | reader brief (TyreTesterForm, sections 2-3) | no | no | historical |
| apps_telemetry.md:14 | 33 | re/scratch/task33/*.md | reader briefs (git-ignored) | no | no | historical |
| apps_telemetry.md:17 | 33 | re/scratch/task33/end_b.sh, results re/scratch/task33/end/ | script: every oracle GUI sequence of Tasks 29-33 | no | yes | rerun (replaced) |
| apps_telemetry.md:18 | 33 | re/scratch/task33/end_a.sh | script: clippy, --locked builds/tests, tools, 4 physics dumps vs v0.32.0 | no | yes | rerun (replaced) |
| apps_telemetry.md:19 | 33 | re/scratch/task33/tgt_old | v0.32.0 build for comparison | no | yes | historical |
| apps_telemetry.md:201 | 33 | re/scratch/task33/end/task33.txt | result text of the 18 Task-33 sequences | no | no | historical |
| apps_telemetry.md:229 | 33 | re/scratch/task33/end_b.sh, end_b2.sh, end_b3.sh | scripts: earlier GUI sequences on the final oracle (Task 32 22, Task 30 38, Task 29 14, Task 31 5) | no | yes | historical |
| apps_telemetry.md:238 | 33 | re/scratch/task33/end_a.sh | script | no | yes | historical |
| apps_telemetry.md:295 | 33 | re/scratch/task33/console.md | reader brief (console variables table cell) | no | no | historical |
| apps_telemetry.md:328 | 33 | re/scratch/task33/telemetry_apps.md | reader brief | no | no | historical |
| apps_car_data.md:14 | 33b | re/scratch/task33b/end_all.sh | script (builds v0.33.0 from a worktree into tgt_old, then end_a.sh + end_b.sh) | no | yes | rerun (replaced) |
| apps_car_data.md:17 | 33b | re/scratch/task33b/end/ (+tgt_old, tgt, state dumps) | build output / result texts, deleted by the task itself | no | no | historical |
| apps_car_data.md:18 | 33b | re/scratch/task33b/cmp.sh app_telemetry, root re/scratch/task33b/root | script + root folder (one GUI compare) | no | yes | rerun (replaced) |
| apps_car_data.md:19 | 33b | re/scratch/task33b/ (git-ignored) | scratch folder | no | no | historical |
| apps_car_data.md:20 | 33b | re/scratch/task33b/scene.sh [filter], scene_result.txt | script (4 scene sequences, Table 1) + result text | no | yes | rerun (replaced) |
| apps_car_data.md:59 | 33b | re/scratch/task33b/cardata/ | extracted data.acd of cars with digital panels (survey input) | no | no | historical |
| apps_car_data.md:73 | 33b | re/scratch/task33b/cars/488_delta/ | made-up car: ks_ferrari_488_gt3 with digital_instruments.ini turned to DELTA_GRAPH (DIRECTION=0, DELTA_MAX=1.5) plus one more (DIRECTION=1, DELTA_MAX… | no | yes | rerun (replaced) |
| online_menus.md:105 | 34 | re/scratch/task34/mousepit.sh (`car_oracle multi --scenario pit_mouse… | script, 5 runs + 1 fault | no | yes | rerun (replaced) |
| online_menus.md:120 | 34 | re/scratch/task34/live.sh; acServer copy re/scratch/task34/server | script + server (Laguna Seca, votes 60 %, kicks 70 %, 12 s votes, Z4 slots with FIXED_SETUP=t34_z4.ini) | no | yes | rerun (replaced) |
| online_menus.md:136 | 34 | re/scratch/task34/end_a.sh (end_a2.sh) | script; machine MCE | no | yes | historical |
| online_menus.md:22 | 34 | re/scratch/task34/cmp.sh <sequence> (ROOT= re/scratch/task31/root for… | script (one GUI compare) | no | yes | rerun (replaced) |
| online_menus.md:23 | 34 | mousepit.sh, live.sh, end_a.sh, end_b.sh | scripts | no | yes | rerun (replaced) |
| online_menus.md:75 | 34 | re/scratch/task34/end_b.sh, end/ | script + results (WARP) | no | yes | rerun (replaced) |

### Replays (17 references; now `tools/proofs/replays.sh`)

| Doc line | Task | Scratch path | What it was | Still there? | A proof needs it? | Mention |
|---|---|---|---|---|---|---|
| replays.md:25 | 28c | `sh re/scratch/task28c/render_crew.sh` (crew, 5 sequences, ~6 min) | script | no | yes | rerun (replaced) |
| replays.md:27 | 28c | `bin/render_oracle_crew.exe` | build output | no | no | rerun (replaced) |
| replays.md:28 | 28c | AO compare line with `--crew 20:10 --root re/scratch/task28c/audio_ro… | request line (command given in full) | no | yes | rerun (replaced) |
| replays.md:30 | 28c | `replay.sh a\|b\|c` (14 whole sessions, ~25 min) | script | no | yes | rerun (replaced) |
| replays.md:32 | 28c | `bin/car_oracle_replay.exe` | build output | no | no | rerun (replaced) |
| replays.md:33 | 28c | `files.sh` (every file of Documents\Assetto Corsa\replay loaded, `bin… | script | no | yes | rerun (replaced) |
| replays.md:35 | 28c | `render_replay.sh` | script (frames + sound of playback, game vs port) | no | yes | rerun (replaced) |
| replays.md:37 | 28c | `determinism.sh` | script (rustyac.exe 0/1/8 workers; and vs v0.28.1 exe) | no | yes | rerun (replaced) |
| replays.md:38 | 28c | `re/scratch/task28c/tgt_v0281/` | build output (exe from tag v0.28.1) | no | yes | rerun (replaced) |
| replays.md:39 | 28c | `ghost.sh` (6 sessions) + `render_ghost.sh`, `bin/car_oracle_ghost.ex… | scripts | no | yes | rerun (replaced) |
| replays.md:40 | 28c | `render_ghost.sh` | script | no | yes | rerun (replaced) |
| replays.md:42 | 28c | `re/scratch/task28c/root_g/` (holds the `.ghost` file ghost.sh leaves) | recording | no | yes | rerun (replaced) |
| replays.md:43 | 28c | `telemetry.sh` (6 sessions), `bin/car_oracle_tel.exe` | script | no | yes | rerun (replaced) |
| replays.md:45 | 28c | `tables.py` | script | no | no | rerun (replaced) |
| replays.md:48 | 28c | `re/scratch/task28c/` + briefs rec.md, play.md, mgr.md, ... | notes | no | no | historical |
| replays.md:570 | 28c | `re/scratch/task28c/exe/real_spa.png` ("not in the repository") | picture | no | no | historical |
| replays.md:578 | 28c | `ghost.md` B7 | notes | no | no | historical |

### Multiplayer (26 references; now `tools/proofs/net.sh`)

| Doc line | Task | Scratch path | What it was | Still there? | A proof needs it? | Mention |
|---|---|---|---|---|---|---|
| multiplayer_web.md:25 | 20w/26 | re/scratch/task20w/tools (wasmtime 25) | tool | no | no | rerun (replaced) |
| docs/csp/online.md:515 | 25 | `re/scratch/task25` (kit that runs local acServer copies) | scripts + server copy kit (notes only; the committed `server/` folder holds the acServer files) | no | yes | historical |
| multiplayer.md:12 | 25 | re/scratch/task25/batch.sh | script (20 runs + 3 faults, 15 min) | no | yes | rerun (replaced) |
| multiplayer.md:13 | 25 | re/scratch/task25/interop and rich (tapes) | recordings (.nettape) | no | yes | rerun (replaced) |
| multiplayer.md:14 | 25 | re/scratch/task25/run_rich.sh | script (live 4-client session that records tapes) | no | no | rerun (replaced) |
| multiplayer.md:19 | 25 | re/scratch/task25/spec_1_transport.md ... spec_9_netcar_step.md | reader briefs | no | no | historical |
| multiplayer.md:20 | 25 | re/scratch/task25/server (Spa), server_laguna, server_rich | acServer copies with configs | no | yes | rerun (replaced) |
| multiplayer.md:265 | 25 | re/scratch/task25/interop/ | tapes + logs of 2 clients Laguna 200 s | no | no | historical |
| multiplayer.md:266 | 25 | re/scratch/task25/rich/ | 4 clients 150 s | no | no | historical |
| multiplayer.md:267 | 25 | re/scratch/task25/hud/hud.png | picture (docs/port/multiplayer_hud.png is the committed copy) | no | no | historical |
| multiplayer.md:272 | 25 | re/scratch/task25/final/ | v0.25.0 release live-run logs | no | no | historical |
| multiplayer.md:498 | 25 | re/scratch/task25/server_laguna | acServer copy "if still there" | no | no | rerun (replaced) |
| multiplayer_web.md:21 | 25 | re/scratch/task25/batch.sh | script | no | yes | rerun (replaced) |
| multiplayer_web.md:213 | 25 | re/scratch/task25/batch.sh re-run | script | no | yes | historical |
| multiplayer_web.md:22 | 26 | re/scratch/task26/gold.sh | script that produced goldens | no | no | rerun (replaced) |
| multiplayer_web.md:24 | 26 | re/scratch/task26/bin/net_golden.exe and .wasm | built from crates/rustyac-net/examples/net_golden.rs | no | yes | rerun (replaced) |
| multiplayer_web.md:246 | 26 | re/scratch/task26/live.sh; grid.sh | scripts | no | yes | historical |
| multiplayer_web.md:250 | 26 | re/scratch/task26/wss.sh | script (TLS front with throw-away cert, `examples/tls_front.rs`, `--join wss://localhost:9643/net/laguna --tls-root ca.pem`) | no | yes | historical |
| multiplayer_web.md:26 | 26 | re/scratch/task26/live.sh, latency.py, live/ | script + analysis + captured csv | no | yes | rerun (replaced) |
| multiplayer_web.md:261 | 26 | re/scratch/task26/latency.py | script (arrival-time statistics from `--net-arrivals` csv) | no | yes | historical |
| multiplayer.md:304 | 37 | re/scratch/task37/net_work/ (or $NET_GOLD_WORK) | work dir of record.sh (server copies, logs) | no | no | rerun (replaced) |
| multiplayer.md:311 | 37 | re/scratch/task37/net_check/check.log | output of check.sh | no | no | rerun (replaced) |
| multiplayer.md:317 | 37 | re/scratch/task37/wasmtime_build | CARGO_TARGET_DIR for `cargo install wasmtime-cli --locked --root target/web-tools/wasmtime` | no | yes | rerun (replaced) |
| tools/net_gold/check.sh:10 | 37 | re/scratch/task37/net_check | usage comment (output default) | no | no | rerun (replaced) |
| tools/net_gold/check.sh:15 | 37 | `out=${NET_GOLD_OUT:-$repo/re/scratch/task37/net_check}` | default output dir | no | no | rerun (replaced) |
| tools/net_gold/record.sh:16 | 37 | `work=${NET_GOLD_WORK:-$repo/re/scratch/task37/net_work}` | default work dir | no | no | rerun (replaced) |

### Launcher (32 references; now `tools/proofs/launcher.sh`)

| Doc line | Task | Scratch path | What it was | Still there? | A proof needs it? | Mention |
|---|---|---|---|---|---|---|
| launcher.md:10 | 35 | re/scratch/task35/sessions.sh | script (drives the 5 sessions + options with scripts/*.txt) | no | yes | rerun (replaced) |
| launcher.md:114 | 35 | re/scratch/task35/ (fake folders, Task 35) | fake Documents folder root for the oracle | no | yes | rerun (replaced) |
| launcher.md:116 | 35 | re/scratch/task35 (oracle refuses outside it) | rule text | no | no | historical |
| launcher.md:122 | 35 | re/scratch/task35/cmp.sh | script | no | yes | rerun (replaced) |
| launcher.md:149 | 35 | re/scratch/task35/scripts/*.txt | click scripts (practice, quickrace, raceweekend, hotlap, options, results; verbs until/click/js/shot/game-exit) | no | yes | rerun (replaced) |
| launcher.md:186 | 35 | re/scratch/task35/end_a.sh, end_b.sh | scripts: end checks (63 suites 306 passed; 41 GUI sequences identical) | no | yes | historical |
| launcher.md:220 | 35 | `cd re/scratch/task35 ... --script scripts/practice.txt --log out/pra… | command example | no | yes | rerun (replaced) |
| launcher.md:6 | 35 | re/scratch/task35/cmp.sh | script (oracle + port on the same fake folder, diff -r) | no | yes | rerun (replaced) |
| launcher.md:8 | 35 | re/scratch/task35/cmp.sh req/<name>.txt <name> | script + request list | no | yes | rerun (replaced) |
| launcher.md:9 | 35 | re/scratch/task35/tdiff.py out/<name>_o.txt out/<name>_r.txt | script (transcript compare ignoring getresults date) | no | yes | rerun (replaced) |
| launcher_online.md:214 | 36 | re/scratch/task36/tgt_old | v0.35.0 build for dump compare | no | yes | historical |
| launcher_online.md:217 | 36 | re/scratch/task35/req (8 request lists) | request lists: probe 59, practice 1956, quickrace 1974, raceweekend 2007, hotlap 1974, options 1843, results 6, smoke 10 = 9,829 | no | yes | rerun (replaced) |
| launcher_online.md:222 | 36 | re/scratch/task26/gold | the 20 net goldens' old home | no | yes | historical |
| launcher_online.md:244 | 36 | re/scratch/task36/scripts/block.txt | strict filter of non-local addresses for the online page run | no | yes | historical |
| launcher_online.md:263 | 36 | re/scratch/task36 (480 MB of fake folders, recordings, scripts) | disk-space note | no | no | historical |
| web_launcher.md:467 | 37 | re/scratch/task37 | heading "Notes ... moved here from re/scratch/task37" | no | no | historical |
| web_launcher.md:699 | 37 | re/scratch/task37/seed.sh | script (fake "real" cfg folder; first start copies controls.ini/video.ini, not .txt; second start keeps launcher's own controls.ini; real Documents h… | no | yes | historical |
| web_launcher.md:713 | 37 | re/scratch/task37/ini_probe*.ps1 | PowerShell probes of GetPrivateProfileString/WritePrivateProfileString rules | no | no | historical |
| web_launcher.md:732 | 37 | re/scratch/task37/pure_cmp.sh | script (12 lists desktop twice: Win32 ini and --pure-ini; compare answers + player's folders) | no | yes | historical |
| web_launcher.md:752 | 37 | re/scratch/task37/wasm_cmp.sh | script (transcript.exe vs transcript.wasm under wasmtime, /acroot and /player) | no | yes | historical |
| web_launcher.md:770 | 37 | re/scratch/task37/wasi_node.mjs | node 26.4.0 WASI runner (fails: fd_readdir ENOSYS on Windows) | no | no | historical |
| web_launcher.md:776 | 37 | re/scratch/task37/end_a.sh, end_b.sh, drives.sh | end-check scripts | no | yes | historical |
| web_launcher.md:777 | 37 | re/scratch/task37/tgt_old | v0.36.0 build | no | yes | historical |
| web_launcher.md:792 | 37/15 | re/scratch/task15/god_tc4.ini (+ copy of player's god.ini) | setup files mounted for two drives | yes | yes | rerun (replaced) |
| web_launcher.md:31 | 37b | re/scratch/task37b/ (theme_internals.md, web_game_raceini.md, pad_ora… | scratch folder list | no | yes | rerun (replaced) |
| web_launcher.md:358 | 37b | re/scratch/task37b/bridge/bridge.sh | script (12 lists through the bridge in headless Chrome vs desktop transcript.exe) | no | yes | rerun (replaced) |
| web_launcher.md:36 | 37b | `--out re/scratch/task37b/pack` | pack dir | no | yes | rerun (replaced) |
| web_launcher.md:38 | 37b | `python web/serve.py --pack re/scratch/task37b/pack --port 8137` | command | no | yes | rerun (replaced) |
| web_launcher.md:415 | 37b | re/scratch/task37b/end_a.sh, end_b.sh, drives.sh | end-check scripts | no | yes | historical |
| web_launcher.md:416 | 37b | re/scratch/task37b/tgt_old | v0.37.0 build | no | yes | historical |
| web_launcher.md:16 | 37c | re/scratch/task37c/ (rec.sh, proof.sh, chrome_proof.sh, flows.py + fl… | scratch folder | no | yes | rerun (replaced) |
| web_launcher.md:19 | 37c | `--out re/scratch/task37c/pack` | pack output dir | no | yes | rerun (replaced) |

### Browser drives (21 references; now `tools/proofs/browser.sh, ai_page.sh`)

| Doc line | Task | Scratch path | What it was | Still there? | A proof needs it? | Mention |
|---|---|---|---|---|---|---|
| web_controls_pack.md:58 | 11 | re/scratch/task11/spec_pad_keyboard.md | disassembly notes (59 KB) | yes | no | historical |
| web.md:184 | 20w | re/scratch/task20w/digest_shards.sh | script (16 shards `math_proof digest --stride 16 --offset 0..15` desktop vs wasm under wasmtime, 2^32 inputs each) | no | yes | historical |
| web.md:236 | 20w | `python re/scratch/task20w/replay_proof.py` | script (every oracle/game drive through replay_hash.exe vs .wasm) | no | yes | rerun (replaced) |
| web.md:281 | 20w | re/scratch/task20w/proof/results_final.md | result table (160 drives, 1,787,777 steps) | no | no | historical |
| web.md:32 | 20w | re/scratch/task20w/ (dlldis.py, asm/, rcp/, replay_proof.py, proof/re… | scratch folder list | no | yes | historical |
| web.md:620 | 20w | wasmtime in re/scratch/task20w/tools | tool | no | no | rerun (replaced) |
| web.md:622 | 20w | copies of the two binaries in re/scratch/task20w/bin | build copies | no | no | rerun (replaced) |
| web.md:624 | 20w | `cp ... re/scratch/task20w/bin/` | command | no | no | rerun (replaced) |
| web.md:625 | 20w | `python re/scratch/task20w/replay_proof.py` | command | no | yes | rerun (replaced) |
| web.md:639 | 20w | `python re/scratch/task20w/desktop_same.py` | script (v0.20.0 vs new desktop, 14 replays) | no | yes | rerun (replaced) |
| web_controls_pack.md:357 | 20w | re/scratch/task20w/replay_proof.py | "not rerun" mention | no | no | historical |
| web.md:767 | 20w2 | re/scratch/task20w2/desktop_same.py | script (14 drives, v0.20.1 vs new, --dump-states, 53 MB to 1.66 GB each; plus 12 s line-follower screenshots, 3 cameras; --help / --list-tracks text) | no | yes | historical |
| web.md:8 | 20w2 | re/scratch/task20w2/ (bin/ with v0.20.1 exe) | scratch folder, old exe | no | yes | historical |
| web.md:844 | 20w2 | re/scratch/task20w2/review_*.md | review notes | no | no | historical |
| web.md:860 | 20w2 | `python re/scratch/task20w2/desktop_same.py` needs bin/rustyac_v0201.… | command | no | yes | rerun (replaced) |
| web_controls_pack.md:359 | 20w2 | re/scratch/task20w2/desktop_same.py | "not rerun" mention | no | no | historical |
| web_controls_pack.md:358 | 26 | re/scratch/task26/proof.py | "not rerun" mention (80 network replays) | no | no | historical |
| web_controls_pack.md:13 | 26b | re/scratch/task26b/ (controls_web.sh, panel_test.js, panel_test2.js,… | scratch folder and file list | no | yes | rerun (replaced) |
| web_controls_pack.md:157 | 26b | re/scratch/task26b/controls_web.sh | script (browser vs desktop, held keys, 2 cases) | no | yes | rerun (replaced) |
| web_controls_pack.md:300 | 26b | re/scratch/task26b/all_dry.txt | output of the whole-install dry run (`web_pack --config all.toml --dry-run`) | no | no | historical |
| web_controls_pack.md:335 | 26b | re/scratch/task26b/small.toml | test pack config (18 cars, 7 tracks) | no | yes | historical |

### Shaders and the page's frame loop (7 references; now `tools/proofs/shaders.sh, ai_page.sh`)

| Doc line | Task | Scratch path | What it was | Still there? | A proof needs it? | Mention |
|---|---|---|---|---|---|---|
| web_renderer.md:481 | 19 | re/scratch/task19/compare_all.sh | script: 18 drives through `audio_oracle compare` | yes | yes | rerun (replaced) |
| web_renderer.md:57 | 37c | re/scratch/task37c/qr/chrome.json | result json (chain 7aea29db0a574b43, 93,774 steps) | no | no | historical |
| web_renderer.md:50 | 38 | re/scratch/task38/frame_check.sh | script (headless Chrome frameProof) | yes | yes | rerun (replaced) |
| web_renderer.md:91 | 38 | re/scratch/task38/wgsl | shader_table --dump output (144 files) | yes | no | rerun (replaced) |
| web_renderer.md:286 | 38b | re/scratch/task38b/swiftshader/ | vk_swiftshader.dll + ICD json copied from Chrome | yes | yes | rerun (replaced) |
| web_renderer.md:341 | 38b | VK_DRIVER_FILES=.../task38b/swiftshader/vk_swiftshader_icd.json | command | yes | yes | rerun (replaced) |
| web_renderer.md:495 | 38b | re/scratch/task38b/tgt_old and its worktree | old build | no | yes | historical |
