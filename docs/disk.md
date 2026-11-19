# Disk space: what lives where, and how to free it again

Written in Task S1 (2026-11-18/19). C: had 16,275 MB free at the start (proof runs want 18 GB,
`tools/proofs/lib.sh`'s keep floor); 66,174 MB at the end. Sizes below are `du -sm` at the end of
S1 (`re/scratch/taskS1/measure.sh` prints the same list, first line the free MB).

## What lives where (MB, end of S1)

| Path | MB | What it is | Can it go? |
|---|---:|---|---|
| `oracle/` | 14,441 | game recordings the proofs and docs read (`t28` 6,265, `t27f` 1,280, `t27` 1,218, `car` 929, ...) | no: `tools/proofs/fresh.sh` / `physics.sh` read `oracle/t28`; server copy only by hand |
| `dist-web/` | 6,968 | `delta/` 6,960 = web_pack's `--delta` output (made 2026-11-18), `site/` 9 | yes once the pack it feeds is uploaded; web_pack rebuilds it |
| `target/debug` | 6,655 | cargo build output | yes: `cargo sweep` (below) |
| `target/release` | 2,137 | cargo build output (the quick lane's exes) | yes: `cargo sweep`; the next proof run rebuilds |
| `tools/*/target` | 5,342 | each tool's own workspace build (web_pack 2,544, render_oracle 1,000, car_oracle 550, audio_oracle 512, chassis_compare 340, ...) | yes: `cargo sweep` |
| `target/wasm32-unknown-unknown`, `target/wasm32-wasip1` | 1,053 + 740 | wasm builds | yes: `cargo sweep` |
| `ignored/` | 2,145 | CSP 1,294, FMOD SDK 649, ODE 45, test cars | no: build inputs |
| `re/gold/web` | 1,975 | browser gold files | no (never delete under `re/gold/`) |
| `target/full_wgpu`, `target/ro_wgpu` | 1,637 + 550 | Full-mode / render_oracle wgpu builds | not by `cargo sweep` (S1 keeps them); a manual rebuild costs one wgpu build each |
| `IDA/` | 1,167 | `csp` 599, `acs` 550 databases | `IDA/acs` can be rebuilt from acs.exe (hours of auto-analysis plus lost names) |
| `re/scratch/*` | 8,482 | per-task scratch | see "re/scratch" below |
| `target/proofs` | 668 | proof run output, `archive/` lists | no: proofs read it |
| `target/web-tools`, `target/wasm-sysroot`, `target/nuget`, `target/notices.json`, `target/tmp` | 606 + 352 + 147 + 2 + 100 | tools, the wasm sysroot, NuGet, licence notices, runtime files | no: not cargo output |
| `re/gold/fresh/rec` | 1 | kept game recordings of the fresh proofs (re-recorded on the next `--full`) | `archive_kept.sh --move` |
| `re/csp` | 299 | CSP work incl. `fakeac` junctions into AC | no (junctions: never a plain recursive delete) |

## How to free space again (in this order)

1. **Build caches** (`cargo sweep` 0.8.0 is installed). There is no rustup here, only rustc 1.97.0,
   so `--installed` cannot work and is not needed. `cargo sweep --time N` goes by the files'
   *access* times, and on this PC every file read during a session gets the same access time (in
   S1 every file on C: showed the session's start time), so first set access := write time on the
   fingerprints, which turns "unused N days" into "not rebuilt in N days":

   ```powershell
   Get-ChildItem target\debug\.fingerprint, target\release\.fingerprint, target\wasm32-*\*\.fingerprint, tools\*\target\*\.fingerprint -Recurse -File -Force | ForEach-Object { $_.LastAccessTime = $_.LastWriteTime }
   ```

   Then sweep each profile folder on its own: on the repo root `cargo sweep` also walks
   `target/full_wgpu` and `target/ro_wgpu`, and it does not follow junctions.

   ```sh
   for d in debug release wasm32-unknown-unknown wasm32-wasip1; do
       CARGO_TARGET_DIR="$PWD/target/$d" cargo sweep --time 3 .; done
   for t in tools/*/; do [ -d "$t/target" ] && (cd "$t" && cargo sweep --time 3 .); done
   ```

   Never touch `target/proofs`, `target/prebuilt`, `target/web-tools`, `target/wasm-sysroot`,
   `target/nuget`, `target/farm_sync`, `target/notices.json`, `target/full_wgpu`, `target/ro_wgpu`.
   S1: 16,275 → 46,525 MB free; the quick lane's build afterwards (11.5 min, CARGO_BUILD_JOBS=4) took
   about 1.5 GB back.
2. **Old kept recordings**: `sh tools/proofs/archive_kept.sh` lists the `re/gold/fresh/rec` folders
   no current key names; `--move` sends them to `/storage/rustyac-archive/kept/<date>/` on the server
   (tar over ssh, every file's SHA-256 checked there) and only then deletes them here. Every change
   to `tools/car_oracle` (or to AC's install) changes every key (`salt_of`, `tools/proofs/lib.sh`), so
   after such a change *all* kept folders are old: that is right, not a bug. The next
   `run_all.sh --full` (or `--rerecord`) re-records the 217 current keys (several hours of game runs).
3. **re/scratch folders only docs point into**: `bash re/scratch/taskS1/archive_scratch.sh <folders>`
   makes one tar each, checks its SHA-256 on both ends, appends `<name>.tar size sha256 date` to the
   server's `/storage/rustyac-archive/scratch/MANIFEST.txt`, and leaves `ARCHIVED.txt` (where, hash) in
   the emptied folder. Check first that no tracked script or crate names the folder
   (`git grep "re/scratch/<name>" -- . ':!docs'`) and that it holds no junction. Never `task46c`,
   `re/csp`, oracle folders.
   To get one back: `ssh thesupersupersigma@10.0.0.140 "cat /storage/rustyac-archive/scratch/<name>.tar" | tar -xf - -C re/scratch`
   (remove its `ARCHIVED.txt` folder first).

## re/scratch

Archived in S1 (on the server in `/storage/rustyac-archive/scratch/`, SHA-256 in MANIFEST.txt):
taskL1, taskC4a, taskF1, taskW1, taskY5b, taskC2, taskY5c, taskY2a, taskY7, taskY3c, taskY7f,
taskY7b, taskY3b, taskT1 (1,849 MB). Earlier archives there: 2026-11-15/ (Task L1's 16 folders),
taskR1, task45, taskS2, taskY7e.

Still here and why (MB): taskY5d 1,608 (`powf4_table.rs` names it), render_oracle 1,455 (render_oracle
writes there), task47c 813 (junctions; task47d scripts), task46b 759 / task47 692 / task46 668 /
task46c 330 (web_checks scripts read them), taskY2c 624 / taskY5 284 / taskS2b 373 (crates name
them), taskC5b 586 and the small taskC0/C1/C3/C5a/C5c/D1/F2/W2 folders (nothing points into them:
no doc, no script; not archived because S1 archives only folders docs point into).

## If it gets tight again: next candidates (not done in S1)

| Candidate | MB | Cost |
|---|---:|---|
| `dist-web/delta` | 6,960 | none once its pack is on the server; `web_pack --delta` remakes it |
| `oracle/` to server only | 14,441 | proofs and docs read it: every fresh/physics proof would need it copied back (~50 min at 5 MB/s) |
| `IDA/acs` | 550 | rebuildable from acs.exe, but names and types made by hand are lost unless the .i64 goes to the server |
| `re/scratch/taskC5b` + the other folders nothing points into | ~600 | none, archive the same way (decision for Super) |
| `target/full_wgpu` + `target/ro_wgpu` | 2,187 | one wgpu rebuild each |
| `re/gold/fresh/rec` after the next `--full` | ~19,000 | re-recording takes hours; archive only when car_oracle changed again |
