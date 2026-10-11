#!/bin/sh
# SPDX-License-Identifier: MIT OR Apache-2.0
# usage: sh audio.sh (from the rustyAC folder): raw clips into re/scratch/site_media/raw, commands into raw/commands.txt
E=target/release/examples/site_clips.exe
O=re/scratch/site_media/raw
mkdir -p $O
: > $O/commands.txt
V="--volume 0.12"
clip() { name=$1; shift; echo "$name: site_clips $*" >> $O/commands.txt
  for try in 1 2 3; do rm -f $O/$name.wav; $E "$@" --out $O/$name.wav > $O/$name.log 2>&1; size=$(stat -c %s $O/$name.wav 2>/dev/null || echo 0); if [ "$size" -gt 100000 ] && grep -q "written" $O/$name.log; then echo "$name ok ($size bytes, try $try)"; return; fi; done; echo "$name FAILED"; }
for rpm in idle 4000 7000 10000 13000 16000; do
  r=$rpm; [ $rpm = idle ] && r=4000
  clip engine_on_$rpm steady --rpm $r --gas 1 --seconds 14 $V
  clip engine_off_$rpm steady --rpm $r --gas 0 --seconds 14 $V
done
clip limiter drive --neutral --seconds 16 $V
clip wheelspin steady --rpm 18500 --gas 1 --spin 110 --slip 1 --speed 5.56 --engine-level 0 --seconds 14 $V
clip wheelspin_flat steady --rpm 18500 --gas 1 --spin 110 --slip 1 --speed 5.56 --inflation 0 --engine-level 0 --seconds 8 $V
clip pit_wheelgun event --path event:/common/screw --seconds 6 $V
clip pit_wheelgun_off event --path event:/common/unscrew --seconds 6 $V
for n in 1 2 3; do clip backfire_$n event --path event:/cars/ks_ferrari_f2004/backfire_ext --param throttle=0 --at exhaust --base-volume 2.5 --seconds 4 $V; done
clip burnout_reference drive --track spa --brake 0.6 --after-pop 2 --seconds 60 $V
