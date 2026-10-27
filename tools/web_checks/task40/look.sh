# D3D11 full_frame pictures of GUI scripts, for choosing the sequences (no compare).
# usage: sh tools/web_checks/task40/look.sh name "script" [name "script"]...
cd "$(dirname "$0")/../../.." || exit 1
out=re/scratch/task40/ff/look
mkdir -p $out
while [ $# -ge 2 ]; do
    ./target/release/examples/full_frame.exe --gui "$2" --out $out --name "$1" --size 1280x720 --video-ini re/scratch/task40/high.ini --car ks_ferrari_f2004 --track magione $EXTRA 2>&1 | tail -n 1 | cut -c1-200
    python -c "from PIL import Image; Image.open('$out/$1.png').convert('RGB').resize((960,540)).save('$out/$1_s.png')"
    shift 2
done
