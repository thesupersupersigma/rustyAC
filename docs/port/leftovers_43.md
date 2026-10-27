# Task 43: player-facing leftovers, and pack updates by delta

## Resume here

- Done: step 1 (`web_pack --delta`).
- Next: step 2 (`race_out.json` after an online session on desktop), then steps 3 to 7.
- Scratch: `re/scratch/task43/`. Rerun scripts: `tools/web_checks/task43/`.
- Disk at the start: 67 GB free.

## Step 1: `web_pack --delta <server manifest>`

The pack on the server is about 28.6 GB, too big for a second copy on this PC. `--delta` builds the
same pack as before, but it only writes files the live pack doesn't have yet. A file counts as
unchanged when it has the same path, size and SHA-256 as in the live `manifest.json`; such a file
is read for its hash but not written. The new full `manifest.json` goes with the changed files.
The tool prints how many files changed and how big they are, and which files of the live pack the
new pack no longer has. `--out` must be an empty folder, so it holds exactly the upload.

How to use it (the pack on the server under `/srv/rustyac/preview`, for example):

```sh
scp me@server:/srv/rustyac/preview/manifest.json live_manifest.json
cd tools/web_pack && cargo run --release -- --out ../../dist-web/delta --delta ../../live_manifest.json
# prints: delta: N new or changed files, X MB (plus manifest.json) in ../../dist-web/delta ...
#         the new pack no longer has M files of the live pack (...): delete them there
#           gone: content/tracks/...
scp -r ../../dist-web/delta/. me@server:/srv/rustyac/preview/
# then delete the "gone:" files on the server (optional: the page never asks for them)
```

`--config`, `--ac` and `--dry-run` work with `--delta` as without it (`--dry-run` writes nothing).

Proof (`sh tools/web_checks/task43/s1/delta.sh`, output in `re/scratch/task43/s1`): an old pack
(F2004 at Laguna Seca and Magione) and a new one (F2004 and Z4 at Laguna Seca: a car more, a track
less). One file of the old pack (`content/cars/ks_ferrari_f2004/sfx/ks_ferrari_f2004.bank`) is
changed, both its bytes and its hash in the manifest, so the delta has to carry it again. Then:
old pack + delta copied over it − the "gone" files = the server's update. That result is compared
with `diff -r` against the full new pack.

| Pack | Files | Size |
|---|---|---|
| old (full) | 1871 + manifest | 1451.4 MB |
| new (full) | 1926 + manifest | 1234.4 MB |
| delta | 100 + manifest | 179.5 MB |
| gone (to delete on the server) | 44 | 319.8 MB |
| old + delta − gone vs new, `diff -r` | 1927 vs 1927 | **identical** (the changed `.bank` is in the delta) |

Test: `a_manifest_reads_back` (`tools/web_pack`): a manifest written by `Manifest::to_json`, with
`"`, `\` and a control character in its paths, reads back with every path, size and hash.
