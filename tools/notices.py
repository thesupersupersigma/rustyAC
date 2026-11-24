# SPDX-License-Identifier: MIT OR Apache-2.0
"""Writes THIRD_PARTY_NOTICES.txt: the license text and copyright notice of every third-party crate in
Cargo.lock and tools/*/Cargo.lock, which the release zips carry (packaging/package.ps1, web/build.py).

    cargo install cargo-about --version 0.9.3 --locked --features cli    (once; see LICENSING.md)
    python tools/notices.py

Run it again whenever a lock file changes.
"""
import glob
import json
import os
import re
import subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main():
    manifests = ['Cargo.toml'] + sorted(os.path.relpath(os.path.dirname(p), ROOT).replace('\\', '/') + '/Cargo.toml'
                                        for p in glob.glob(os.path.join(ROOT, 'tools', '*', 'Cargo.lock')))
    json_path = os.path.join(ROOT, 'target', 'notices.json')
    os.makedirs(os.path.dirname(json_path), exist_ok=True)
    texts = {}  # (license id, text) -> the crates under it
    for manifest in manifests:
        # into a file: cargo-about refuses to write to a pipe on Windows
        out = subprocess.run(['cargo', 'about', 'generate', '--format', 'json', '--locked', '--all-features', '-o', json_path,
                              '-c', 'tools/about.toml', '--manifest-path', manifest],
                             cwd=ROOT, capture_output=True, text=True, encoding='utf-8')
        if out.returncode != 0:
            raise SystemExit(f'cargo about failed on {manifest}:\n{out.stderr}')
        with open(json_path, encoding='utf-8') as f:
            licenses = json.load(f)['licenses']
        for lic in licenses:
            text = '\n'.join(line.rstrip() for line in lic['text'].replace('\r\n', '\n').strip().split('\n'))
            crates = texts.setdefault((lic['id'], text), set())
            crates.update(f"{u['crate']['name']} {u['crate']['version']}" for u in lic['used_by'])
    crates = {c for used in texts.values() for c in used}
    # a lock file also lists optional dependencies that no build here turns on: never compiled
    locked = set()
    for manifest in manifests:
        lock = open(os.path.join(ROOT, os.path.dirname(manifest), 'Cargo.lock'), encoding='utf-8').read()
        locked |= {f'{n} {v}' for n, v in re.findall(r'name = "([^"]+)"\nversion = "([^"]+)"\nsource = "registry', lock)}
    unbuilt = sorted(locked - crates)
    lines = ['Third-party notices for rustyAC',
             '',
             f'rustyAC builds on these {len(crates)} crates (Rust libraries) from the lock files of its',
             'repository; not every one is in every program. Below, each license text comes after the',
             'list of crates it covers, as the crate itself ships it (with its copyright lines). Made',
             'by tools/notices.py with cargo-about. See LICENSING.md for rustyAC\'s own licenses.',
             '',
             f'The lock files also name {len(unbuilt)} optional dependencies that no rustyAC build turns on, so',
             f'they are never compiled or shipped: {", ".join(unbuilt)}.',
             '']
    for (lic, text), used in sorted(texts.items(), key=lambda t: (t[0][0], sorted(t[1])[0])):
        lines += ['=' * 100, f'{lic}, used by: {", ".join(sorted(used))}', '=' * 100, '', text, '']
    # not a crate: crates/rustyac-ode's feature `embree` builds it from source (LICENSE.txt there is LICENSE-APACHE)
    apache = open(os.path.join(ROOT, 'LICENSE-APACHE'), encoding='utf-8').read().replace('\r\n', '\n').strip()
    lines += ['=' * 100, 'Apache-2.0, used by: Embree 3.13.4 (C++ library, the Custom Shaders Patch mode\'s ray casts)', '=' * 100, '',
              'Copyright 2009-2021 Intel Corporation', '', apache, '']
    # not a crate: crates/rustyac-lua's feature `luajit` builds it from source; its COPYRIGHT: MIT for
    # LuaJIT and the Lua 5.1/5.2 code in it (the same permission text as LICENSE-MIT), dlmalloc public domain
    mit = open(os.path.join(ROOT, 'LICENSE-MIT'), encoding='utf-8').read().replace('\r\n', '\n').strip()
    mit = mit[mit.index('Permission is hereby'):]
    lines += ['=' * 100, 'MIT, used by: LuaJIT 2.1 (C library, the Custom Shaders Patch mode\'s Lua)', '=' * 100, '',
              'Copyright (C) 2005-2026 Mike Pall. All rights reserved.', '',
              'LuaJIT includes code from Lua 5.1/5.2: Copyright (C) 1994-2012 Lua.org, PUC-Rio.', '', mit, '',
              'LuaJIT includes code from dlmalloc, which has this license statement:', '',
              'This is a version (aka dlmalloc) of malloc/free/realloc written by', 'Doug Lea and released to the public domain, as explained at',
              'https://creativecommons.org/licenses/publicdomain', '']
    with open(os.path.join(ROOT, 'THIRD_PARTY_NOTICES.txt'), 'w', encoding='utf-8', newline='\n') as f:
        f.write('\n'.join(lines))
    print(f'THIRD_PARTY_NOTICES.txt: {len(crates)} crates, {len(texts)} license texts, from {len(manifests)} lock files')


if __name__ == '__main__':
    main()
