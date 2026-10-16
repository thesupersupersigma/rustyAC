# Assetto Corsa's launcher

## Resume here

- Done: `crates/rustyac-launcher` (bridge, `SetupUserDirectory`, WebView2 shell with the
  player's own theme, scripted runs, `--transcript`). The bridge answers 59 of 59 probe
  requests byte for byte like AssettoCorsa.exe (`re/scratch/task35/cmp.sh req/probe.txt probe`,
  then `python tdiff.py out/probe_o.txt out/probe_r.txt`); written files identical.
- Next: Practice → race.ini → rustyac.exe → back; other sessions; Options; results page;
  screenshots; this report.

(The full report follows when the launcher is done.)
