# web_checks

The rerun scripts of Tasks 38d to 43 (browser, server panel, sound, graphics and HUD checks), moved out
of the git-ignored `re/scratch/task…/` by the scratch cleanup of 2026-10-27. Each folder keeps its task
name, so a script's `cd "$(dirname "$0")/../../.."` still lands in the repo root.

Only the scripts live here. What they read and write (test packs, site builds, Chrome profiles, run
output) is still `re/scratch/<task>/…` and is gone: make a pack with `tools/web_pack` from the `.toml`
next to the script, build the site with `web/build.py`, and the scripts recreate the rest. They were
moved with their paths adjusted, not rerun.
