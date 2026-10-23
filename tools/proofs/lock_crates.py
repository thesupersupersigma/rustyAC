# SPDX-License-Identifier: GPL-3.0-or-later
"""The crates of this repository a Cargo.lock change reaches (tools/proofs/areas.sh).

    python tools/proofs/lock_crates.py <Cargo.lock> <tag> [<rev>]

Prints the folders (relative to the repository, one a line) of the path crates whose entry in the
lock changed since <tag> (a dependency added, dropped or moved; their own version is ignored:
every release bumps it), and of the path crates that reach a changed outside package (a new
version, a new or dropped package) through outside packages only. A path crate's own folder
rule in areas.map then picks the areas, as a change to its files would. The graph is the lock's
own; the folders come from `cargo metadata --no-deps` (no network, no build). Exit 1 with the
reason on stderr when it can't tell (no lock at <tag>, cargo metadata fails, a dependency or a
path crate the lock names is not found).
"""
import json
import os
import subprocess
import sys
import tomllib


def fail(why):
    print(why, file=sys.stderr)
    sys.exit(1)


def packages(text):
    return tomllib.loads(text).get('package', [])


def key(p):
    # a path crate is the same crate whatever its version
    return (p['name'], None if 'source' not in p else p['version'])


def entry(p):
    q = {k: v for k, v in p.items() if not ('source' not in p and k == 'version')}
    return json.dumps(q, sort_keys=True)


def main():
    if len(sys.argv) not in (3, 4):
        fail('usage: lock_crates.py <Cargo.lock> <tag> [<rev>]')
    lock, tag = sys.argv[1], sys.argv[2]
    # <rev>: the lock as committed there instead of the working copy (areas_test.sh)
    rev = sys.argv[3] if len(sys.argv) == 4 else None
    repo = subprocess.run(['git', 'rev-parse', '--show-toplevel'], capture_output=True, text=True).stdout.strip()
    rel = os.path.relpath(os.path.abspath(lock), repo).replace('\\', '/')
    def at(commit):
        shown = subprocess.run(['git', '-C', repo, 'show', f'{commit}:{rel}'], capture_output=True, text=True, encoding='utf-8')
        if shown.returncode != 0:
            fail(f'{rel} is not in {commit}')
        return shown.stdout

    try:
        before = {key(p): entry(p) for p in packages(at(tag))}
        now = packages(at(rev) if rev else open(lock, encoding='utf-8').read())
    except (tomllib.TOMLDecodeError, KeyError, OSError) as e:
        fail(f'{rel}: {e}')
    changed = {key(p) for p in now if before.get(key(p)) != entry(p)}

    manifest = os.path.join(os.path.dirname(os.path.abspath(lock)), 'Cargo.toml')
    meta = subprocess.run(['cargo', 'metadata', '--format-version', '1', '--no-deps', '--manifest-path', manifest], capture_output=True, text=True, encoding='utf-8')
    if meta.returncode != 0:
        fail(f'cargo metadata of {manifest}: {meta.stderr.strip().splitlines()[-1:]}')
    folder = {}
    for p in json.loads(meta.stdout)['packages']:
        if p['source'] is None:
            folder[p['name']] = os.path.relpath(os.path.dirname(p['manifest_path']), repo).replace('\\', '/')

    # the lock's graph: "name", "name version" or "name version (source)"
    by_name = {}
    for p in now:
        by_name.setdefault(p['name'], []).append(p)

    def deps(p):
        out = []
        for d in p.get('dependencies', []):
            parts = d.split(' ')
            hits = [q for q in by_name.get(parts[0], []) if len(parts) == 1 or q['version'] == parts[1]]
            if len(hits) != 1:
                fail(f'{rel}: the dependency "{d}" of {p["name"]} is not one package of the lock')
            out.append(hits[0])
        return out

    def reaches(p, seen):
        # through outside packages only: a path crate on the way answers for itself
        for d in deps(p):
            if key(d) in seen:
                continue
            seen.add(key(d))
            if 'source' in d and (key(d) in changed or reaches(d, seen)):
                return True
        return False

    picked = set()
    for p in now:
        if 'source' in p:
            continue
        if p['name'] not in folder:
            fail(f'{rel}: the path crate {p["name"]} is not in cargo metadata')
        if key(p) in changed or reaches(p, set()):
            picked.add(folder[p['name']])
    for f in sorted(picked):
        print(f)


if __name__ == '__main__':
    main()
