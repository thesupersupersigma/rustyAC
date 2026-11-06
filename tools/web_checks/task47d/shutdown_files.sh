#!/bin/bash
# Task 47d step 3: the apps' files at the end of a drive, desktop vs page. One session (car, track,
# a video.ini in triple-screen mode, the same --gui-do clicks on the Triple screen app's spinners):
# rustyac.exe writes them into its --cfg-dir / savedData when it closes; the page's code
# (full_frame --shutdown: rustyac_web::full::shutdown_files) keeps them. Every file the page keeps
# must equal the desktop's at the same place. (The post-processing filter, chat export and force
# feedback files take the same write_files path; this session makes none of them.)
cd "$(dirname "$0")/../../.." || exit 1
D=re/scratch/task47d/step3
rm -rf $D; mkdir -p $D/desk/docs/cfg $D/desk/own/cfg $D/page
# the screens the game's app writes at its start values (Task 47c's run6), in both Documents
mkdir -p $D/page/no_documents/cfg
cp re/scratch/task47c/run6/cfg/triple_screen.ini $D/desk/docs/cfg/
cp re/scratch/task47c/run6/cfg/triple_screen.ini $D/page/no_documents/cfg/
printf '[CAMERA]\r\nMODE=TRIPLE\r\n\r\n[VIDEO]\r\nWIDTH=1280\r\nHEIGHT=720\r\nFULLSCREEN=0\r\n' > $D/video.ini
# the Triple app's first and last spinners' "+" and "-" (the form at (0, 80) on 1920 x 1080), then its rendering
S='show:triple_screen;click:220:515;click:220:515;click:220:515;click:40:665;click:560:665;click:560:665'
CAR=${CAR:-bmw_m3_e30} TRACK=${TRACK:-magione}
target/release/rustyac.exe --car $CAR --track $TRACK --no-race-ini --video-ini "$(cygpath -am $D/video.ini)" --width 1920 --height 1080 \
    --gui-do "$S" --duration 4 --documents "$(cygpath -am $D/desk/docs)" --cfg-dir "$(cygpath -am $D/desk/own/cfg)" > $D/desk.txt 2>&1
echo "desktop exit $?"
./target/release/examples/full_frame.exe --car $CAR --track $TRACK --warp --size 1920x1080 --video-ini $D/video.ini --gui "$S" --shutdown \
    --out $D/page --name p > $D/page.txt 2>&1
echo "page exit $?"
same=0 diff=0
for f in $(cd $D/page/p.shutdown && find . -type f | sort); do
    rel=${f#./gui_cfg/}; rel=${rel#./}
    case $f in ./gui_cfg/*) d=$D/desk/own/cfg/$rel ;; *) d=$D/desk/own/$rel ;; esac
    if cmp -s "$D/page/p.shutdown/$f" "$d"; then echo "IDENTICAL $rel ($(wc -c < "$d") bytes)"; same=$((same+1)); else echo "DIFFERENT $rel"; diff=$((diff+1)); fi
done
echo "desktop's own folder: $(cd $D/desk/own && find . -type f | sort | tr '\n' ' ')"
echo "PROOF shutdown_files identical=$same different=$diff"
