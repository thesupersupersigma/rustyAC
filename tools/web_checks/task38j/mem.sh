#!/bin/bash
# Task 38j step 3: Full drives one after another in one page (Laguna Seca, Spa, three times
# each), the wasm memory and the files held after each.
# usage: mem.sh   (needs web/build.py's build; writes re/scratch/task38j/mem/)
cd /c/Users/thesupersupersigma/Desktop/project5/rustyAC
C=re/scratch/task38j/cond; M=re/scratch/task38j/mem; PACK=re/gold/web/pack
mkdir -p $M
sed 's/^TRACK=ks_laguna_seca/TRACK=spa/' $C/race.ini > $M/race_spa.ini
js() { node -e 'process.stdout.write(JSON.stringify(require("fs").readFileSync(process.argv[1],"utf8")))' "$1"; }
cat > $M/mem.js <<EOF
for (let i = 0; i < 600 && !(window.rustyac && window.rustyac.driveRace && window.rustyac.memory && !document.getElementById('drive').disabled); i++) await new Promise((r) => setTimeout(r, 100));
const races = { laguna: $(js $C/race.ini), spa: $(js $M/race_spa.ini) };
const assists = $(js $C/assists.ini);
const rows = [{ drive: 'none', ...window.rustyac.memory() }];
for (let round = 0; round < ${ROUNDS:-3}; round++) for (const name of ['laguna', 'spa']) {
  try { await window.rustyac.driveRace(races[name], assists); } catch (e) { rows.push({ drive: name, error: String(e) }); return rows; }
  await new Promise((r) => setTimeout(r, 3000));
  rows.push({ drive: name, state: window.rustyac.state, ...window.rustyac.memory() });
}
return rows;
EOF
py -3 web/serve.py --port 8152 --ac "C:/Program Files (x86)/Steam/steamapps/common/assettocorsa" --quiet > $M/serve.log 2>&1 &
sp=$!
sleep 5
node web/check.mjs --url "http://127.0.0.1:8152/?testfolder=/ac-test/&seed=28005&backend=full" --menu --script $M/mem.js --screenshot $M/last.png --profile $M/chrome_profile --port 9352 --timeout 1200 --flags "--use-webgpu-adapter=swiftshader" > $M/mem.json 2> $M/mem.err
echo "check.mjs exit $?"
kill $sp 2>/dev/null
rm -rf $M/chrome_profile
node -e 'const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); console.log(`error ${r.error}`); for (const x of r.script || []) console.log(JSON.stringify(x));' $M/mem.json
