# Full mode's fixed frame with AC's GUI, natively on D3D11 and on wgpu (GPU), logs compared.
# usage: sh tools/web_checks/task40/ff.sh <name> "<gui-do script>" [full_frame args...]
cd "$(dirname "$0")/../../.." || exit 1
name=$1; script=$2; shift 2
out=re/scratch/task40/ff
mkdir -p $out/d3d $out/wgpu
./target/release/examples/full_frame.exe --gui "$script" --out $out/d3d --name "$name" "$@" || exit 1
RUSTYAC_WGPU_ADAPTER=gpu ./target/full_wgpu/release/examples/full_frame.exe --gui "$script" --out $out/wgpu --name "$name" "$@" || exit 1
python tools/web_checks/task40/cmp_log.py "$out/d3d/$name.kgllog" "$out/wgpu/$name.kgllog"
