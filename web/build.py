# SPDX-License-Identifier: GPL-3.0-or-later
"""Builds rustyAC's browser preview into dist-web/site/.

    python web/build.py                the site (wasm optimised with wasm-opt)
    python web/build.py --no-opt       skip wasm-opt (quicker, a larger file)
    python web/build.py --zip <file>   also pack the site into a zip (what a release attaches)

What it does:
  1. makes sure Rust can build for wasm32-unknown-unknown: `rustup target add` where there is
     rustup, otherwise it downloads Rust's own `rust-std` for the installed version into
     target/wasm-sysroot (checked against the hash rust-lang.org publishes) and points rustc at it
  2. cargo build --release --locked -p rustyac-web --target wasm32-unknown-unknown
     (no SIMD, no fast-math: the build's floating point is the desktop's, see docs/port/web.md)
  3. wasm-bindgen (JS glue) and wasm-opt, in the versions pinned below, downloaded into
     target/web-tools on first use and checked against the hashes below
  4. copies web/index.html, app.js, style.css next to them

The site holds no Assetto Corsa file. Content comes from the player's own folder, or from a
preview pack that `tools/web_pack` builds (dist-web/preview/, never committed, never released).
"""
import argparse
import hashlib
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import urllib.request
import zipfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
TARGET = 'wasm32-unknown-unknown'
# must be the wasm-bindgen version of crates/rustyac-web/Cargo.toml
WASM_BINDGEN = '0.2.129'
BINARYEN = 'version_123'
TOOLS = {
    ('Windows', 'wasm-bindgen'): (
        f'https://github.com/rustwasm/wasm-bindgen/releases/download/{WASM_BINDGEN}/wasm-bindgen-{WASM_BINDGEN}-x86_64-pc-windows-msvc.tar.gz',
        '79e348c169d0c10c0b287647f1e753d0985353ca96800bc29be8f4261baf13e6',
        f'wasm-bindgen-{WASM_BINDGEN}-x86_64-pc-windows-msvc/wasm-bindgen.exe'),
    ('Windows', 'wasm-opt'): (
        f'https://github.com/WebAssembly/binaryen/releases/download/{BINARYEN}/binaryen-{BINARYEN}-x86_64-windows.tar.gz',
        '7b3568424a0f871a52865d5c78177db646b1832a8c487321e27703103f936880',
        f'binaryen-{BINARYEN}/bin/wasm-opt.exe'),
    ('Linux', 'wasm-bindgen'): (
        f'https://github.com/rustwasm/wasm-bindgen/releases/download/{WASM_BINDGEN}/wasm-bindgen-{WASM_BINDGEN}-x86_64-unknown-linux-musl.tar.gz',
        '82d12bb940e2d4e72e0d5605387fc1b8ca179044e012b620f0ce4e7440e8320e',
        f'wasm-bindgen-{WASM_BINDGEN}-x86_64-unknown-linux-musl/wasm-bindgen'),
    ('Linux', 'wasm-opt'): (
        f'https://github.com/WebAssembly/binaryen/releases/download/{BINARYEN}/binaryen-{BINARYEN}-x86_64-linux.tar.gz',
        'e959f2170af4c20c552e9de3a0253704d6a9d2766e8fdb88e4d6ac4bae9388fe',
        f'binaryen-{BINARYEN}/bin/wasm-opt'),
}
SITE_FILES = ['index.html', 'app.js', 'style.css']


def say(text):
    print(text, flush=True)


def run(command, **kwargs):
    say('> ' + ' '.join(command))
    subprocess.run(command, check=True, **kwargs)


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()


def download(url, path):
    say(f'downloading {url}')
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with urllib.request.urlopen(url) as response, open(path + '.part', 'wb') as out:
        shutil.copyfileobj(response, out)
    os.replace(path + '.part', path)


def tool(name, given):
    """The pinned program, downloaded and checked on first use."""
    if given:
        return given
    key = (platform.system(), name)
    if key not in TOOLS or platform.machine().lower() not in ('amd64', 'x86_64'):
        sys.exit(f'no pinned download of {name} for this machine: install it '
                 f'(wasm-bindgen-cli {WASM_BINDGEN}, binaryen {BINARYEN}) and pass --{name} <path>')
    url, digest, inside = TOOLS[key]
    folder = os.path.join(ROOT, 'target', 'web-tools')
    program = os.path.join(folder, *inside.split('/'))
    if not os.path.isfile(program):
        archive = os.path.join(folder, os.path.basename(url))
        if not os.path.isfile(archive) or sha256(archive) != digest:
            download(url, archive)
        if sha256(archive) != digest:
            sys.exit(f'{archive}: SHA-256 is not the pinned {digest}')
        with tarfile.open(archive) as tar:
            tar.extractall(folder)
    return program


def wasm_target_env():
    """The environment that lets cargo build for wasm, installing Rust's wasm std if needed."""
    env = dict(os.environ)
    libdir = subprocess.run(['rustc', '--print', 'target-libdir', '--target', TARGET], capture_output=True, text=True).stdout.strip()
    if libdir and os.path.isdir(libdir) and any(n.startswith('libstd') for n in os.listdir(libdir)):
        return env
    if shutil.which('rustup'):
        run(['rustup', 'target', 'add', TARGET])
        return env
    # a Rust without rustup (the stand-alone installer): Rust's own std for the target, unpacked
    # inside the repository
    version = subprocess.run(['rustc', '--version'], capture_output=True, text=True, check=True).stdout.split()[1]
    sysroot = os.path.join(ROOT, 'target', 'wasm-sysroot')
    lib = os.path.join(sysroot, 'lib', 'rustlib', TARGET, 'lib')
    marker = os.path.join(sysroot, f'{TARGET}-{version}.ok')
    if not os.path.isfile(marker):
        name = f'rust-std-{version}-{TARGET}'
        archive = os.path.join(sysroot, 'dl', name + '.tar.xz')
        url = f'https://static.rust-lang.org/dist/{name}.tar.xz'
        download(url, archive)
        with urllib.request.urlopen(url + '.sha256') as response:
            expected = response.read().decode().split()[0]
        if sha256(archive) != expected:
            sys.exit(f'{archive}: SHA-256 is not the one rust-lang.org publishes ({expected})')
        unpacked = os.path.join(sysroot, 'dl')
        with tarfile.open(archive) as tar:
            tar.extractall(unpacked)
        shutil.rmtree(os.path.join(sysroot, 'lib', 'rustlib', TARGET), ignore_errors=True)
        shutil.copytree(os.path.join(unpacked, name, f'rust-std-{TARGET}', 'lib', 'rustlib', TARGET), os.path.join(sysroot, 'lib', 'rustlib', TARGET))
        shutil.rmtree(os.path.join(unpacked, name), ignore_errors=True)
        open(marker, 'w').close()
    assert os.path.isdir(lib), lib
    key = 'CARGO_TARGET_' + TARGET.upper().replace('-', '_') + '_RUSTFLAGS'
    env[key] = (env.get(key, '') + ' --sysroot ' + sysroot.replace('\\', '/')).strip()
    say(f'no rustup: Rust\'s std for {TARGET} is in {sysroot}')
    return env


def megabytes(path):
    return os.path.getsize(path) / 1048576


def main():
    parser = argparse.ArgumentParser(description='Build the browser preview into dist-web/site/')
    parser.add_argument('--out', default=os.path.join(ROOT, 'dist-web', 'site'))
    parser.add_argument('--no-opt', action='store_true', help='skip wasm-opt')
    parser.add_argument('--zip', help='also write the site as this zip file')
    parser.add_argument('--wasm-bindgen', help='path to wasm-bindgen ' + WASM_BINDGEN)
    parser.add_argument('--wasm-opt', help='path to wasm-opt')
    args = parser.parse_args()

    env = wasm_target_env()
    run(['cargo', 'build', '--release', '--locked', '-p', 'rustyac-web', '--lib', '--target', TARGET], cwd=ROOT, env=env)
    built = os.path.join(ROOT, 'target', TARGET, 'release', 'rustyac_web.wasm')

    out = os.path.abspath(args.out)
    pkg = os.path.join(out, 'pkg')
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(pkg)
    run([tool('wasm-bindgen', args.wasm_bindgen), '--target', 'web', '--no-typescript', '--out-dir', pkg, '--out-name', 'rustyac_web', built])
    wasm = os.path.join(pkg, 'rustyac_web_bg.wasm')
    before = megabytes(wasm)
    if not args.no_opt:
        # -O3 keeps IEEE arithmetic as it is (binaryen only reorders floats with --fast-math, which
        # is not given); only the features rustc's wasm32 baseline uses are switched on
        features = ['--enable-bulk-memory', '--enable-bulk-memory-opt', '--enable-nontrapping-float-to-int', '--enable-sign-ext',
                    '--enable-mutable-globals', '--enable-reference-types', '--enable-multivalue', '--enable-call-indirect-overlong']
        run([tool('wasm-opt', args.wasm_opt), '-O3', *features, wasm, '-o', wasm + '.opt'])
        os.replace(wasm + '.opt', wasm)
    for name in SITE_FILES:
        shutil.copyfile(os.path.join(ROOT, 'web', name), os.path.join(out, name))
    for name in ['LICENSE-GPL', 'LICENSING.md']:
        shutil.copyfile(os.path.join(ROOT, name), os.path.join(out, name))

    files = []
    for folder, _, names in os.walk(out):
        files += [os.path.join(folder, n) for n in names]
    say(f'\nsite: {out}')
    for path in sorted(files):
        say(f'  {os.path.getsize(path):>10,}  {os.path.relpath(path, out)}')
    total = sum(os.path.getsize(p) for p in files)
    say(f'  wasm {megabytes(wasm):.2f} MB' + ('' if args.no_opt else f' (before wasm-opt: {before:.2f} MB)') + f'; the whole site {total / 1048576:.2f} MB')
    if args.zip:
        os.makedirs(os.path.dirname(os.path.abspath(args.zip)), exist_ok=True)
        with zipfile.ZipFile(args.zip, 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as z:
            for path in sorted(files):
                z.write(path, os.path.relpath(path, out).replace('\\', '/'))
        say(f'zip: {args.zip} ({megabytes(args.zip):.2f} MB, sha256 {sha256(args.zip)})')
    say('try it: python web/serve.py')


if __name__ == '__main__':
    main()
