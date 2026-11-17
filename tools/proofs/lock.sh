# SPDX-License-Identifier: GPL-3.0-or-later
#
# One proof run at a time on this PC, whatever the checkout (sourced by run_all.sh and lib.sh,
# never run): two runs at once would share the CPU, the memory and the game's kept results.
# The lock is the folder %USERPROFILE%\.rustyac\proof.lock, made with mkdir (atomic); its file
# "owner" holds three lines: the repo's path, the Windows PID of the run's shell, the start time.

proof_lock_dir=${USERPROFILE:-$HOME}/.rustyac/proof.lock

proof_unlock() {
    [ -z "$proof_lock_mine" ] || rm -rf "$proof_lock_dir"
    proof_lock_mine=
}

# is the shell with this Windows PID still there? (no answer from tasklist counts as yes:
# a lock is only ever taken over on proof)
proof_lock_alive() {
    tasklist 2>/dev/null | awk -v p="$1" '$2 == p && /sh\.exe/ { f = 1 } END { exit !(f || NR < 5) }'
}

# proof_lock <repo>: returns once this run holds the lock; at once inside a run that holds it
proof_lock() {
    [ -z "$PROOF_LOCKED" ] || return 0
    mkdir -p "$(dirname "$proof_lock_dir")"
    lock_said=
    until mkdir "$proof_lock_dir" 2>/dev/null; do
        lock_repo= lock_pid= lock_start=
        { read -r lock_repo; read -r lock_pid; read -r lock_start; } < "$proof_lock_dir/owner" 2>/dev/null
        if [ -z "$lock_pid" ]; then
            # between the other run's mkdir and its write: look once more before calling it stale
            sleep 2
            [ -s "$proof_lock_dir/owner" ] && continue
        fi
        if [ -z "$lock_pid" ] || ! proof_lock_alive "$lock_pid"; then
            # mv is atomic: of two waiting runs only one takes the stale lock away
            if mv "$proof_lock_dir" "$proof_lock_dir.stale.$$" 2>/dev/null; then
                rm -rf "$proof_lock_dir.stale.$$"
                echo "the proof lock of ${lock_repo:-an unknown run} (PID ${lock_pid:-?}, started ${lock_start:-?}) is stale: that run is gone, taking it over"
            fi
            continue
        fi
        if [ "$lock_said" != "$lock_pid" ]; then
            echo "waiting for the proof run in $lock_repo (started $lock_start)"
            lock_said=$lock_pid
        fi
        sleep 30
    done
    printf '%s\n%s\n%s\n' "$1" "$(cat /proc/$$/winpid)" "$(date '+%Y-%m-%d %H:%M:%S')" > "$proof_lock_dir/owner"
    proof_lock_mine=1
    export PROOF_LOCKED=1
    trap proof_unlock EXIT
    trap 'exit 130' INT TERM
}
