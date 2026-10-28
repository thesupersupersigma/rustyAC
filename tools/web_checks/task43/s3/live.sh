#!/bin/sh
# Task 43 step 3: the old mouse pit menu in rustyac.exe itself. A keyboard driver's controls
# (a copy of Documents' controls.ini with INPUT_METHOD=KEYBOARD), an AI driver in the player's
# car called in at 340 s of the run (a race of 3 laps; its start comes at about 321 s of a
# headless run), the menu clicked by --pit-click: "Medium", fuel up twice, body repair,
# CONFIRM, every 2 s from 340 s on (clicks outside the state "choosing" do
# nothing, so the first round the car is in its box makes the stop); recorded, then replayed
# with 0, 1 and 8 workers (the clicks travel in <file>.pit).
# Usage: sh tools/web_checks/task43/s3/live.sh
set -u
cd "$(dirname "$0")/../../../.."
OUT=re/scratch/task43/s3/live
EXE=target/release/rustyac.exe
mkdir -p $OUT
sed 's/^INPUT_METHOD=.*/INPUT_METHOD=KEYBOARD/' "$USERPROFILE/Documents/Assetto Corsa/cfg/controls.ini" > $OUT/controls.ini
COMMON="--headless --no-race-ini --car bmw_m3_e30 --track magione --sessions r3 --controls $OUT/controls.ini"
CLICKS=""
t=340
while [ $t -lt 600 ]; do
  CLICKS="$CLICKS --pit-click tyre:2@$t --pit-click fuel_up@$t.2 --pit-click fuel_up@$t.4 --pit-click repair_body@$t.6 --pit-click confirm@$t.8"
  t=$((t + 2))
done
$EXE $COMMON --duration 300 --ai-drive --ai-pit-at 340 $CLICKS --record $OUT/a.ryin --out-dir $OUT/live > $OUT/live.txt 2>&1 < /dev/null
echo "live: exit $?"
grep -iE "state hash|pit|mouse|steps" $OUT/live.txt | head -20
for t in 0 1 8; do
  $EXE --headless --replay $OUT/a.ryin --physics-threads $t --state-hash --controls $OUT/controls.ini --out-dir $OUT/r$t > $OUT/replay_$t.txt 2>&1 < /dev/null
  echo "replay $t: exit $? $(grep -i 'state hash' $OUT/replay_$t.txt)"
done
ls -la $OUT/a.ryin.pit && cat $OUT/a.ryin.pit
