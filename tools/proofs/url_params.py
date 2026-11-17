# SPDX-License-Identifier: GPL-3.0-or-later
"""The page's address parameters against docs/flags.md (the browser area of the proofs).

    python tools/proofs/url_params.py

A parameter the page reads (web/app.js: params.get('x') / params.has('x'); web/theme_glue.js: its
own read of the address) and the page "The browser page" of docs/flags.md does not name, or one
the page names and no script reads, is printed and the exit code is 1.
"""
import pathlib
import re
import sys

repo = pathlib.Path(__file__).resolve().parents[2]
read = set(re.findall(r"params\.(?:get|has)\('([a-z]+)'\)", (repo / 'web/app.js').read_text(encoding='utf-8')))
read |= set(re.findall(r"location\.search\)\.get\('([a-z]+)'\)", (repo / 'web/theme_glue.js').read_text(encoding='utf-8')))
assert len(read) > 30, 'the page\'s reads were not found'
section = next(s for s in (repo / 'docs/flags.md').read_text(encoding='utf-8').split('\n## ') if s.startswith('The browser page\n'))
# on the page a parameter is a `?name` or `?name=value` code piece (a longer example: its &name= too)
named = {m for code in section.split('`')[1::2] if code.startswith('?') for m in re.findall(r'[?&]([a-z]+)', code)}
for name in sorted(read - named):
    print(f'?{name}: read by the page, not in docs/flags.md')
for name in sorted(named - read):
    print(f'?{name}: in docs/flags.md, not read by the page')
print(f'{len(read)} address parameters')
sys.exit(read != named)
