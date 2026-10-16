# The launcher's online pages, replays, stats and events (Task 36)

## Resume here

Done: the three small fixes (getguid/getid, the player's skin in race_out.json, the order of
getvideomodes). Next: the online pages (lobby, LAN, favourites, details, ping, password, join).

## The three small fixes

| Fix | What changed | Proof |
|---|---|---|
| `getguid` / `getid` | One id for both programs: `rustyac_game::identity::resolve`, the function rustyac.exe's join uses. With Steam: the Steam id. Without: rustyAC's own id from `rustyac_net.ini` beside the exe. Only if that fails, a new GUID (the original's answer without Steam). | Two launcher runs answer the same id three times each. A local acServer logs the same GUID when rustyac.exe joins, with Steam and with `--no-steam`. |
| The player's skin in `race_out.json` | A race.ini of one car (hotlap, practice) made no grid, so the player had no skin and `"skin": ""` was written. Now car 0 of `race_grid` gives the player its skin also then, and a car left alone keeps it. | `car_oracle race-ini` on the launcher's hotlap race.ini: **IDENTICAL**, car 0 skin `0_white_scorpion`. rustyac.exe on the same file writes `"skin": "0_white_scorpion"`. |
| `getvideomodes` order | `acVideoModes.dll` (disassembled): `CreateDXGIFactory` (not Factory1), `EnumAdapters(0)`, `EnumOutputs(0)`, `GetDisplayModeList(R8G8B8A8_UNORM, 0)`, the modes kept in DXGI's order, no sorting. The port now makes the same calls. | launcher_oracle (x86, the real dll) and the port: transcripts **identical**, 6,370 bytes. |
