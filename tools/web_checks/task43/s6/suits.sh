#!/bin/bash
# Task 43 step 6: a Z4 in its "fanatec" skin (not the first by name) at Laguna Seca in Full mode,
# from the preview pack before the change ("old") and after it (old + the delta); which suit
# files the page fetched, and the picture.
#   sh tools/web_checks/task43/s6/suits.sh    (needs re/scratch/task43/s6/old and delta, web/build.py's site)
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC || exit 1
S=re/scratch/task43/s6; P=8194
[ -d $S/old ] && [ -d $S/delta ] || { echo "no $S/old or $S/delta" >&2; exit 1; }
sed -e 's/^TRACK=magione/TRACK=ks_laguna_seca/' -e 's/^MODEL=lotus_elise_sc/MODEL=bmw_z4_gt3/' -e 's/^SKIN=.*/SKIN=fanatec/' re/gold/web/pack/cfg/race.ini > $S/race.ini
js() { node -e 'process.stdout.write(JSON.stringify(require("fs").readFileSync(process.argv[1],"utf8")))' "$1"; }
cat > $S/look.js <<END
for (let i = 0; i < 600 && !(window.rustyac && window.rustyac.driveRace && !document.getElementById('drive').disabled); i++) await new Promise((r) => setTimeout(r, 100));
await window.rustyac.driveRace($(js $S/race.ini), $(js re/gold/web/pack/cfg/assists.ini));
await new Promise((r) => setTimeout(r, 8000));
const log = JSON.parse(localStorage.getItem('rustyac.diagnostics') || '[]').flatMap((v) => v.events || []);
return { state: window.rustyac.state, drive: window.rustyac.drive, suits: log.filter((l) => /texture\/(driver|crew)_|skins\/[^/]+\/skin\.ini/i.test(l)), asked: (window.rustyac.notes || '').split('\n').filter((l) => l.startsWith('files that were asked')) };
END
for name in old new; do
    [ $name = new ] && cp -r $S/delta/. $S/old/
    py -3 web/serve.py --port $P --pack $S/old --quiet > $S/$name.serve.log 2>&1 &
    sp=$!; sleep 4
    node web/check.mjs --url "http://127.0.0.1:$P/?backend=full&launcher=0" --menu --script $S/look.js --screenshot $S/$name.png --profile $S/prof --port 9375 --timeout 600 --flags "${FLAGS:---enable-features=Vulkan}" > $S/$name.json 2> $S/$name.err
    echo "$name: check.mjs exit $?"
    kill $sp 2>/dev/null; wait $sp 2>/dev/null; rm -rf $S/prof
    node -e 'const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); console.log(`error ${r.error}`); console.log(JSON.stringify(r.script, null, 1));' $S/$name.json
done
