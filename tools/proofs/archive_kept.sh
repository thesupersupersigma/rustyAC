#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The game's kept recordings (re/gold/fresh/rec/<name>/<key>/, see lib.sh) whose key no run of
# today's sources asks for any more: a changed oracle or game install makes new keys and leaves the
# old folders behind. The current keys come from fresh.sh in its listing mode (PROOF_LIST_KEYS:
# nothing runs or records).
#   sh tools/proofs/archive_kept.sh           lists the old folders and their size (changes nothing)
#   sh tools/proofs/archive_kept.sh --move    sends them (a tar stream over ssh) to
#       $ARCHIVE_DEST/<date>/, checks every file's SHA-256 there against the one taken here, and
#       only then deletes them here
# ARCHIVE_DEST (default thesupersupersigma@10.0.0.140:/storage/rustyac-archive/kept) needs ssh
# without a password prompt. rclone is not used: plain tar + ssh/scp work everywhere here.
repo=$(cd "$(dirname "$0")/../.." && pwd)
cd "$repo" || exit 2
rec=re/gold/fresh/rec
work=target/proofs/archive
rm -rf "$work"; mkdir -p "$work"

# the folders a fresh run reads today (the multi parts included)
PROOF_LIST_KEYS=$repo/$work/keys.txt PROOF_OUT=$repo/$work/run PROOF_QUICK= PROOF_RERECORD= \
    FRESH_PARTS="car conditions awd hybrid sus survey crash multi" sh tools/proofs/fresh.sh > "$work/run.txt" 2>&1
sed "s|^$repo/||" "$work/keys.txt" 2>/dev/null | sort -u > "$work/current.txt"
# no keys at all is a broken listing, never "everything is old"
[ -s "$work/current.txt" ] || { echo "fresh.sh listed no keys (see $work/run.txt); nothing done" >&2; exit 2; }

find "$rec" -mindepth 2 -maxdepth 2 -type d | sort > "$work/all.txt"
comm -23 "$work/all.txt" "$work/current.txt" > "$work/old.txt"
missing=$(comm -13 "$work/all.txt" "$work/current.txt" | wc -l)
n=$(wc -l < "$work/old.txt")
kb=0
[ "$n" = 0 ] || kb=$(tr '\n' '\0' < "$work/old.txt" | xargs -0 du -sk | awk '{ s += $1 } END { print s }')
[ -z "$1" ] && tr '\n' '\0' < "$work/old.txt" | xargs -0 -r du -sh
echo "old kept folders: $n, $((kb / 1024)) MB; current keys: $(wc -l < "$work/current.txt") ($missing not kept yet)"
[ "$1" = --move ] || exit 0
[ "$n" = 0 ] && exit 0

dest=${ARCHIVE_DEST:-thesupersupersigma@10.0.0.140:/storage/rustyac-archive/kept}/$(date +%Y-%m-%d)
host=${dest%%:*}; dir=${dest#*:}
# every file's SHA-256 here, paths relative to re/gold/fresh/rec
sed "s|^$rec/||" "$work/old.txt" > "$work/rel.txt"
(cd "$rec" && tr '\n' '\0' < "$repo/$work/rel.txt" | xargs -0 find | sort | while read -r f; do
    [ -f "$f" ] && echo "$f"; done | tr '\n' '\0' | xargs -0 sha256sum) > "$work/kept.sha256" || exit 2
ssh -o BatchMode=yes "$host" "mkdir -p '$dir'" || exit 2
(cd "$rec" && tar -cf - -T "$repo/$work/rel.txt") | ssh -o BatchMode=yes "$host" "tar -xf - -C '$dir'" || { echo "copy failed; nothing deleted" >&2; exit 2; }
scp -q -B "$work/kept.sha256" "$host:$dir/kept.sha256" || exit 2
# the server hashes its own copies and must find every file of the list, byte for byte
if ! ssh -o BatchMode=yes "$host" "cd '$dir' && sha256sum --quiet -c kept.sha256"; then
    echo "SHA-256 differs on $host:$dir; nothing deleted" >&2; exit 2
fi
echo "SHA-256 matched: $(wc -l < "$work/kept.sha256") files on $host:$dir"
tr '\n' '\0' < "$work/old.txt" | xargs -0 rm -rf
find "$rec" -mindepth 1 -maxdepth 1 -type d -empty -delete
echo "moved $n folders, $((kb / 1024)) MB"
