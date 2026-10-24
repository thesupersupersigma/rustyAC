# Task 40: HUD, menus and apps in the browser

## Resume here

- Done: step 0 (lib.sh skips the recording cache for `--ai-line` cases; the
  graphics page's High preset is AC's shipped `cfg/video.ini`).
- Next: step 1, FW1 text on wgpu.
- Scratch: `re/scratch/task40/`. Disk free at start: 36 GB.

## Step 0

- `tools/proofs/lib.sh`: multi cases with `--ai-line` no longer use the
  recording cache, like `--replay` / `--ghost` / `--telemetry`.
- `web/app.js`: High = AC's `cfg/video.ini` as shipped (AC keeps it at
  `cfg/video.ini`, not `system/cfg/`; it has no size, so 1920 x 1080 stays).
  A fresh browser has no video.ini, so it reads as High. Medium and Low keep
  their 0.39 values (the game's defaults with no video.ini, changed as before).
