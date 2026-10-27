#!/bin/bash
# page_proof.sh <tape name> <camera>: the sound proof in headless Chrome on the scratch pack
cd "$(dirname "$0")/../../.."
D=re/scratch/task41
P=${PORT:-8151}
py -3 web/serve.py --port $P --site $D/site --pack $D/pack --quiet > $D/page/serve.log 2>&1 &
SP=$!
sleep 4
cat > $D/page/proof_script.js <<JS
return await window.rustyac.soundProof('tapes/$1.audiotape', '$2', 5);
JS
node web/check.mjs --url "http://127.0.0.1:$P/" --menu --script ${SCRIPT:-$D/page/proof_script.js} --profile $D/chrome_profile --port $((P+1200)) --timeout 900 --flags "--autoplay-policy=no-user-gesture-required" > $D/page/chrome_$1_$2.json 2> $D/page/chrome_$1_$2.err
echo "exit $?"
kill $SP 2>/dev/null
