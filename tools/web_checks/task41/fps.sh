#!/bin/bash
# fps.sh <name> <query>: a 40 s autodrive in headless Chrome on the scratch pack
cd "$(dirname "$0")/../../.."
D=re/scratch/task41
P=${PORT:-8161}
py -3 web/serve.py --port $P --site $D/site --pack $D/pack --quiet > $D/page/serve.log 2>&1 &
SP=$!
sleep 4
node web/check.mjs --url "http://127.0.0.1:$P/?go=1&autodrive=1&car=ks_ferrari_f2004&track=spa&$2" --seconds ${SECS:-40} --screenshot $D/page/$1.png --script $D/page/fps_script.js --profile $D/chrome_profile --port $((P+1200)) --timeout 900 --flags "--autoplay-policy=no-user-gesture-required ${FLAGS}" > $D/page/$1.json 2> $D/page/$1.err
echo "exit $?"
kill $SP 2>/dev/null
