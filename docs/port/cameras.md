# AC's cameras (Task 46)

## Resume here

- Step 0: pit_mouse crash fixed (9188050); the page's first start compared with a new desktop
  player (below). Archive of old kept results: running (`tools/proofs/archive_kept.sh --move`).
- Next: step 1, the audit table of every camera, sent to the user before step 2.

## Step 0: housekeeping

### The GUI pit_mouse crash

The GUI oracle made the escape menu a plain `Control` (0x178 bytes, `gui_ac_hud.rs`); the pit
sequence reads `escMenu+0x1a8` (the menu's setup screen) and puts in its own only when it reads 0.
That word was past the block, so heap garbage: the game followed it into
`SetupScreen::isFixedFuel`. The block is now 0x400 zeroed bytes. GUI `pit_mouse` 10 runs: command
log and pixels IDENTICAL in all 74 frames every time, no crash.

### A new player's first start: page against desktop

Task 45c's open question (g-force 1.4 against 1.0, AI level 98 against 100) was not a page
difference: the desktop launcher's `--player-cfg` (on by default) copies this PC's own
`Documents\Assetto Corsa\cfg` into the new player's folder first. A new AC player has none; with
`--player-cfg none` the desktop writes 1.4 and 98, the page's values. The page needs no change for
that.

Found on the way: after `ac://home` the page let the theme's old document run on until the frame
reloaded (one more career start and two more Drive restores, all written to `race.ini`); the
desktop's WebView runs none of it. The page now answers the leaving document's requests empty and
passes none to the launcher (`web/theme_glue.js`, `web/launcher.js`).

Proof: `tools/web_checks/task46/first_run.py` (desktop `first_desk.txt`, page `first.txt`, both
from an empty folder, language picked, Drive opened in both documents):

| | desktop | page |
|---|---|---|
| files in the player's folder | 58 | 58 |
| `cfg/` files that differ | `video.ini` (page: its window size, 800x600 windowed, by design), `race.ini` (one trailing empty line) | |
| other files that differ | `launcherdata/cache_*.json`: the AC folder's path (the page's is `C:/assettocorsa`) and special events' listing order | |
| requests (counted) | 3757 | 3799 |

The requests left that differ: the page opens Drive by itself when the theme starts, once more than
the desktop script clicks it (the Drive restore writes the same values again); the theme's career
start finishes in the page's first document and not in the desktop's (timing: on the desktop the
language click lands while the career is still loading, "Career reloading language"); and spelling only
(`getCfg`/`getcfg`, `getvideomodes`'s slash, `generatecss` times).
