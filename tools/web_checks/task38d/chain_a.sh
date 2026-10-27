# waits for the orphaned fresh.sh (pid 814) to end, then runs the game-loading areas one by one
while kill -0 814 2>/dev/null; do sleep 30; done
tail -n 1 target/proofs/fresh.log
CARGO_BUILD_JOBS=4 sh tools/proofs/run_all.sh sound render gui replays ai_page
