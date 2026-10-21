# SPDX-License-Identifier: GPL-3.0-or-later
"""A local static server to try the browser preview.

    python web/serve.py                                   http://127.0.0.1:8080/
    python web/serve.py --port 9000 --pack dist-web/preview
    python web/serve.py --ac "C:/.../assettocorsa"        also serves that folder read-only at
                                                          /ac-test/ (the test hook of the folder
                                                          picker: ?testfolder=/ac-test/)
    python web/serve.py --fault cut:collider.kn5          a test of the page's downloads: the
                                                          first answer for a path containing the
                                                          text goes wrong once (cut: half, then
                                                          the connection closes; wrong: one byte
                                                          changed; zero: an empty 200; short: a
                                                          shorter file than the list says; 503)

Serves dist-web/site/ at / and, when it exists, the preview pack at /preview/ (where the page
looks for `preview/manifest.json`). It listens on this computer only. It never writes anything.
"""
import argparse
import http.server
import json
import mimetypes
import os
import posixpath
import urllib.parse

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
MIME = {'.wasm': 'application/wasm', '.js': 'text/javascript', '.html': 'text/html; charset=utf-8', '.css': 'text/css', '.json': 'application/json', '.ttf': 'font/ttf'}


def make_handler(mounts):
    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.1'

        def log_message(self, format, *args):
            if not self.server.quiet:
                super().log_message(format, *args)

        def resolve(self):
            """(file system path, query) of the request, or (None, query)."""
            parsed = urllib.parse.urlsplit(self.path)
            path = posixpath.normpath(urllib.parse.unquote(parsed.path))
            query = urllib.parse.parse_qs(parsed.query)
            for prefix, folder in mounts:
                if path == prefix.rstrip('/') or path.startswith(prefix):
                    rest = path[len(prefix):].lstrip('/')
                    full = os.path.normpath(os.path.join(folder, *rest.split('/'))) if rest else folder
                    try:
                        inside = os.path.commonpath([full, folder]) == folder
                    except ValueError:
                        # (another drive letter in the request)
                        inside = False
                    return (full if inside else None), query
            return None, query

        def send(self, status, body, kind='text/plain; charset=utf-8'):
            self.send_response(status)
            self.send_header('Content-Type', kind)
            self.send_header('Content-Length', str(len(body)))
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            if self.command != 'HEAD':
                self.wfile.write(body)

        def do_HEAD(self):
            self.do_GET()

        def do_GET(self):
            # only this computer's own pages may ask: a name that merely resolves to 127.0.0.1
            # (DNS rebinding) is turned away
            host = (self.headers.get('Host') or '').split(':')[0].lower()
            if host not in ('127.0.0.1', 'localhost', '[::1]'):
                return self.send(403, b'this server only answers to 127.0.0.1 / localhost')
            full, query = self.resolve()
            if full is None:
                return self.send(404, b'not found')
            if 'list' in query and os.path.isdir(full):
                # the folder picker's test hook: a folder's entries as JSON
                entries = []
                with os.scandir(full) as it:
                    for entry in it:
                        stat = entry.stat()
                        entries.append({'name': entry.name, 'dir': entry.is_dir(), 'size': 0 if entry.is_dir() else stat.st_size, 'mtime': stat.st_mtime_ns / 1_000_000})
                return self.send(200, json.dumps(entries).encode(), 'application/json')
            if os.path.isdir(full):
                full = os.path.join(full, 'index.html')
            if not os.path.isfile(full):
                return self.send(404, b'not found')
            size = os.path.getsize(full)
            kind = MIME.get(os.path.splitext(full)[1].lower()) or mimetypes.guess_type(full)[0] or 'application/octet-stream'
            fault = None
            if self.command == 'GET':
                for i, (what, text) in enumerate(self.server.faults):
                    if text in self.path:
                        fault = what
                        self.server.faults.pop(i)
                        self.log_message('fault %s for %s', what, self.path)
                        break
            if fault == '503':
                return self.send(503, b'a test fault')
            # one range "bytes=N-" (what the page asks for to resume a download)
            start = 0
            ranged = (self.headers.get('Range') or '').strip()
            if ranged.startswith('bytes=') and ranged.endswith('-') and ranged[6:-1].isdigit() and int(ranged[6:-1]) < size:
                start = int(ranged[6:-1])
            length = 0 if fault == 'zero' else size - start - (1 if fault == 'short' else 0)
            self.send_response(206 if start else 200)
            self.send_header('Content-Type', kind)
            self.send_header('Content-Length', str(length))
            self.send_header('Accept-Ranges', 'bytes')
            if start:
                self.send_header('Content-Range', f'bytes {start}-{size - 1}/{size}')
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            if self.command == 'HEAD':
                return
            with open(full, 'rb') as f:
                f.seek(start)
                left = length
                sent = 0
                while left > 0:
                    block = f.read(min(1 << 20, left))
                    if not block:
                        break
                    if fault == 'wrong' and sent <= length // 2 < sent + len(block):
                        block = bytearray(block)
                        block[length // 2 - sent] ^= 0xFF
                        block = bytes(block)
                    if fault == 'cut' and sent + len(block) > length // 2:
                        # half of it, then the connection breaks off
                        try:
                            self.wfile.write(block[:length // 2 - sent])
                            self.wfile.flush()
                        except (BrokenPipeError, ConnectionError):
                            pass
                        self.close_connection = True
                        self.connection.shutdown(2)
                        return
                    try:
                        self.wfile.write(block)
                    except (BrokenPipeError, ConnectionError):
                        return
                    sent += len(block)
                    left -= len(block)

    return Handler


def main():
    parser = argparse.ArgumentParser(description='Serve the browser preview on this computer')
    parser.add_argument('--site', default=os.path.join(ROOT, 'dist-web', 'site'))
    parser.add_argument('--pack', default=os.path.join(ROOT, 'dist-web', 'preview'))
    parser.add_argument('--ac', help='an Assetto Corsa folder to serve read-only at /ac-test/ (test hook)')
    parser.add_argument('--port', type=int, default=8080)
    parser.add_argument('--quiet', action='store_true')
    parser.add_argument('--fault', action='append', default=[], help='KIND:TEXT, once each (a test of the page; see above)')
    args = parser.parse_args()
    if not os.path.isfile(os.path.join(args.site, 'index.html')):
        raise SystemExit(f'{args.site} has no index.html: run `python web/build.py` first')
    mounts = []
    if os.path.isdir(args.pack):
        mounts.append(('/preview/', os.path.abspath(args.pack)))
    if args.ac:
        mounts.append(('/ac-test/', os.path.abspath(args.ac)))
    mounts.append(('/', os.path.abspath(args.site)))
    server = http.server.ThreadingHTTPServer(('127.0.0.1', args.port), make_handler(mounts))
    server.quiet = args.quiet
    server.faults = []
    for fault in args.fault:
        what, _, text = fault.partition(':')
        if what not in ('cut', 'wrong', 'zero', 'short', '503') or not text:
            raise SystemExit(f'--fault {fault}: KIND:TEXT with KIND cut, wrong, zero, short or 503')
        server.faults.append((what, text))
    server.daemon_threads = True
    print(f'rustyAC browser preview: http://127.0.0.1:{args.port}/', flush=True)
    for prefix, folder in mounts:
        print(f'  {prefix:<10} {folder}', flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == '__main__':
    main()
