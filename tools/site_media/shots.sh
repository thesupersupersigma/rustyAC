#!/bin/sh
# SPDX-License-Identifier: MIT OR Apache-2.0
# usage: sh shots.sh  (from the rustyAC folder); writes re/scratch/site_media/final/*.png and commands.txt
R=target/release/rustyac.exe
O=re/scratch/site_media/final
mkdir -p $O
Q="--width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --motion-blur 0 --supersample 2 --no-hud"
F=${1:-.}
[ "$F" = . ] && : > $O/commands.txt
shot() { name=$1; shift; echo $name | grep -q "$F" || return 0; echo "$name: rustyac.exe $* --screenshot $name" >> $O/commands.txt; $R "$@" --screenshot $O/$name 2>&1 | grep "png:" ; }
shot renderer_core_spa.png --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 12 $Q
shot renderer_core_spa_debug.png --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 12 $Q --debug-view
shot renderer_core_monza.png --car bmw_1m --track monza --no-race-ini --autodrive --at 12 $Q
shot renderer_core_monza_debug.png --car bmw_1m --track monza --no-race-ini --autodrive --at 12 $Q --debug-view
shot renderer_core_laguna_seca.png --car bmw_m3_e30 --track ks_laguna_seca --no-race-ini --autodrive --at 12 $Q
shot renderer_core_laguna_seca_debug.png --car bmw_m3_e30 --track ks_laguna_seca --no-race-ini --autodrive --at 12 $Q --debug-view
B="--car ks_ferrari_f2004 --track spa --no-race-ini --flat-out --flat-out-brake 0.6 --no-auto-clutch --at 5 --lead-in 5"
shot hero_burnout.png $B $Q --free-camera 5,0.5,3/0,0.5,-1/50
shot og_burnout.png $B --width 2400 --height 1260 --graphics max --cube-size 2048 --aa 8 --motion-blur 0 --supersample 2 --no-hud --free-camera 5,0.5,3/-0.9,0.7,0.2/50
G="--width 2560 --height 1440 --graphics max --cube-size 2048 --aa 8 --supersample 2 --no-hud"
shot gallery_f2004_spa_chase.png --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 21.6 --lead-in 1 $G --motion-blur 4
shot gallery_f2004_spa_trackside.png --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 19 --lead-in 1 $G --motion-blur 0 --free-camera 4,0.7,11/0,0.5,0/22
shot gallery_f2004_cockpit.png --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 21.6 --lead-in 1 --camera cockpit $G --motion-blur 0
shot gallery_night_lights.png --car bmw_1m --track spa --no-race-ini --sun 86 --show-lights --autodrive --at 12 --lead-in 1 $G --motion-blur 0 --free-camera -3.5,0.9,7/0,0.6,0/40
shot gallery_heavy_clouds.png --car ks_ferrari_f2004 --track spa --no-race-ini --autodrive --at 30 --lead-in 1 --camera chase2 --weather 7_heavy_clouds --sun 30 $G --motion-blur 0
shot gallery_crash_damage.png --car bmw_1m --track spa --no-race-ini --show-damage 70,65,80,75,80 --at 2 $G --motion-blur 0 --free-camera -3,1.0,5/0,0.6,0.5/40
shot gallery_f40_monza.png --car ferrari_f40 --track monza --no-race-ini --autodrive --at 12 --lead-in 1 $G --motion-blur 0 --free-camera -4,0.7,7/0,0.6,0/38
shot gallery_488_gt3_laguna_seca.png --car ks_ferrari_488_gt3 --track ks_laguna_seca --no-race-ini --autodrive --at 14 --lead-in 1 $G --motion-blur 0 --free-camera -4.5,0.9,8/0,0.6,0/35
