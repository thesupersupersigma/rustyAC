# AC's apps, part 2: telemetry, car data, stats, the console, translations (Task 33)

## Resume here

State after the last commit (update after every commit):

- **Done and proven:** translations: `acTranslate` / `loadMainINI` / `acLocaleSetLocale`
  (`crates/rustyac-gui/src/locale.rs`), the language from Documents
  `cfg/launcher.ini [WINDOW] LANGUAGE` or `--language`; any GUI sequence of the oracle runs in a
  language with `<sequence>#<lang>` (`sh re/scratch/task33/cmp.sh "esc_menu#it"`, batch
  `re/scratch/task33/lang.sh`: 12 sequences in German and Italian, all identical).
- **Done:** `--dev-apps` and the game's `ENABLE_DEV_APPS` (`render/gui.rs dev_apps_enabled`);
  Task 32's answers: the pitch of car 0's `view.ini` for every car, the apps' saved `car.ini` /
  `dash_cam.ini` / `controls.ini [FF_ENHANCEMENT]` read back at the next start (FFMULT and
  STEER_ASSIST as physics-thread jobs, new job `SetSteerAssist`).
- **In progress:** reader briefs (git-ignored) in `re/scratch/task33/*.md`.
- **Free space at the start:** 57 GB.
