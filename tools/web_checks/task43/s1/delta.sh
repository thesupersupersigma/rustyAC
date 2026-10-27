#!/bin/sh
# Task 43 step 1: an older pack + `web_pack --delta <its manifest.json>` == a full new pack, file for file.
# old: F2004 at Laguna Seca and Magione; new: F2004 and Z4 at Laguna Seca (a car more, a track less),
# and one file of the old pack is "changed" (its bytes and its manifest hash) so the delta must
# carry it again. Everything stays in re/scratch/task43/s1; the game's folder is only read.
set -e
cd "$(dirname "$0")/../../../.."
s=re/scratch/task43/s1
tool=tools/web_pack/target/release/web_pack.exe
rm -rf $s/old $s/new $s/delta $s/merged
printf 'cars = ["ks_ferrari_f2004"]\ntracks = ["ks_laguna_seca", "magione"]\n' > $s/old.toml
printf 'cars = ["ks_ferrari_f2004", "bmw_z4_gt3"]\ntracks = ["ks_laguna_seca"]\n' > $s/new.toml
$tool --config $s/old.toml --out $s/old > $s/old.log
$tool --config $s/new.toml --out $s/new > $s/new.log
# change one file the new pack keeps: in the old pack's copy and in its manifest
f=$(grep -o '"path": "content/cars/ks_ferrari_f2004/[^"]*"' $s/old/manifest.json | head -1 | cut -d'"' -f4)
h=$(grep -F "\"path\": \"$f\"" $s/old/manifest.json | grep -o '[0-9a-f]\{64\}')
printf 'x' >> "$s/old/$f"
sed -i "s/$h/$(printf '0%.0s' $(seq 64))/" $s/old/manifest.json
echo "changed in the old pack: $f"
$tool --config $s/new.toml --out $s/delta --delta $s/old/manifest.json | tee $s/delta.log | tail -8
# the server's update: the old pack, the delta copied over it, the gone files deleted
cp -r $s/old $s/merged
cp -r $s/delta/. $s/merged/
sed -n 's/^  gone: //p' $s/delta.log | while IFS= read -r g; do rm "$s/merged/$g"; done
find $s/merged -type d -empty -delete
echo "files: new $(find $s/new -type f | wc -l), merged $(find $s/merged -type f | wc -l), delta $(find $s/delta -type f | wc -l)"
diff -r $s/new $s/merged && echo "IDENTICAL: old pack + delta == full new pack"
grep -q "^$f\$" /dev/null || test -f "$s/delta/$f" && echo "the changed file is in the delta"
