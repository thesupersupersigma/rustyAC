#!/bin/bash
# Task 46: rustyac.exe with the documents the page's run had (AC's own camera files, as a new
# player's launcher copies them, and the race.ini the page's launcher wrote, its car and track): its camera
# settings line against the page's (cams.sh first).
cd "$(dirname "$0")/../../.." || exit 1
AC="C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"
S=re/scratch/task46/page; D=$S/desk
rm -rf $D; mkdir -p $D/docs/cfg $D/cfg
cp "$AC/cfg/camera_onboard.ini" "$AC/cfg/camera_manager.ini" $D/docs/cfg/
cp $S/race.ini $D/docs/cfg/race.ini
key() { python -c "import configparser,sys;c=configparser.RawConfigParser(strict=False);c.read('$S/race.ini');print(c['RACE'][sys.argv[1]])" "$1"; }
target/release/rustyac.exe --car "$(key MODEL)" --track "$(key TRACK)" --documents "$(cygpath -am $D/docs)" --cfg-dir "$(cygpath -am $D/cfg)" --duration 4 > $D/run.txt 2>&1
desk=$(grep '^camera settings: ' $D/run.txt | sed 's/^camera settings: //')
page=$(python -c "
import json;r=json.load(open('$S/cams.json',encoding='utf-8'))
print(next(json.loads(f['value'])['camera_settings'] for f in r['flow'] if str(f.get('value')).startswith('{\"camera\"')))")
echo "desktop: $desk"; echo "page:    $page"
[ -n "$desk" ] && [ "$desk" = "$page" ] && echo "camera settings: IDENTICAL" || echo "camera settings: DIFFERENT"
