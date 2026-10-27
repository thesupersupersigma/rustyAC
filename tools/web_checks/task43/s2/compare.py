"""Task 43 step 2: rustyAC's online race_out.json (per client) against acServer's own results files.
   py -I tools/web_checks/task43/s2/compare.py re/scratch/task43/s2/run"""
import glob
import json
import os
import sys

run = sys.argv[1]
server = {}
for f in sorted(glob.glob(os.path.join(run, 'server_results', '*.json'))):
    j = json.load(open(f, encoding='utf-8-sig'))
    server[j['Type']] = j
kinds = {1: 'PRACTICE', 2: 'QUALIFY', 3: 'RACE'}
bad = 0
for name in ('a', 'b'):
    out = json.load(open(os.path.join(run, 'out_' + name, 'race_out.json'), encoding='utf-8'))
    players = [p['name'] for p in out['players']]
    print(f"== {name}: players {[p for p in players if p]} of {len(players)} cars, track {out['track']}, {out['number_of_sessions']} sessions")
    for s in out['sessions']:
        srv = server.get(kinds.get(s['type']))
        ours = sorted((players[l['car']], l['time'], tuple(l['sectors']), l['cuts']) for l in s['laps'])
        theirs = sorted((l['DriverName'], l['LapTime'], tuple(l['Sectors']), l['Cuts']) for l in srv['Laps']) if srv else []
        same = ours == theirs
        bad += not same
        counts = {players[i]: n for i, n in enumerate(s['lapstotal']) if players[i]}
        best = {players[b['car']]: b['time'] for b in s['bestLaps']}
        srv_best = {r['DriverName']: r['BestLap'] for r in srv['Result'] if r['DriverName'] and r['BestLap'] != 999999999} if srv else {}
        order = [players[c] for c in s.get('raceResult') or [] if players[c]]
        srv_order = [r['DriverName'] for r in srv['Result'] if r['DriverName']] if srv else []
        line = f"  {s['name']}: {len(ours)} laps {'= acServer' if same else 'DIFFERENT from acServer'}; lapstotal {counts}; best {best}"
        if s['type'] != 3:
            line += f" ({'= acServer' if best == srv_best else 'acServer ' + str(srv_best)})"
            bad += best != srv_best
        else:
            line += f"; order {order} ({'= acServer' if order == srv_order else 'acServer ' + str(srv_order)})"
            bad += order != srv_order
        print(line)
print('ALL THE SAME' if bad == 0 else f'{bad} DIFFERENCES')
