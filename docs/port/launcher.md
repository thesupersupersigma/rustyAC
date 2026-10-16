# Assetto Corsa's launcher

## Resume here

- Done: `crates/rustyac-launcher` (bridge, `SetupUserDirectory`, WebView2 shell with the
  player's own theme, scripted runs, `--transcript`). The bridge answers 59 of 59 probe
  requests byte for byte like AssettoCorsa.exe (`re/scratch/task35/cmp.sh req/probe.txt probe`,
  then `python tdiff.py out/probe_o.txt out/probe_r.txt`); written files identical.
- Practice → race.ini → rustyac.exe → back works: `re/scratch/task35/scripts/practice.txt`
  (run as in `cmp.sh`'s neighbour commands: `rustyac-launcher.exe --docs <fake/live> --game-arg
  --headless --game-arg --duration --game-arg 160 --game-arg --no-steam --game-arg --autodrive
  --script scripts/practice.txt --log out/practice.log --offscreen 1600x900`). The launcher
  passes race.ini's RACE/MODEL, TRACK, CONFIG_TRACK as `--car/--track/--layout` (rustyac.exe
  takes those from its command line only). A run without laps shows "Invalid results", as in
  the theme's own rules.
- Next: replay the logged requests through oracle + port (`log2req.py`); Quick race / Race
  weekend / Hotlap / Options scripts (written in scripts/); screenshots; this report.

(The full report follows when the launcher is done.)
