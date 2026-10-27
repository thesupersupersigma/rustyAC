#!/bin/sh
# Download faults, old page (v0.38.2) against the fixed one. Everything on 127.0.0.1.
#   sh tools/web_checks/task38d/dl.sh old|new [case ...]
# cases: clean cut wrong zero short 503x5 spoiled; one line per case in dl_<side>.txt
cd "$(dirname "$0")/../../.." || exit 1
side=$1
shift
here=re/scratch/task38d
port=8188
[ "$side" = new ] && sh tools/web_checks/task38d/newsite.sh
site=$here/site_$side
url="http://127.0.0.1:$port/?go=1&car=bmw_z4_gt3&track=magione&spawn=pit"
read_diag='return { log: window.rustyac.log.slice(-4), diag: (JSON.parse(localStorage.getItem("rustyac.diagnostics") || "[]").pop() || { events: [] }).events.filter((e) => !/^\S+ download: .*downloaded$/.test(e)) };'
printf '%s' "$read_diag" > $here/diag.js

serve() { # serve <fault args...>
    py web/serve.py --site "$site" --pack $here/pack --port $port --quiet "$@" > $here/serve.log 2>&1 &
    srv=$!
    sleep 2
}
check() { # check <case> <profile> [more check.mjs args]
    name=$1 prof=$2
    shift 2
    node web/check.mjs --url "$url" --seconds 2 --profile "$here/prof_$prof" --timeout 300 "$@" > $here/out_${side}_$name.json 2> $here/out_${side}_$name.err
}
summary() { # one line: the case, the page's end state, its error, what diagnostics say
    py - "$1" "$here/out_${side}_$1.json" <<'PY'
import json, sys
name, path = sys.argv[1], sys.argv[2]
try:
    r = json.load(open(path, encoding='utf-8'))
except Exception as e:
    print(f'{name} | no report ({e})'); sys.exit()
s = r.get('script') or {}
diag = [e for e in (s.get('diag') or []) if 'download' in e or 'cache' in e or 'error' in e]
print(f"{name} | state {r.get('state')} | error {r.get('error')} | " + ' / '.join(diag)[:900])
PY
}

for c in "$@"; do
    case $c in
    clean) serve; check clean clean --script $here/diag.js ;;
    cut) serve --fault cut:magione.kn5; check cut cut --script $here/diag.js ;;
    wrong) serve --fault wrong:bmw_z4_gt3/data.acd; check wrong wrong --script $here/diag.js ;;
    zero) serve --fault zero:bmw_z4_gt3/data.acd; check zero zero --script $here/diag.js ;;
    short) serve --fault short:bmw_z4_gt3/data.acd; check short short --script $here/diag.js ;;
    503x5) serve --fault 503:bmw_z4_gt3/data.acd --fault 503:bmw_z4_gt3/data.acd --fault 503:bmw_z4_gt3/data.acd --fault 503:bmw_z4_gt3/data.acd --fault 503:bmw_z4_gt3/data.acd
        check 503x5 503 --script $here/diag.js ;;
    spoiled)
        # a first visit keeps the files; the script spoils one and adds an old pack's file;
        # the second visit reads the browser's storage
        serve; check spoiled_first spoiled --keep-profile --script tools/web_checks/task38d/spoil.js
        kill $srv 2>/dev/null; wait $srv 2>/dev/null
        summary spoiled_first >> $here/dl_$side.txt
        serve; check spoiled spoiled --keep-profile --script $here/diag.js ;;
    esac
    kill $srv 2>/dev/null; wait $srv 2>/dev/null
    summary $c >> $here/dl_$side.txt
    rm -rf "$here/prof_"* "$here/prof_"*.rustyac-check
done
