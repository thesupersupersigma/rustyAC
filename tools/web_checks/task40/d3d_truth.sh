#!/bin/sh
# D3D11 port side of some GUI sequences: kgl logs + DirectWrite glyphs/layouts (ground truth)
cd "$(dirname "$0")/../../.."
O=re/scratch/task40/d3d
mkdir -p $O
for name in ${NAMES:-text base widgets_a hud_messages hud_cars esc_menu pit_menu setup_tabs@bmw_z4_gt3 app_essentials app_telemetry console esc_menu#it pause_menu#de lb_tab_12 end_race_12}; do
    f=$(echo "$name" | tr '@#' '__')
    rm -f $O/glyphs_$f.txt
    RUSTYAC_FW1_GLYPHS=$O/glyphs_$f.txt timeout 600 tools/render_oracle/target/release/render_oracle.exe gui-run --side port --gui "$name" --root re/scratch/task40/root --out $O --size 1920x1080 < /dev/null > $O/$f.out 2>&1
    echo "$name rc=$?"
done
