#!/bin/sh
# Task 43b step 2: a live pit stop through the old mouse menu. The game's AI never uses the menu
# (AIDriver::stepSplineSelection puts its car into the box itself) and is not called in by its
# own stint in a race's last two laps; here --ai-drive drives a keyboard driver's car
# (INPUT_METHOD=KEYBOARD, so --pit-click gives the mouse menu) and --ai-pit-at calls it in early.
# PitStop goes Idle -> WaitingForDriver -> Choosing (in the box, under 1.5 km/h); --pit-click
# picks tyre 2, fuel up twice, body repair and CONFIRM in rounds from FROM to TO every EVERY s
# (clicks outside Choosing do nothing; a CONFIRM with nothing picked ends the stop at 0 s).
# Recorded, then replayed with 0 / 1 / 8 workers (the clicks travel in <file>.pit). The run of
# the report: SESSIONS=r6 SECS=300 FROM=211 TO=212 DRIVE="--ai-drive --ai-pit-at 150"
#   sh tools/web_checks/task43b/pit_live.sh
set -u
cd "$(dirname "$0")/../../.." || exit 1
OUT=re/scratch/task43b/pit
EXE=target/release/rustyac.exe
rm -rf $OUT && mkdir -p $OUT
sed 's/^INPUT_METHOD=.*/INPUT_METHOD=KEYBOARD/' "$USERPROFILE/Documents/Assetto Corsa/cfg/controls.ini" > $OUT/controls.ini
COMMON="--headless --no-race-ini --car bmw_m3_e30 --track magione --sessions ${SESSIONS:-p10} --controls $OUT/controls.ini"
CLICKS=""
t=${FROM:-2}
while [ $t -lt ${TO:-400} ]; do
  CLICKS="$CLICKS --pit-click tyre:2@$t --pit-click fuel_up@$t.2 --pit-click fuel_up@$t.4 --pit-click repair_body@$t.6 --pit-click confirm@$t.8"
  t=$((t + ${EVERY:-2}))
done
timeout 900 $EXE $COMMON --duration ${SECS:-120} ${DRIVE:-} $CLICKS --record $OUT/a.ryin --out-dir $OUT/live > $OUT/live.txt 2>&1 < /dev/null
echo "live: exit $?"
grep -iE "pit|state hash|steps in" $OUT/live.txt | grep -v LOADING | head -20
for w in 0 1 8; do
  timeout 900 $EXE --headless --replay $OUT/a.ryin --physics-threads $w --state-hash --controls $OUT/controls.ini --out-dir $OUT/r$w > $OUT/replay_$w.txt 2>&1 < /dev/null
  echo "replay $w: exit $? $(grep -i 'state hash' $OUT/replay_$w.txt) $(grep -i 'pit stops of your car' $OUT/replay_$w.txt)"
done
