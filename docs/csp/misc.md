# Misc: everything else CSP touches

Two detail files cover what is left once physics, rendering, UI, audio, input, online and VR are
taken out. This file is the index.

| File | Area | Hooked / looked-up | Port difficulty |
|---|---|---|---|
| [`misc_ai_race.md`](misc_ai_race.md) | AI drivers (19 `AIDriver` hooks, AI line loaders and recorder, 11 `RaceEngineer` hooks), `RaceManager` / `FlagManager` / timing, the replay manager and the **extended replay file**, ghost car, comparative telemetry, pit stop and crew, the Lua new-mode host (10 shipped modes) | ~118 / ~40 (first extraction; 92 table rows were later reclassified, see [`attach.md`](attach.md)) | **M-L**: rustyAC's AI, referee, pits, replays are bit-exact against plain `acs.exe`; every CSP change is a delta on a ported function; AI laps are statistical because CSP's pool and rand stream differ |
| [`misc_core.md`](misc_core.md) | the config and content-loading seam (`INIReader` 12 hooks, `Path`, `FolderEncrypter`, `FileChangeObserver`), `Sim` / `Game`, cameras (chaser and cockpit Lua kinds, free camera), shared-memory additions, Steam, the Python bridge (213 `ac.ext_*` functions), crash handling, hot reload, `cache/`, CSP's own 186 type names | ~134 / ~200 (most lookups are CSP's own type names) | **M for what rustyAC needs, L for a faithful copy**: the seams exist only because `acs.exe` is closed; rustyAC needs the behaviours, not the hooks |

Highlights worth knowing before reading:

- **`INIReader` is the config seam of the whole patch.** The same hooks that parse `extended-N`
  (see [`physics.md`](physics.md)) serve CSP's merged config, embedded default `ac/*.ini`
  copies and the `data_override` / skins overlay, so CSP content can be read through AC's own
  reader. A native port implements this once, in the content loader.
- **The extended replay file** and the ghost / comparative-telemetry changes are the only on-disk
  format additions outside configs; replays recorded under CSP need a CSP-aware reader.
- **Reproducibility of AI laps** is an open experiment (the AI draws from the physics thread's
  C `rand()`): recording the same AI lap twice with CSP on decides whether AI recordings can be
  matched exactly or only statistically (`misc_ai_race.md` section 8, question 1).
